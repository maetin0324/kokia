# kokia 追加計画書：ネストした `poll` から「タスク構造（await グラフ）」を復元する

目的：`GenFuture::poll` ヒットの**入退場イベント**だけから、親子（await）関係を**ランタイム非依存**に推定し、
「通常の関数」と同じ操作性で **論理スタック**・**タスク一覧**・**子タスクリンク**・**停止点ローカル** を提供する。

---

## 1. 用語と前提

* **Task**: 生成器オブジェクト（`GenFuture` の `self`）で一意に識別。`TaskId = self_ptr`.
* **Parent**: 親の `GenFuture::poll` の実行中に**子**の `GenFuture::poll` が呼ばれたときの、その親タスク。
* **Edge**: `Parent --awaits--> Child`（親が子を `.await` している事実の観測）。
* **Callsite**: 親タスクの `.await` 停止点（ソース行/場所）。**discriminant** と行番号で ID 化。
* **入退場イベント**: `GenFuture::poll` の **entry** と **exit**。exit は `ret`／unwind 双方を扱う。

---

## 2. 観測モデル（高レベル設計）

1. **正規表現 BP** を `core::future::from_generator::GenFuture<...>::poll` に設定（v0 マングリング対応）。
2. **entry** で「今からポーリングされる生成器（子）」を捕捉し、**同一 OS スレッドのスタック**から最も近い**親 `GenFuture::poll` フレーム**を探索 → `Parent` 推定。
3. **edge 登録**：`Parent -> Child`。併せて

   * 親の **現在 discriminant**（停止点）を読み、
   * 親フレームの **RIP → 行番号**を引いて **callsite_id** を特定（`(parent_task_id, suspend_idx, file:line)`）。
4. **exit** で `Poll::Ready/Pending` を確定。`Ready` なら「その callsite の await は解決済み」フラグ。
5. これを**全スレッド**で継続収集 → **Task Graph（有向非巡回）** と **論理スタック（動的スコープ）**が得られる。

> ポイント：**ランタイム非依存**（Tokio/async-std 等の内部には触れない）。
> 親子判定は **“同一スレッド上の最近傍 GenFuture フレーム”** という**安定した経験則**に基づく。

---

## 3. 低レベル実装戦略

### 3.1 ブレークポイント設置と関数境界

* **entry**: `GenFuture::poll` シンボル範囲先頭に SW-BP（INT3）。
* **exit**: 同関数内の **すべての `ret`** に**一過性（temporal）BP**を自動配置。

  * 関数範囲は `DW_AT_low_pc/high_pc` または ELF シンボルサイズから取得。
  * 初回ヒット時に関数バイト列を `capstone` で逆アセンブルし `ret` アドレス群を列挙 → BP を張る。
* **unwind exit**: パニック等で `ret` を経ずに抜けるケースは、**次の停止時**にスレッドの**実 OS スタックから poll ネストを再構成**し、差分で **pop** を補正（詳細 3.5）。

### 3.2 スレッド別「動的 poll スコープ」管理

```rust
struct ThreadPollScope {
    // ネスト順に子→親（top が現在実行中）
    stack: Vec<TaskId>,
    // 直近フレームスキャン結果のキャッシュ（高速化）
    last_scan_fingerprint: StackFp,
}
```

* **entry**: `child` を push。`parent` は push 前の `stack.last()` か、後述のフレームスキャン結果を優先。
* **exit**: `child` を pop（不整合なら 3.5 の“再同期”へ）。

### 3.3 親フレーム探索（フレームスキャン）

* **現在スレッドのバックトレース**を `kokia-unwind` で取得。
* 上位フレーム（古い方）へ向け **最初に現れる `GenFuture::poll`** を親とみなす。
* もし見つからなければ **root タスク**（spawn された独立タスク）として扱う。
* **高速化**:

  * 各フレーム RIP → `is_genfuture_poll` 判定を**アドレス帯域キャッシュ**（`FxHashMap<Range<u64>, bool>`）。
  * スキャンは **上限フレーム数**や **時間制限**を持つ（重い時は `stack.last()` フォールバック）。

### 3.4 discriminant・ローカル・callsite 同定

* **discriminant**: DWARF から generator 型メタを引き、**判別子オフセット**を読取 → 停止点 index。
* **ローカル**: active **variant** 内の `Field {name, offset, size, ty}` を列挙し `self_ptr + offset` で値を読む。
* **callsite**: 親フレームの `RIP` を `addr2line` → `file:line`。

  * さらに **generator 側の停止点 index**と突き合わせ、`callsite_id = hash(parent_task_id, suspend_idx, file:line)`。

### 3.5 再同期（例外・最適化・非対称制御）

* 条件：

  * `exit` BP を経由せずに **entry** に再度入った、
  * SIG で中断、`longjmp`/panic で戻った、
  * BP 無効化中に実行が進んだ…等。
* 再同期手順：

  1. 現在の OS スタックから **“`GenFuture::poll` の実在列”** を抽出し `actual = [child, ..., root]`。
  2. `ThreadPollScope.stack` と `actual` の **最長共通接頭辞**を取り、差分を **pop/push** で整合化。
  3. これにより **親子リンク推定**の誤りを漸進的に矯正。

### 3.6 `Ready/Pending` 判定（exit）

* **戻り値 ABI**を DWARF 型から解読（`Poll<T>` のディスクリミナント位置）し、`Ready/Pending` を確定。
* `Ready` なら `Edge{parent->child, callsite_id}.completed = true`。

### 3.7 タスク同一性・再利用対策

* 生成器のメモリ再利用に備え、**指紋**を持たせる：

  * `(self_ptr, type_name_hash, first_seen_ts)` を `TaskKey` として採用。
  * さらに `self_ptr` 先頭数十バイトの **初期スナップショット**をハッシュ化して補助キーに。

---

## 4. データモデル

```rust
type TaskId = u64; // self_ptr

struct TaskInfo {
    id: TaskId,
    type_name: Option<String>,
    first_seen: Instant,
    last_seen: Instant,
    current_discriminant: Option<u64>,
    last_rip: Option<u64>,          // 観測時のIP（ソース相関に使用）
    last_thread: Tid,
    roots: bool,                    // 親なし観測
    completed: bool,                // 最終Ready観測後にtrue
}

struct EdgeId(u128); // hash(parent, child, callsite_id)

struct Edge {
    parent: TaskId,
    child: TaskId,
    callsite: CallsiteId,
    first_seen: Instant,
    last_seen: Instant,
    completed: bool,
}

struct CallsiteId(u128);
struct Callsite {
    parent: TaskId,            // 正規化用に parent の fingerprint も含める
    suspend_idx: Option<u32>,  // generator 停止点 index
    file: Option<String>,
    line: Option<u32>,
}
```

---

## 5. API（`kokia-async`）とイベントフロー

```rust
// エンジンから呼ばれるフック
fn on_poll_entry(tid: Tid, rip: u64, regs: &Regs);
fn on_poll_exit(tid: Tid, rip: u64, regs: &Regs, retval: Option<EnumVal>);
fn on_thread_stop(tid: Tid); // 再同期契機

// クエリ系（CLI/TUI やコマンドから呼ぶ）
fn async_bt(current_tid: Tid) -> Vec<TaskId>;          // 直近の論理スタック（子→親）
fn async_tasks() -> Vec<TaskSummary>;
fn async_locals(task: TaskId) -> Vec<Var>;             // 停止点ローカル（variant live のみ）
fn async_edges(parent: Option<TaskId>) -> Vec<Edge>;   // 観測済み await グラフ
```

**イベント順序（典型）**

1. INT3（entry）→ `on_poll_entry`：`child` を push、親探索→ `Edge` 登録、discriminant/locals 更新
2. 実行 → INT3（exit）→ `on_poll_exit`：`Ready/Pending` 判定、`child` を pop
3. 以降繰り返し。非対称時は `on_thread_stop` で再同期。

---

## 6. 親子推定ルール（曖昧時の優先順）

1. **フレームスキャン親**（最寄り上位の `GenFuture::poll`）
2. ダメなら **動的スコープ最上位**（`ThreadPollScope.stack.last()`）
3. それも無ければ **root** とする

> 1 が最も信頼性が高い。2 はパフォーマンス重視モードのフォールバック。
> 3 は spawn 直下に対応（親不在）。

---

## 7. 様々な async パターンへの対応

* **`select!` / `tokio::select!`**：親が複数子を交互に `poll`。各子ごとに **別 `Edge`** が観測される。`Ready` 観測で当該 `callsite` を completed に。
* **combinator（`join`, `map`, `then` 等）**：多層の `Future::poll` が挟まるが、**親子は “GenFuture↔GenFuture” 間でのみ張る**。中間層は**構造ノイズ**とみなし論理スタックから除外（オプションで表示可）。
* **`Pin<Box<dyn Future>>`**：動的 dispatch でも最終的に具象 `poll` に到達 → `GenFuture` で捕捉可能。
* **`spawn` 系**：親フレームに `GenFuture` が存在しないため **root** として扱う。`JoinHandle.await` 側で別の辺が張られる（オプション）。
* **`panic!` / `unwind`**：3.5 の再同期で矛盾を解消。`child` が未 `ret` で消える場合があるため、`TaskInfo.completed` は **Ready 観測時のみ true** とする。

---

## 8. タイムトラベル連携（rr / lite）

* **rr**：entry/exit の**順序が完全再現**されるため、`reverse-step/continue` 時には

  * 直前イベントへ戻し **ThreadPollScope** を**巻き戻し**、グラフを一貫化。
* **lite**：チェックポイント境界で `tasks/edges` を**スナップショット**保存し、復元時に再適用。

---

## 9. パフォーマンス最適化

* **ディスアセンブル一度きり**：関数ごとに `ret` アドレス列をキャッシュ。
* **フレームスキャンの早期停止**：一定フレーム数上限（例: 64）。`GenFuture` 判定は帯域キャッシュ。
* **サンプリング**：`GenFuture::poll` entry の観測比率を設定（1/N）。**構造抽出フェーズ**は 1/1、**運用フェーズ**は 1/8 等。
* **DWARF 検索キャッシュ**：型解決（生成器 variant, discriminant offset）を `TypeIdHash` キーでメモ。

---

## 10. 可観測性の欠落に対するフォールバック

* **LTO/最適化で `GenFuture::poll` が潰れた**

  * 推奨フラグ（`-C debuginfo=2 -C force-frame-pointers=yes`）のガイダンスを UI に表示。
  * 代替として「**汎用 `<T as Future>::poll` パターン**」へ BP 範囲を拡大（誤検知はフレームスキャンで抑制）。
* **ローカルが optimized-out**

  * variant フィールドが見えない箇所は `optimized-out` 表示。
  * 任意で `#[inline(never)]` / `-C opt-level=0` を推奨。

---

## 11. テストシナリオ

1. **直列 await**：`async fn a()->await b()->await c()`

   * 期待：`a -> b`, `b -> c` の辺。discriminant が段階的に変化。
2. **並列 select**：`select! { _ = x => ..., _ = y => ... }`

   * 期待：`parent->x`, `parent->y` 両辺。Ready 観測は一方のみ。
3. **join**：`join!(x, y)`

   * 期待：両子へ辺、両方 `Ready` 後に親 `Ready`。
4. **spawn + await**：`let j = tokio::spawn(child); j.await`

   * 期待：child は root。`await` 側で `parent->join_future` の辺。
5. **panic / unwind**：子 `panic!`

   * 期待：exit-ret 無しでも再同期で poll-stack 一致へ。
6. **dyn Future**：`Box<dyn Future>` 経由

   * 期待：最終的に `GenFuture` で子捕捉。

---

## 12. 主要ロジック擬似コード

```rust
fn on_poll_entry(tid: Tid, regs: &Regs, rip: u64) -> Result<()> {
    let child_self = regs.arg0();                 // SysV: RDI
    let child = TaskId(child_self);

    // 1) 親探索（優先: フレームスキャン）
    let parent = scan_parent_genfuture(tid)
        .or_else(|| scope[tid].stack.last().copied());

    // 2) タスク登録・属性更新
    let discr = read_discriminant(child)?;
    let (file, line) = addr2line(rip);
    tasks.upsert(child, |t| {
        t.last_thread = tid; t.last_seen = now(); t.current_discriminant = Some(discr); t.last_rip = Some(rip);
    });

    // 3) エッジ登録（callsite 同定）
    if let Some(p) = parent {
        let suspend_idx = tasks[p].current_discriminant;
        let callsite_id = hash(p, suspend_idx, file, line);
        edges.upsert(EdgeId::new(p, child, callsite_id), |e| e.last_seen = now());
    } else {
        tasks[child].roots = true;
    }

    // 4) 動的スコープ push
    scope[tid].stack.push(child);

    // 5) exit ret アドレスに一過性BPを配置（初見関数のみ計算）
    ensure_ret_breakpoints(rip_function_range(rip));

    Ok(())
}

fn on_poll_exit(tid: Tid, regs: &Regs, rip: u64, retval: Option<EnumVal>) -> Result<()> {
    let child = scope[tid].stack.pop().unwrap_or_else(|| resync_from_os_stack(tid));

    if let Some(val) = retval {
        if is_poll_ready(val) {
            // parent がいれば当該 callsite を completed に
            if let Some(p) = scan_parent_genfuture(tid).or_else(|| scope[tid].stack.last().copied()) {
                if let Some(cs) = last_callsite_id(p, child) {
                    edges[EdgeId::new(p, child, cs)].completed = true;
                }
            }
            tasks[child].completed = true;
        }
    }
    Ok(())
}
```

---

## 13. CLI/TUI 拡張仕様（抜粋）

* `async tasks`：`TaskId  Type  State(discr)  LastSeen  Thread  Root/Leaf`
* `async bt`：**論理スタック**（子→親）。`bt` と並置表示切替。
* `async edges [--parent <TaskId>] [--completed]`：await グラフの行列表現。
* `async locals <TaskId>`：現在 variant に live なローカル（巨大バッファは長さのみ）。
* `async where <TaskId>`：`file:line` と `suspend_idx` を要約表示。

---

## 14. 拡張余地（オプション）

* **“中間 combinator 層” の表示切替**（非 GenFuture も含む `Future::poll` を透明化/表示）
* **wake/waker 追跡**（`RawWakerVTable` uprobe でキュー遷移を可視化）
* **`.asyncmap` セクション**（将来的に rustc ドライバから停止点↔フィールド完全マップを得る）

---

## 15. 成功基準（受け入れ）

* `select!/join!/spawn/Box<dyn Future>` を含む実アプリで、

  1. `async bt` が **親子チェーン**を正しく表示、
  2. `async edges` が **多分岐 await** を網羅、
  3. `async locals` が **停止点の live 変数**を復元、
  4. rr 上の `reverse-next` でも **構造が巻き戻し/再現** できる。


# kokia 追加実装計画書：`async fn` のローカル変数キャプチャ

目的：**ランタイム非依存**に、`async fn` の**現在停止点に live なローカル変数**を、通常関数と同等（`locals`）の操作で取得・表示できるようにする。
方式は **DWARF ロケーション評価**＋**generator（`GenFuture`）レイアウト解析**の二段構え（優先順：DWARF → generator）。

---

## 0. 成果物（P0 受け入れ条件）

* `async locals <TaskId>`：対象 `TaskId`（= generator `self` のアドレス）の**停止点ローカル**を名前・型・値で一覧化。
* `locals`（通常フレーム）でも DWARF に従って**レジスタ/スタック/メモリ**から復元。
* 最低限の型表示：整数/浮動/ポインタ/配列/スライス/`&str`/`String`/`Vec<T>`（プレビュー）/構造体/列挙（`Option`/`Result`）/参照/`Box`/`Pin`。
* 最適化（O0）で正確、O2 で欠損は `optimized-out` と注記。

---

## 1. コンポーネント構成（新規/拡張）

```
kokia-dwarf/
  ├─ loader.rs            # ELF/DWARF 読み込み（既存）
  ├─ loc_eval.rs          # ★DWARF ロケーション式評価（新規強化）
  ├─ type_tree.rs         # 型木（DIE）→ kokia内表現への解決（拡張）
  ├─ decode.rs            # ★値デコード（基本型/複合型/標準型プリティプリンタ）
  └─ cache.rs             # 型/ロケーション/行情報キャッシュ（拡張）

kokia-async/
  ├─ generator.rs         # ★generator 判別子/variant/フィールド取得
  ├─ locals.rs            # ★async locals 実装（DWARF優先→generator フォールバック）
  └─ names.rs             # ★フィールド名の人間可読化（suffix除去/合成）

kokia-target/
  └─ mem.rs               # process_vm_readv ラッパ、境界・長さ上限（拡張）

kokia-cli/
  └─ cmds/async_locals.rs # CLI/TUI コマンド/整形出力（新規）
```

主要依存：`gimli`, `object`, `bitflags`, `smallvec`, `fxhash`, `capstone`（既存）。

---

## 2. 取得戦略（優先度とフォールバック）

### 2.1 優先：DWARF ロケーション評価（「本来の変数名」を得る）

* 入力：**対象フレーム**（`TaskId` の poll 内での RIP）と **関数スコープの変数 DIE**。
* 手順：

  1. `gimli` で該当コンパイル単位→関数 DIE→子 DIE（`DW_TAG_variable`, `DW_TAG_formal_parameter`）を列挙。
  2. 現在の `RIP` に対する **`DW_AT_location` / location list** を評価（`loc_eval.rs`）。
  3. 評価結果は「レジスタ/FP相対/メモリ間接/ピース合成」。必要に応じて `process_vm_readv`。
  4. 型 DIE を `type_tree.rs` で kokia 型に解決 → `decode.rs` で値整形。
* 長所：**ソースの変数名**が復元でき、generator の内部名に依らない。
* 注意：最適化により location が `empty`（= optimized out）の場合がある。

### 2.2 次善：generator レイアウト（variant フィールド直読み）

* 入力：`TaskId=self_ptr`、generator 型の DIE、**active variant**（判別子）。
* 手順：

  1. `generator.rs`：`GenFuture::poll` の `self` 型 DIE を逆引きし **判別子オフセット/サイズ**を取得→現在 variant を決定。
  2. variant の **フィールド列 `{name, offset, size, ty}`** を取得。
  3. 各フィールドを `self_ptr + offset` で読み、`decode.rs` で整形。
  4. `names.rs` で `__await_3`, `<local>@5`, `.0` などの**実装依存 suffix を除去/正規化**。
* 長所：**停止点で live な全フィールド**を読みやすい。
* 注意：名前が実装名になる可能性 → 2.1 の結果と **名称マージ（同値アドレス一致で rename）**。

---

## 3. 値デコード（`decode.rs`）

### 3.1 共通方針

* **深さ制限**と**要素数上限**（既定：深さ 3、配列/Vec 表示 16 要素）で再帰を打ち切り、サマリ表示。
* **ゼロコピー禁止**：常に `process_vm_readv` で**別プロセスから安全に読み出し**。
* **サニタイズ**：巨大長さや不正ポインタは途中で切って `…` と表示（上限は設定化）。

### 3.2 プリミティブ

* 整数/浮動/Bool/Char：リトルエンディアンでパース。
* ポインタ：`0x…` 表示＋`addr2line` でシンボリック（関数ポインタ等）。

### 3.3 参照/スライス/文字列

* `&T`：中身を 1 段だけ展開（深さ制限考慮）。
* `&[T]`：`{ptr, len}` 読み、`T` を要素上限で配列表示。
* `&str`：`{ptr, len}` → UTF-8 バリデーション。失敗時は `b"...hex..."`。
* `String`：`{ptr, len, cap}` 読み、`len` まで文字列化。
* `OsString/PathBuf`：`Vec<u8>` として読み、表示は `b"...hex..."`（Linuxは UTF-8 想定でも安全重視）。

### 3.4 コレクション

* `Vec<T>`：`{ptr, len, cap}` → 要素上限で `[T; n]` 風プレビュー。
* `Box<T>`/`Pin<Box<T>>`：内部 `T` を 1 段展開。
* `Option<T>`/`Result<T,E>`：**DWARF の判別子**で active variant を判断し、その中身を表示。
* `Rc/Arc`：`ptr` と `strong/weak` カウンタ（実装依存フィールド）は可能なら推測、未対応時は `ptr` のみ。

### 3.5 ユーザ定義構造体/列挙

* 構造体：各フィールドを再帰。
* 列挙：判別子→ active variant→ フィールド再帰。
* 循環検出：訪問済み `addr+ty` をセットで記録し `…(cycle)`。

---

## 4. 実装詳細

### 4.1 ロケーション式評価（`loc_eval.rs`）

* 実装：`gimli::Evaluation` を利用し、**必要なレジスタ値/メモリ読取**を `kokia-target` にコールバック。
* 対応命令：`DW_OP_reg*`, `DW_OP_breg*`, `DW_OP_fbreg`, `DW_OP_piece`, `DW_OP_deref`, `DW_OP_plus_uconst` 等の基本群。
* 結果型：

  ```rust
  enum Loc {
      Reg { reg: u16 },
      Addr { addr: u64, size: usize },
      Pieces(Vec<Piece>), // 分割された複合
      Empty,              // optimized-out
  }
  ```

### 4.2 generator メタ（`generator.rs`）

* **判別子の位置**：generator の DIE（variantを伴う擬似 enum）を走査し、

  * `__state` 等の **discriminant フィールド**のオフセット/サイズを抽出。
* **active variant 特定**：`target.read(self_ptr + off, size)` で現在値を取得。
* **フィールド列挙**：active variant の `DW_TAG_member` を `Field {name, offset, size, ty}` へ展開。

### 4.3 名前マージ（DWARF 優先）

* 2.1 の**本来の変数名**と 2.2 の**generator フィールド**が**同一アドレス**を指すとき、

  * 表示名は **DWARF 名を優先**、generator 名は `aka <raw>` として補足。

### 4.4 メモリ読取ガード（`kokia-target::mem`）

* `read(addr, len)` は `MAX_LEN_PER_READ` を超えると分割。
* 総量上限 `MAX_TOTAL_BYTES_PER_CMD` を超えたら途中打ち切りで `…(truncated)`。
* 読めないアドレスは `invalid-address` と表示。

---

## 5. CLI/TUI 仕様

```
(kokia) async locals [<TaskId>|--current]
# 出力例
name           type                    value
-------------  ----------------------  ----------------------------
buf            Vec<u8> (len=64)        [0x12, 0x34, ... 16 more]
path           PathBuf                 b"/tmp/input.bin"
bytes_read     usize                   4096
peer           Option<SocketAddr>      Some(127.0.0.1:8080)
self           &mut MyState            MyState { phase: Handshake, ... }
__aka __await_3 <raw>                  &TcpStream(0x7f..)
```

オプション：

* `--max-depth N`, `--max-elems N`, `--hex`（バイナリは常に hex）
* `--raw`（generator フィールド名をそのまま出す）
* `locals`（通常フレーム）は同 UI で動作

---

## 6. テスト（自動）

### 6.1 ユニット

* `decode.rs`：あらゆる型のデコード関数に対し**合成メモリ**で golden テスト。
* `loc_eval.rs`：DWARF サンプル（固定 ELF）で命令網羅テスト。
* `generator.rs`：判別子/variant 抽出、フィールド列挙。

### 6.2 結合・例題

* `examples/async_locals`：

  * 停止点 1：`let s = String::from("abc"); let v = vec![1u32,2,3];`
  * 停止点 2（`await` 後）：`s` 再利用、`v` ドロップ済み → `optimized-out` を検出。
* `select!/join!/spawn` 複合で、各停止点の live 変数が**期待どおり**に変化。

### 6.3 逆実行（rr）

* `reverse-next` で停止点を戻し、**同じ `async locals` 出力**が再現されること。

---

## 7. パフォーマンス最適化

* **型/ロケーション/行情報キャッシュ**：`(cu_id, die_offset)` キーの LRU。
* **読み出し coalesce**：同一ページに跨る読みを `readv` で束ねる。
* **ヒューリスティクス**：`Vec<u8>`/`String` 等は**長さ上限**を小さく（既定 256B）。

---

## 8. 既知の難所と緩和

* **最適化で location 消失** → generator フォールバック＋`optimized-out`表示。
* **巨大/非整合メモリ** → 上限・タイムアウト・境界チェックで安全に打ち切り。
* **trait オブジェクト（`dyn Trait`）** → vtable まで表示（型名解決は将来拡張）。
* **レイアウト変更（rustc 更新）** → CI でツールチェーン固定。将来 `.asyncmap`（独自メタ）導入余地。

---

## 9. 実装スケジュール（P0：2–3 週）

* W1:

  * [ ] `loc_eval.rs` 実装（DWARF 基本命令）
  * [ ] `decode.rs` 基本型/参照/スライス/文字列
  * [ ] `generator.rs`（判別子/variant 抽出）
* W2:

  * [ ] `Vec/Box/Option/Result/Struct/Enum` デコード
  * [ ] `async locals` 統合（DWARF→generator フォールバック）
  * [ ] CLI 出力/上限/エラー整備
* W3:

  * [ ] 例題/結合テスト/rr 逆実行検証
  * [ ] キャッシュ/性能チューニング

---

## 10. 擬似コード断片

### 10.1 async locals（高位）

```rust
pub fn async_locals(task: TaskId, ctx: &mut Ctx) -> Vec<VarView> {
    let rip = ctx.async_state.tasks[&task].last_rip.unwrap();
    // 1) DWARF から名前付きローカルを復元
    let mut vars = dwarf_locals_at(rip, ctx).collect::<Vec<_>>();
    // 2) generator フォールバック（アドレス未出現の領域）
    let gen_vars = generator_fields(task, ctx);
    merge_by_address(&mut vars, gen_vars); // 同じaddrならDWARF名優先
    // 3) デコード＆表示用 View へ
    vars.into_iter().map(|b| decode_value(b, ctx)).collect()
}
```

### 10.2 DWARF ロケーション評価

```rust
fn eval_loc(die: &DieRef, rip: u64, regs: &Regs, mem: &dyn Mem) -> Loc {
    let mut eval = die.location_expr().evaluation();
    let mut res = eval.evaluate_with(|ctx| match ctx {
        gimli::EvaluationResult::RequiresRegister { register, .. } => {
            Ok(Value::Generic(regs.get(register)))
        }
        gimli::EvaluationResult::RequiresMemory { address, size, .. } => {
            Ok(Value::Bytes(mem.read(address, size)?))
        }
        // ... 他要求に対応
    })?;
    loc_from(res)
}
```

### 10.3 `&str`/`String` の読取

```rust
fn read_rust_str(mem: &dyn Mem, ptr: u64, len: usize) -> DisplayVal {
    let n = len.min(MAX_STR_BYTES);
    let bytes = mem.read(ptr, n).unwrap_or_default();
    match std::str::from_utf8(&bytes) {
        Ok(s) => DisplayVal::Str(s.to_owned(), len, n < len),
        Err(_) => DisplayVal::Bytes(bytes, len, true),
    }
}
```

---

### 11. ビルド推奨（デバッグ対象側）

* `RUSTFLAGS="-C debuginfo=2 -C force-frame-pointers=yes"`（P0 は `-C opt-level=0` を推奨）
* LTO 無効、`panic=unwind`、strip 無効

---

## 12. Task と Async Backtrace の概念的違い

**重要**: Task（タスク）と Async Backtrace（async バックトレース）は**異なる概念**であり、混同してはならない。

### 12.1 Task とは

* **定義**: `tokio::spawn` 等で生成された**独立した実行単位**
* **識別**: generator の `self` ポインタ（`TaskId = self_ptr`）で一意に識別
* **ライフサイクル**: spawn → poll の繰り返し → 完了/キャンセル
* **スコープ**: タスクは**独立して追跡**され、親タスクとの関係は `.await` を通じた間接的なもの

**例**:
```rust
let task1 = tokio::spawn(async { ... });
let task2 = tokio::spawn(async { ... });
task1.await;
task2.await;
```
→ `task1` と `task2` はそれぞれ独立した Task

### 12.2 Async Backtrace とは

* **定義**: **await チェーン**で構成される poll の連鎖的呼び出し
* **類比**: 通常の関数呼び出しにおける**コールスタック**に相当
* **構成**: 現在実行中の async 関数から、それを `.await` している親 async 関数へと遡る**論理的なスタックフレーム**
* **スコープ**: ある時点での**一連の Future::poll 呼び出しチェーン**

**例**:
```rust
async fn a() {
    b().await;
}
async fn b() {
    c().await;  // ← ここでブレークポイント
}
async fn c() { ... }
```
→ ブレークポイント時点での async backtrace: `c` → `b` → `a`

### 12.3 async locals の正しい実装

`async locals` コマンドは**現在のブレークポイント位置**でのローカル変数を表示する機能であり、**Task ID を引数に取るべきではない**。

#### 表示すべき変数の種類

1. **Generator state machine 内の変数**
   - `.await` を跨いで保持される必要がある変数
   - Generator（Future 実装構造体）のフィールドとして保存
   - discriminant（状態）に応じた active variant のフィールドから取得

2. **スタック上のローカル変数**
   - `.await` を跨がないため、通常の関数と同様にスタック/レジスタに配置
   - DWARF location expressions で取得
   - 現在の RIP、レジスタ状態、スタックフレームから評価

#### 実装方針

```rust
pub fn get_async_locals_at_current_frame() -> Result<Vec<Variable>> {
    let current_pc = get_current_pc();
    let current_frame = get_current_frame();
    let registers = get_current_registers();

    let mut variables = Vec::new();

    // 1) DWARF location evaluation: スタック/レジスタ上の変数
    //    - await を跨がない通常のローカル変数
    //    - DW_TAG_variable, DW_TAG_formal_parameter から取得
    let dwarf_vars = evaluate_dwarf_locals(current_pc, current_frame, registers)?;
    variables.extend(dwarf_vars);

    // 2) Generator state machine: Future 構造体内の変数
    //    - await を跨いで保持される変数
    //    - 現在の async 関数が generator の場合のみ
    if let Some(generator_self) = detect_generator_self(current_frame) {
        let discriminant = read_discriminant(generator_self)?;
        let generator_vars = extract_generator_fields(generator_self, discriminant)?;

        // アドレスベースでマージ（同じアドレスなら DWARF 名を優先）
        merge_variables(&mut variables, generator_vars);
    }

    Ok(variables)
}
```

#### CLI 仕様

**現在の仕様（誤り）**:
```
(kokia) async locals <TaskId>
```
→ Task ID を指定する必要があり、概念的に誤っている

**正しい仕様**:
```
(kokia) async locals
```
→ 引数なし。現在のブレークポイント位置（現在の async 関数フレーム）のローカル変数を表示

**通常の `locals` コマンドとの統合**:
```
(kokia) locals
```
→ 現在のフレームが async 関数の場合、自動的に generator 変数も含めて表示
→ 通常の関数の場合、DWARF location evaluation のみ

### 12.4 実装上の注意点

1. **Task tracking との分離**
   - Task tracking (`async tasks`, `async edges`) は Task の生成・完了・親子関係を追跡
   - Async locals は**現在の実行位置**でのローカル変数を表示
   - これらは独立した機能

2. **Generator self ポインタの検出**
   - 現在のフレームが `GenFuture::poll` または async 関数の場合
   - 第一引数（`self`）から generator のアドレスを取得
   - DWARF で関数名と型情報を確認

3. **変数のマージ**
   - DWARF 変数と generator フィールドが同じアドレスを指す場合
   - DWARF の変数名を優先（ソースコード上の名前）
   - Generator のフィールド名は `(aka __await_3)` のように補足情報として表示

4. **最適化への対応**
   - O2 等で最適化された場合、DWARF location が `optimized-out` の可能性
   - この場合でも generator フィールドから値を取得できる可能性がある
   - 両方で取得できない場合のみ `<optimized out>` と表示

---

## 13. 実装修正タスク

以下の修正を行う：

1. **コマンドインターフェース**
   - `async locals <TaskId>` → `async locals` に変更
   - 引数は不要（現在のフレームから自動検出）

2. **`get_async_locals` 関数の書き直し**
   - 引数: `task_id: u64` → 引数なし
   - 現在の PC、フレーム、レジスタから変数を取得
   - Generator self は現在のフレームから検出

3. **`locals` コマンドとの統合**
   - 現在のフレームが async 関数かを判定
   - async 関数の場合、generator 変数も自動的に含める
   - 通常の関数と同じ UI で表示

4. **DWARF location evaluation の優先**
   - スタック/レジスタ上の変数を DWARF から取得
   - Generator フィールドはフォールバック/補完として使用

---

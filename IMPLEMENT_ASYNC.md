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


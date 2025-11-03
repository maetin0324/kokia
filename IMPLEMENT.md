# 0. 目標 / 非目標

## 0.1 目標（P0〜P2）

* P0（PoC → α）

  * 単一/多スレッドの **実行/停止/ステップ/ブレークポイント**（SW/HW）
  * **バックトレース**（FP と DWARF CFI 併用）、**ソース行/シンボル化**
  * **ローカル変数表示**（DWARF LocExpr 評価）
  * **GenFuture::poll エントリ/リターン監視**で **論理スタック（await チェーン）** の可視化
* P1（β）

  * **タイムトラベル**: rr リプレイに対する **reverse-continue / reverse-step / reverse-next / reverse-finish**
  * 条件付き BP / ログポイント / ウォッチポイント（DR0–DR7）
  * TUI（対話 REPL + 画面分割: スタック/ローカル/ソース/async-tasks）
* P2（1.0 候補）

  * **自前レコーダ（軽量）**: チェックポイント＋決定的再実行で逆方向実行（小/中規模向け）
  * eBPF/uprobes を用いた **低侵襲観測バックエンド**（本番観測）※停止は不可、観測のみ
  * AArch64/Linux 対応、Musl/コンテナでの動作確認

## 0.2 非目標（当面）

* Windows / macOS カーネル API 対応
* JIT 言語（JVM/JS）特有のデバッグ統合

---

# 1. リポジトリ構成（ワークスペース）

```
kokia/
├─ Cargo.toml                      # workspace
├─ kokia-cli/                      # CLI/TUI フロント
├─ kokia-core/                     # デバッガ中核（状態機械・コマンド・セマンティクス）
├─ kokia-target/                   # OS/CPU 依存: ptrace, /proc, hw breakpoints
├─ kokia-dwarf/                    # gimli による DWARF/ELF/CFI/LocExpr 評価
├─ kokia-unwind/                   # スタックアンワインダ（FP優先→DWARF FDE/CIE）
├─ kokia-async/                    # GenFuture 解析・論理スタック再構成
├─ kokia-tt-rr/                    # rr（gdb-remote）バックエンド
├─ kokia-tt-lite/                  # 軽量タイムトラベル（チェックポイント＋再実行）
├─ kokia-ebpf/                     # 低侵襲観測（将来）
└─ examples/                       # 動作検証サンプル（sync/async/混在）
```

主要依存：

* `nix`（ptrace/regs/signal）、`gimli`/`object`/`addr2line`、`capstone`（逆アセンブル任意）、`rustyline` か `reedline`（REPL）、`tui`/`ratatui`（TUI）、`serde`（スクリプト/設定）

---

# 2. システムアーキテクチャ

## 2.1 抽象ターゲット層（`kokia-target`）

* **Process**: attach/detach, fork/exec 追跡, `PTRACE_SEIZE`, `PTRACE_INTERRUPT`
* **Thread**: 列挙、レジスタ取得・設定、single-step、シグナル配送
* **Memory**: read/write、page perms、/proc/<pid>/maps 解析
* **Breakpoints**

  * **SW**: INT3（`0xCC`）の挿入・原命令管理・再実行補正（EIP/RIP 調整）
  * **HW**: DR0–DR7 設定 API（watchpoint: r/w/exec, len）
* **Events**: exec, fork, clone, exit, signal, syscall-stop（必要に応じ）
* **Perf**（任意）: サンプリング

## 2.2 デバッグ情報層（`kokia-dwarf`）

* ELF 解析・.debug_* セクション読取（`object`）
* 行番号/シンボル化（`addr2line` 相当を `gimli` で）
* CFI（.eh_frame, .debug_frame）→ **アンワインド規則**へ
* **変数ロケーション**（DWARF Expr/Eval）: レジスタ/FP 相対/メモリ間接/ピース合成

## 2.3 アンワインダ（`kokia-unwind`）

* 戦略順：**FPベース（force-frame-pointers 推奨）→ CFI（FDE/CIE）→ フォールバック**
* `RIP, RSP, CFA` を逐次計算、内蔵キャッシュ
* **非同期安全**: ストップ時の全スレッド BT を取得

## 2.4 Async 知能（`kokia-async`）

**目的**: ランタイム非依存で async を「通常関数のように」見せる。

* **GenFuture::poll** への **正規表現 BP** をセット（v0 マングリング対応）

  * `core::future::from_generator::GenFuture<...>::poll`
* **エントリ時**:

  * 第1引数 `self`（*generator object*）のポインタを取得 → `TaskId = self as usize`
  * カレントスレッドのスタックを遡って **最も近い上位の GenFuture フレーム**を探索 → `parent: Option<TaskId>`
  * `TaskGraph[TaskId] = { parent, thread, last_poll_ts, ip }` 更新
  * **discriminant**（生成器の状態）を DWARF 型情報から **オフセット解決 → 読取**
* **リターン時**:

  * `Poll::Pending/Ready` をレジスタ/戻り値から判定（ABI 依存; Rust ABI の `enum` レイアウトを DWARF で確認）
* **ローカル変数復元**:

  * `TaskId` に結びつく generator の **active variant** に含まれる **フィールド（= live ローカル）**を列挙
  * 変数名は rustc の生成器フィールド名（`<local>@<suspend#>` など）が DWARF に残るため、そのまま or 整形して表示
* **論理スタック表示**:

  * `TaskId` から `parent` を辿って**await チェーン**を可視化（通常 BT と併置表示）

> 将来（P2）: `kokia-coop`（任意の opt-in ライブラリ）で「休眠タスク列挙」も可能にするが、P0〜P1 は **純粋に GenFuture 監視のみ**。

## 2.5 タイムトラベル（`kokia-tt-rr` / `kokia-tt-lite`）

* **rr バックエンド（P1）**

  * `rr replay -s <port>` が提供する **GDB Remote Serial Protocol (RSP)** に **クライアント実装**で接続
  * RSP の `bc`（reverse-continue）, `bs`（reverse-stepi）, `br`（reverse-finish）等を実装
  * メモリ/レジスタ取得は RSP 経由 → `kokia-core` の共通表示ロジックを再利用
* **軽量レコーダ（P2）**

  * **チェックポイント**: `/proc/<pid>/maps` を解析し、匿名/私有ページを **write-xor-execute 非依存で差分スナップショット**（COW 風; `userfaultfd` 検討）
  * **イベントログ**: シグナル/スレッド生成/`mmap`/`munmap`/`mprotect`/`futex`/`rdtsc` 禁止等を追跡
  * **逆方向実行**: 最近のチェックポイントへロールバック→前進シミュレーションで指定地点まで復元
    ※ 重いが**小規模検証**には有効（rr 非依存）

---

# 3. kokia-core（コマンド/状態機械）

## 3.1 内部状態

```rust
struct DebugSession {
    target: Box<dyn TargetBackend>,        // ptrace / rr
    processes: HashMap<Pid, ProcessState>,
    breakpoints: BpTable,
    hw_watchpoints: HwWpTable,
    async_state: AsyncState,               // TaskGraph 等
    dwarf_ctx: DwarfContext,
    unwind_ctx: UnwindContext,
    settings: Settings,
}

struct AsyncState {
    tasks: HashMap<TaskId, TaskInfo>,
    // スレッドごとの poll ネスト（動的スコープ）
    poll_stack: HashMap<Tid, Vec<TaskId>>,
}
```

## 3.2 コマンド（GDB 互換＋拡張）

* 既存互換: `run`, `attach <pid>`, `break [file:line|func|addr]`, `delete`, `continue`, `step/next/finish`,
  `bt`, `frame`, `info threads`, `info registers`, `x/<fmt> <addr>`, `set var`, `watch/rwatch/awatch` …
* 拡張（kokia）:

  * `async bt`（現在地点の**論理スタック**を表示）
  * `async locals [task-id]`（指定 Task の**停止点ローカル**表示）
  * `async tasks`（観測済み Task 一覧＋親子）
  * `tt record|replay|reverse-...`（タイムトラベル）
  * `logpoint <loc> <printf-like>`（停止せず記録）
  * `script run <file>.kok`（JSON/DSL で一括操作）
* すべて JSON-RPC でも発火可能（外部 UI 連携用）

---

# 4. 主要アルゴリズム & 実装指針

## 4.1 SW ブレークポイント

* 既設 INT3 の再入防止（多重挿入）管理
* 命令長（x86-64 可変長）の**原命令キャッシュ** → 命令先頭にのみ設定
* 命中時: RIP を 1 戻し → 原命令を一時復元 → **単ステップ**で再実行 → 再度 INT3 復帰

## 4.2 バックトレース

* `force-frame-pointers=yes` を推奨（P0 の信頼性向上）
* CFI では `gimli::UnwindSection` を解釈、CIE/FDE をキャッシュ
* PLT/GOT/`__libc_start_main` 境界など既知停止点で “ユーザーフレームのみ” を抽出

## 4.3 ローカル変数

* `gimli::Evaluation` で DWARF 式を評価（レジスタ/メモリ読みを `kokia-target` にコールバック）
* `DW_OP_piece` 複合や SSE レジスタ断片も可能な範囲で復元
* 最適化で消失時は “optimized-out” 表示

## 4.4 GenFuture 解析（重要）

* シンボル解決: v0 マングリング対応の関数名正規表現を準備
* **discriminant 位置の特定**:

  1. generator 型（enum のような可変レイアウト）を DWARF 型木から探索
  2. active variant 判別子の格納オフセットを得る
  3. `process_vm_read` で該当バイトを読取 → 状態決定
* **live ローカル復元**:

  * active variant に含まれるフィールド群を列挙
  * “`__await_N`/`<local>@N`” 等の名前を人間可読に正規化
* **論理スタック**:

  * poll エントリ時にスレッドの **poll_stack** に push、リターン時に pop
  * “親”は push 直前のトップ、もしくは実フレーム走査で最近の GenFuture を採用

---

# 5. ビルド/実行前提 & 推奨フラグ

ターゲットプログラム側（デバッグ時の推奨）:

* `RUSTFLAGS="-C debuginfo=2 -C force-frame-pointers=yes"`（P0は `-C opt-level=0`）
* LTO/ThinLTO/O0 前提。`panic=unwind`。strip 無効。
* （可能なら）`-Z symbol-mangling-version=v0` の利用（チームの toolchain 固定）

---

# 6. テスト計画

## 6.1 単体テスト

* `kokia-dwarf`: DWARF サンプル（elfdump 固定）に対する行番/CFI/LocExpr の golden テスト
* `kokia-unwind`: 合成フレーム列（模擬レジスタ）での復元テスト
* `kokia-async`: コンパイル済み `async` サンプルの **discriminant オフセット検知**テスト

## 6.2 結合テスト

* `examples/`

  * `sync_basic`: 関数ネスト/最適化 off
  * `async_basic`: `async fn`/`await`/`select!`/`tokio::spawn`
  * `async_mixed`: sync/async 複合、パニック、 unwinding
* スクリプトで **kokia → attach → break → continue → hit** までを自動検証

## 6.3 タイムトラベル

* `rr` で記録 → `kokia tt replay` で **reverse-step/continue** の整合性を検証
* 例: “Ready → Pending” を跨ぐ箇所に戻れること

---

# 7. CLI/TUI 仕様（抜粋）

```
$ kokia run ./target/debug/app --arg1
$ kokia attach <pid>
(kokia) break src/main.rs:42
(kokia) continue
(kokia) bt
(kokia) locals
(kokia) async bt
(kokia) async tasks
(kokia) async locals 0x7f3abc123000
(kokia) watch *0x7fff... len 8 write
(kokia) tt record ./target/debug/app
(kokia) tt replay rr-trace/
(kokia) reverse-next
```

TUI: 左ペイン「ソース」、右上「スタック（ユーザ/論理 切替）」、右下「ローカル/async-locals」、下部「コマンドライン」。

---

# 8. セキュリティ/権限・運用

* `ptrace_scope=0` が必要な環境では sudo 不要。制限下では root 推奨。
* rr 利用時はパフォーマンス低下と互換 CPU 前提に留意。
* eBPF/uprobes は CAP_BPF 等が必要（P2 以降）。

---

# 9. リスクと緩和

* **最適化でローカル不可視** → P0 は O0 前提。重要関数に `#[inline(never)]` 推奨。
* **生成器レイアウトの rustc 依存** → CI で特定ツールチェーン固定。将来は `.asyncmap`（rustc ドライバ拡張）検討。
* **タイムトラベルの実装負債** → 初期は rr 連携に限定。P2 でスコープを小さくした軽量レコーダを実装。
* **マルチスレッド停止順の競合** → `PTRACE_SEIZE + INTERRUPT` で全スレッド同期停止を標準化。

---

# 10. マイルストーンと所要タスク

## P0（6–8 週目安）

* [ ] `kokia-target`: attach/continue/step/regs/mem/bp（SW）
* [ ] `kokia-dwarf`: ELF/DWARF ローダ、行番号/シンボル化
* [ ] `kokia-unwind`: FP→CFI の BT
* [ ] `kokia-async`: GenFuture::poll の検出・TaskId 追跡・論理 BT（最小）
* [ ] `kokia-cli`: REPL と基本コマンド、テキスト表
* [ ] examples と自動テスト

**受け入れ基準**:
`async fn` を含むプログラムで、`break`, `continue`, `bt`, `locals`, `async bt`, `async locals` が機能。

## P1（+6–8 週）

* [ ] `kokia-tt-rr`: RSP クライアント（regs/mem/bp/reverse-*）
* [ ] `kokia-target`: HW watchpoint
* [ ] 条件付き BP / ログポイント
* [ ] TUI 実装（ratatui）

**受け入れ基準**:
rr リプレイ上で `reverse-next` などが実行でき、論理 BT/ローカルが逆方向にも再現。

## P2（+8–12 週）

* [ ] `kokia-tt-lite`: チェックポイント＋再実行（小規模向け）
* [ ] `kokia-ebpf`: poll エントリ/exit の uprobe サンプリング（観測専用）
* [ ] AArch64 対応（最低限）

---

# 11. 実装のキーピース（擬似コード）

### 11.1 GenFuture::poll での Task 生成

```rust
fn on_breakpoint(ctx: &mut DebugSession, tid: Tid, rip: u64) -> Result<()> {
    if ctx.async_state.is_genfuture_poll(rip)? {
        // SysV ABI: first arg in RDI (x86-64)
        let regs = ctx.target.get_regs(tid)?;
        let self_ptr = regs.rdi;
        let parent = ctx.async_state.current_task_of(tid);
        let discr = read_discriminant(&ctx.dwarf_ctx, &ctx.target, self_ptr)?;
        ctx.async_state.on_poll_enter(tid, self_ptr, parent, discr, rip);
    }
    Ok(())
}

fn read_discriminant(dwarf: &DwarfContext, tgt: &dyn TargetBackend, self_ptr: u64) -> Result<u64> {
    let ty = dwarf.resolve_generator_type(self_ptr)?;      // 型特定（周辺から）
    let off = ty.discriminant_offset.ok_or(Error::NoDisc)?;
    let sz  = ty.discriminant_size;
    Ok(tgt.read_uint(self_ptr + off, sz)?)
}
```

### 11.2 生成器ローカルの復元

```rust
fn async_locals(task: &TaskInfo, dwarf: &DwarfContext, tgt: &dyn TargetBackend) -> Vec<Var> {
    let gen = dwarf.generator_layout(task.type_id);
    let variant = gen.variant(task.discriminant)?;
    variant.fields.iter().filter_map(|f| {
        let addr = task.self_ptr + f.offset;
        let val  = tgt.read_bytes(addr, f.size).ok()?;
        Some(Var { name: f.user_friendly_name(), ty: f.ty, value: decode(val, f.ty) })
    }).collect()
}
```

### 11.3 逆方向実行（rr）

```rust
match cmd {
  Cmd::ReverseContinue => rr.send(b"bc")?,
  Cmd::ReverseStep => rr.send(b"bs")?,
  Cmd::ReverseFinish => rr.send(b"br")?,
  _ => ...
}
```

---

# 12. パフォーマンス最適化

* シンボル/CFI/行番号は **アドレス帯域ごとの LRU キャッシュ**
* ブレーク頻度が高い `GenFuture::poll` は **サンプリング率**（例: 1/N）を設定可能
* 大型メモリ読みは **paged read**（`process_vm_readv`）で回数削減

---

# 13. 開発運用・品質

* **ツールチェーン固定**（Rust nightly ではなく stable の最新版 + `-Z` を使わない範囲から開始）
* CI: Linux（Ubuntu LTS）で examples をビルド → kokia の自動スクリプトで統合テスト
* 例外発生時の **ミニダンプ**（内部状態）を収集し自己診断
* ログ: `RUST_LOG=kokia=info,kokia_async=debug` など

---

# 14. 将来拡張（研究枠）

* `.asyncmap`：rustc_driver で MIR 停止点→フィールド対応表を **カスタムセクション**に埋め込み、最適化下でも 100% 再現
* `RawWakerVTable` への uprobe で **wake/wake_by_ref** 観測 → スケジューラ可視化
* **協調モード**（任意の軽量レジストリ）で **休眠タスク列挙**を可能に

---

# 15. まとめ（実装の勘どころ）

* **まず P0**：ptrace + DWARF + GenFuture 監視だけで「**break/backtrace/locals** と **async 論理 BT/locals**」を成立させる。
* **P1**：**rr** をバックエンドに **reverse-* コマンド**を完成。UI/TUI を整える。
* **P2**：軽量レコーダで rr 非依存のタイムトラベル、eBPF 観測で本番適用への道を開く。

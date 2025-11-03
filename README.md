# Kokia - Rust Async Debugger

ランタイム非依存でRustの非同期関数をデバッグできるデバッガ。

## プロジェクト構成

- **kokia-core**: デバッガコアロジック
- **kokia-async**: async機能（GenFuture監視、論理スタック構築）
- **kokia-target**: ptrace/プロセス制御
- **kokia-dwarf**: DWARFデバッグ情報解析
- **kokia-cli**: CLIインターフェース

## ビルド

```bash
cargo build --release
```

## 使用方法

### 方法1: プロセスを起動してデバッグ (推奨)

```bash
# 1. デバッグ対象プログラムをビルド
RUSTFLAGS="-C debuginfo=2 -C force-frame-pointers=yes" cargo build --package simple_async

# 2. Kokiaで起動してデバッグ
cargo run --package kokia-cli -- run ./target/debug/simple_async

# または引数を渡す場合
cargo run --package kokia-cli -- run ./target/debug/simple_async arg1 arg2
```

### 方法2: 既存のプロセスにアタッチ

```bash
# 1. デバッグ対象プログラムを起動
./target/debug/simple_async &
echo $!  # PIDを確認

# 2. Kokiaでアタッチ
cargo run --package kokia-cli -- attach --pid <PID> ./target/debug/simple_async
```

### デバッグコマンド

```
(kokia) help              # ヘルプを表示
(kokia) find double       # "double"を含むシンボルを検索
(kokia) async list        # async関連のシンボルをすべて表示
(kokia) break main        # mainにブレークポイントを設定
(kokia) continue          # 実行継続（プロセスを起動した場合は実行開始）
(kokia) quit              # 終了
```

## 実装済み機能

### P0機能（基本実装）
- ✅ プロセスへのアタッチ/デタッチ
- ✅ **プロセス起動モード（spawn）**
- ✅ ELF/DWARF情報の読み込み
- ✅ シンボル名の解決
- ✅ ソフトウェアブレークポイント（INT3）
- ✅ メモリ読み書き
- ✅ レジスタアクセス
- ✅ GenFuture::poll検出（パターンマッチング）
- ✅ async関数のクロージャ検出
- ✅ 論理スタック構造（基本）
- ✅ タスクトラッカー
- ✅ **clap deriveベースのCLIインターフェース**

### 未実装機能
- [ ] 実際のブレークポイントヒット時の処理
- [ ] シグナル処理
- [ ] ステップ実行
- [ ] バックトレース表示
- [ ] ローカル変数表示（DWARF LocExpr評価）
- [ ] 論理スタック（awaitチェーン）の実際の構築
- [ ] discriminant読み取り
- [ ] タイムトラベル（rr連携）

## 技術的な詳細

### GenFuture検出

async関数はコンパイル時に以下のように変換されます：

- `async fn double(x: i32) -> i32` → `double::{{closure}}`
- Future::poll実装: `<BlockingTask<T> as Future>::poll`

Kokiaは正規表現を使用してこれらのシンボルを検出します：

- `Future.*poll` - Future::pollの実装
- `closure.*\$u7d\$` - async関数のクロージャ（マングリング名）
- `::\{\{closure\}\}` - async関数のクロージャ（デマングル名）

### アーキテクチャ

```
kokia-cli (REPL)
    ↓
kokia-core (Debugger)
    ↓
    ├─ kokia-target (ptrace, Memory, Registers)
    ├─ kokia-dwarf (ELF/DWARF解析)
    └─ kokia-async (GenFuture検出, 論理スタック)
```

## テスト

```bash
# 全体のテスト
cargo test

# 個別のクレートをテスト
cargo test --package kokia-dwarf -- --nocapture
cargo test --package kokia-async -- --nocapture
```

## ライセンス

MIT OR Apache-2.0

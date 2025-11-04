//! デバッガのメインロジック

use crate::{breakpoint::BreakpointManager, Breakpoint, BreakpointId, Result};
use kokia_async::AsyncTracker;
use kokia_dwarf::{DwarfLoader, LineInfoProvider, Symbol, SymbolResolver};
use kokia_target::{Memory, Process, Registers, StopReason};
use std::path::Path;

/// スタックフレーム情報
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// フレーム番号（0が最新）
    pub frame_number: usize,
    /// プログラムカウンタ（リターンアドレスまたは現在のPC）
    pub pc: u64,
    /// ベースポインタ
    pub rbp: u64,
    /// 関数名（シンボルから解決）
    pub function_name: Option<String>,
    /// ソースファイル名
    pub file: Option<String>,
    /// 行番号
    pub line: Option<u32>,
    /// 保存された RDI レジスタ値（async 関数の self ポインタ候補）
    pub saved_rdi: Option<u64>,
}

/// デバッガ
pub struct Debugger {
    /// デバッグ対象プロセス
    process: Option<Process>,
    /// プロセスID
    pid: Option<i32>,
    /// メモリアクセス
    memory: Option<Memory>,
    /// レジスタアクセス
    registers: Option<Registers>,
    /// DWARF情報ローダー
    dwarf_loader: Option<DwarfLoader>,
    /// シンボル解決器
    symbol_resolver: Option<SymbolResolver>,
    /// 行番号情報プロバイダー（DwarfLoaderへの参照が必要）
    // LineInfoProviderはライフタイム付きなので、毎回DwarfLoaderから作成
    /// Asyncタスクトラッカー
    async_tracker: AsyncTracker,
    /// ブレークポイント管理
    breakpoint_manager: BreakpointManager,
}

impl Debugger {
    /// 新しいデバッガを作成する
    pub fn new() -> Self {
        Self {
            process: None,
            pid: None,
            memory: None,
            registers: None,
            dwarf_loader: None,
            symbol_resolver: None,
            async_tracker: AsyncTracker::new()
                .expect("Failed to create AsyncTracker"),
            breakpoint_manager: BreakpointManager::new(),
        }
    }

    /// プロセスにアタッチされているか確認し、Registersへの参照を取得
    fn require_registers(&self) -> Result<&Registers> {
        self.registers
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))
    }

    /// プロセスにアタッチされているか確認し、Memoryへの参照を取得
    fn require_memory(&self) -> Result<&Memory> {
        self.memory
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))
    }

    /// 実行時アドレスをファイルオフセットに変換する（PIE対応）
    ///
    /// PIEの場合、実行時アドレスからベースアドレスを引いてオフセットに変換します。
    /// 非PIEの場合、アドレスをそのまま返します。
    fn runtime_addr_to_offset(&self, addr: u64) -> Result<u64> {
        let resolver = self.symbol_resolver.as_ref()
            .ok_or_else(|| anyhow::anyhow!("DWARF information not loaded"))?;

        if resolver.is_pie() {
            let memory = self.require_memory()?;
            let base = memory.get_base_address()? as u64;
            Ok(addr.saturating_sub(base))
        } else {
            Ok(addr)
        }
    }

    /// ファイルオフセットを実行時アドレスに変換する（PIE対応）
    ///
    /// PIEの場合、オフセットにベースアドレスを加算して実行時アドレスに変換します。
    /// 非PIEの場合、オフセットをそのまま返します。
    fn offset_to_runtime_addr(&self, offset: u64) -> Result<u64> {
        let resolver = self.symbol_resolver.as_ref()
            .ok_or_else(|| anyhow::anyhow!("DWARF information not loaded"))?;

        if resolver.is_pie() {
            let memory = self.require_memory()?;
            let base = memory.get_base_address()? as u64;
            Ok(base + offset)
        } else {
            Ok(offset)
        }
    }

    /// シンボル名からシンボルを検索して、最適な候補を選択する
    ///
    /// マングル名とデマングル名の両方でマッチングを試み、以下の優先順位で選択します：
    /// 1. 完全一致するシンボル（マングル名またはデマングル名）
    /// 2. 候補が1つだけの場合はそれを使用
    /// 3. デマングル名に部分一致するシンボル
    /// 4. 最初のシンボル
    ///
    /// 複数の候補があり選択できない場合はエラーを返します。
    fn find_best_symbol(&self, symbol_name: &str) -> Result<Symbol> {
        let symbols = self.find_symbols(symbol_name);

        // まず完全一致を試す
        let symbol = symbols
            .iter()
            .find(|s| s.name == symbol_name || s.demangled_name == symbol_name)
            .or_else(|| {
                // 完全一致が見つからない場合、部分一致の結果を使用
                if symbols.len() == 1 {
                    // 候補が1つだけの場合はそれを使用
                    symbols.first()
                } else if symbols.is_empty() {
                    None
                } else {
                    // 複数の候補がある場合は、最も正確にマッチするものを選択
                    // デマングル名に含まれるものを優先
                    symbols
                        .iter()
                        .find(|s| s.demangled_name.contains(symbol_name))
                        .or_else(|| symbols.first())
                }
            });

        match symbol {
            Some(s) => Ok(s.clone()),
            None => {
                if symbols.is_empty() {
                    Err(anyhow::anyhow!("Symbol not found: {}", symbol_name))
                } else {
                    // 複数の候補がある場合はリストを表示
                    let mut msg = format!(
                        "Multiple symbols match '{}'. Please be more specific:\n",
                        symbol_name
                    );
                    for (i, sym) in symbols.iter().enumerate() {
                        msg.push_str(&format!("  {}. {}\n", i + 1, sym.demangled_name));
                    }
                    Err(anyhow::anyhow!(msg))
                }
            }
        }
    }

    /// 実行可能ファイルを起動してデバッグを開始する
    ///
    /// 新しいプロセスを起動してデバッグ対象とします。
    /// プロセスは最初の命令で停止状態で開始されます。
    /// メモリマッピングが完全に初期化されているため、ブレークポイントを安全に設定できます。
    pub fn spawn<P: AsRef<Path>>(&mut self, program: P, args: &[String]) -> Result<()> {
        let process = Process::spawn(program, args)?;
        let pid = process.pid();
        self.pid = Some(pid);
        self.memory = Some(Memory::new(pid));
        self.registers = Some(Registers::new(pid));
        self.process = Some(process);
        Ok(())
    }

    /// 既存のプロセスにアタッチする
    pub fn attach(&mut self, pid: i32) -> Result<()> {
        let process = Process::attach(pid)?;
        self.pid = Some(pid);
        self.memory = Some(Memory::new(pid));
        self.registers = Some(Registers::new(pid));
        self.process = Some(process);
        Ok(())
    }

    /// ELFバイナリからDWARF情報を読み込む
    pub fn load_binary<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let loader = DwarfLoader::load(path)?;
        let resolver = SymbolResolver::new(&loader)?;
        self.dwarf_loader = Some(loader);
        self.symbol_resolver = Some(resolver);
        Ok(())
    }

    /// シンボル名からアドレスを解決する
    pub fn resolve_symbol(&self, name: &str) -> Option<u64> {
        self.symbol_resolver.as_ref()?.resolve(name)
    }

    /// アドレスからシンボルを解決する
    pub fn reverse_resolve(&self, addr: u64) -> Option<Symbol> {
        let resolver = self.symbol_resolver.as_ref()?;
        let lookup_addr = self.runtime_addr_to_offset(addr).ok()?;
        resolver.reverse_resolve(lookup_addr)
    }

    /// アドレスから行番号情報を取得する
    pub fn get_line_info(&self, addr: u64) -> Option<(String, u32)> {
        let loader = self.dwarf_loader.as_ref()?;
        let lookup_addr = self.runtime_addr_to_offset(addr).ok()?;
        let line_provider = LineInfoProvider::new(loader);
        let line_info = line_provider.lookup(lookup_addr).ok()??;
        Some((line_info.file?, line_info.line? as u32))
    }

    /// パターンにマッチするシンボルを検索する
    pub fn find_symbols(&self, pattern: &str) -> Vec<Symbol> {
        self.symbol_resolver
            .as_ref()
            .map(|r| r.find_symbols(pattern))
            .unwrap_or_default()
    }

    /// async関連のシンボルをすべて検索する
    pub fn find_async_symbols(&self) -> Vec<Symbol> {
        let resolver = match &self.symbol_resolver {
            Some(r) => r,
            None => return Vec::new(),
        };

        resolver
            .all_symbols()
            .filter(|sym| self.async_tracker.detector().is_async_related(&sym.name))
            .cloned()
            .collect()
    }

    /// async関数のclosureかどうか判定する（ランタイム非依存）
    ///
    /// 検出条件：
    /// - シンボル名が `::{{closure}}` を含む
    /// - ランタイム内部（tokio::, async_std::, futures::）は除外
    /// - 標準ライブラリ（std::, core::, alloc::）は除外
    /// - 依存ライブラリ（parking_lot, hashbrown等）は除外
    fn is_user_async_closure(name: &str) -> bool {
        // closure シンボルかチェック
        if !name.contains("{{closure}}") {
            return false;
        }

        // {{constant}}などの特殊closureを除外
        if name.contains("{{constant}}") {
            return false;
        }

        // ランタイム内部を除外
        if name.starts_with("tokio::")
            || name.starts_with("async_std::")
            || name.starts_with("futures::")
            || name.starts_with("mio::")
            || name.contains("::runtime::")
            || name.contains("::executor::")
            || name.contains("::task::")
        {
            return false;
        }

        // 標準ライブラリを除外
        if name.starts_with("std::") || name.starts_with("core::") || name.starts_with("alloc::")
        {
            return false;
        }

        // よく使われる依存ライブラリを除外
        if name.starts_with("parking_lot")
            || name.starts_with("hashbrown::")
            || name.starts_with("tracing::")
            || name.starts_with("serde::")
            || name.starts_with("log::")
            || name.starts_with("bytes::")
            || name.starts_with("hyper::")
            || name.starts_with("h2::")
        {
            return false;
        }

        // drop_in_place などのシステム関数を除外
        if name.contains("drop_in_place")
            || name.contains("::fmt::")
            || name.contains("::clone::")
            || name.contains("::drop::")
        {
            return false;
        }

        true
    }

    /// async関数のclosureを検出する（ランタイム非依存）
    ///
    /// 最新のRustでは`GenFuture::poll`が存在しないため、async関数のclosureを
    /// 直接検出してブレークポイントを設定します。
    pub fn find_async_function_closures(&self) -> Vec<Symbol> {
        let resolver = match &self.symbol_resolver {
            Some(r) => r,
            None => return Vec::new(),
        };

        resolver
            .all_symbols()
            .filter(|sym| Self::is_user_async_closure(&sym.demangled_name))
            .cloned()
            .collect()
    }

    /// async関数のFuture::poll 実装を検索する（ランタイム非依存）
    ///
    /// 検出戦略：
    /// 1. GenFuture::poll (古いRustバージョン)
    /// 2. async関数のclosure (最新のRust)
    ///
    /// 注意：最新のRustではFuture::pollがインライン化されているため、
    /// async関数のclosureエントリーポイントを代わりに使用します。
    pub fn find_genfuture_poll_symbols(&self) -> Vec<Symbol> {
        let resolver = match &self.symbol_resolver {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut symbols = Vec::new();

        // 1. GenFuture::poll を検出（古いRustコンパイラ用）
        for sym in resolver.all_symbols() {
            if (sym.name.contains("GenFuture") && sym.name.contains("poll"))
                || (sym.name.contains("core..future..from_generator") && sym.name.contains("poll")) {
                symbols.push(sym.clone());
            }
        }

        // GenFuture::pollが見つかった場合はそれを返す
        if !symbols.is_empty() {
            return symbols;
        }

        // 2. 最新のRust: async関数のclosureを検出
        self.find_async_function_closures()
    }

    /// async関数に自動的にブレークポイントを設定する
    ///
    /// async タスクトラッキングのために、async関数のエントリポイントに
    /// ブレークポイントを設定します。
    ///
    /// 検出戦略：
    /// - 古いRust: GenFuture::poll を検出
    /// - 最新のRust: async関数のclosureを検出（Future::pollはインライン化されるため）
    ///
    /// 注意：最新のRustでは完全なpoll entry/exitの監視ではなく、
    /// async関数が最初に呼ばれたときにトラッキングします。
    pub fn set_genfuture_poll_breakpoints(&mut self) -> Result<Vec<BreakpointId>> {
        let symbols = self.find_genfuture_poll_symbols();

        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        let mut breakpoint_ids = Vec::new();

        for symbol in symbols {
            match self.set_async_breakpoint_by_symbol(&symbol.name) {
                Ok(bp_id) => {
                    breakpoint_ids.push(bp_id);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to set breakpoint on {}: {}", symbol.name, e);
                }
            }
        }

        Ok(breakpoint_ids)
    }

    /// Async tracking用のブレークポイントをシンボル名で設定する
    ///
    /// entry（関数先頭）とexit（各ret命令）の両方にブレークポイントを設定します。
    fn set_async_breakpoint_by_symbol(&mut self, symbol_name: &str) -> Result<BreakpointId> {
        use crate::breakpoint::BreakpointType;

        let symbol = self.find_best_symbol(symbol_name)?;
        let entry_address = self.offset_to_runtime_addr(symbol.address)?;

        // メモリを取得（借用問題を避けるため後で使う）
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        // 1. Entry用のブレークポイントを設定
        let entry_bp_id = self.breakpoint_manager
            .add_and_enable_with_type(entry_address, memory, BreakpointType::AsyncEntry)?;

        // 2. Exit用のブレークポイントを設定（ret命令を検出）
        if symbol.size > 0 {
            // 関数のバイト列を読み取る
            let code = memory.read(entry_address as usize, symbol.size as usize)?;

            // ret命令を検出
            match crate::disasm::find_ret_instructions(&code, symbol.address) {
                Ok(ret_addrs) => {
                    for ret_addr in ret_addrs {
                        let actual_ret_addr = self.offset_to_runtime_addr(ret_addr)?;

                        // 各ret命令にExitブレークポイントを設定
                        if let Err(e) = self.breakpoint_manager.add_and_enable_with_type(
                            actual_ret_addr,
                            memory,
                            BreakpointType::AsyncExit,
                        ) {
                            eprintln!("Warning: Failed to set exit breakpoint at 0x{:x}: {}", actual_ret_addr, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to disassemble function {}: {}", symbol_name, e);
                }
            }
        }

        // Entryブレークポイントのidを返す
        Ok(entry_bp_id)
    }

    /// ブレークポイントを設定する（アドレス指定）
    pub fn set_breakpoint(&mut self, address: u64) -> Result<BreakpointId> {
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        self.breakpoint_manager.add_and_enable(address, memory)
    }

    /// シンボル名からブレークポイントを設定する
    ///
    /// DWARF行番号情報を使って、関数の最初の有効なソース行にブレークポイントを設定します。
    /// PIEの場合、実行時ベースアドレスを自動的に加算します。
    /// 非PIEの場合、シンボルアドレスは既に絶対アドレスなので加算しません。
    pub fn set_breakpoint_by_symbol(&mut self, symbol_name: &str) -> Result<BreakpointId> {
        let symbol = self.find_best_symbol(symbol_name)?;

        // DWARF行番号情報を使って最初の有効な行のアドレスを取得
        let mut breakpoint_address = symbol.address;
        if let Some(loader) = &self.dwarf_loader {
            let line_provider = LineInfoProvider::new(loader);

            // シンボルのアドレス範囲で最初の有効な行を検索
            let end_addr = if symbol.size > 0 {
                symbol.address + symbol.size
            } else {
                symbol.address + 0x1000 // サイズ不明の場合は適当な範囲
            };

            match line_provider.find_first_line_in_range(symbol.address, end_addr) {
                Ok(Some(first_line_addr)) => {
                    breakpoint_address = first_line_addr;
                }
                Ok(None) | Err(_) => {
                    // 行番号情報が見つからない場合、シンボルアドレスを使用
                }
            }
        }

        let actual_address = self.offset_to_runtime_addr(breakpoint_address)?;
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        self.breakpoint_manager.add_and_enable(actual_address, memory)
    }

    /// ブレークポイントを削除する
    pub fn remove_breakpoint(&mut self, id: BreakpointId) -> Result<()> {
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        self.breakpoint_manager.remove_and_disable(id, memory)
    }

    /// すべてのブレークポイントを取得する
    pub fn breakpoints(&self) -> impl Iterator<Item = &Breakpoint> {
        self.breakpoint_manager.all()
    }

    /// プロセスを実行継続する（停止イベントを待たない）
    pub fn continue_execution(&self) -> Result<()> {
        if let Some(process) = &self.process {
            process.continue_execution()?;
        }
        Ok(())
    }

    /// プロセスを実行継続して停止イベントを待機する
    ///
    /// プロセスを実行継続し、次の停止イベント（ブレークポイント、シグナル、終了など）まで待機します。
    /// ブレークポイントヒット時は、PCを自動的に1バイト戻します（INT3命令の分）。
    pub fn continue_and_wait(&mut self) -> Result<StopReason> {
        let process = self.process.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        // 現在のPCを取得
        let registers = self.require_registers()?;
        let current_pc = registers.get_pc()?;

        // 現在のPCにブレークポイントがあるかチェック
        let bp_at_current_pc = self.breakpoint_manager.find_by_address(current_pc);

        // ブレークポイント上にいる場合、一時的に無効化してステップ実行してから再有効化
        if let Some(bp_id) = bp_at_current_pc {
            self.breakpoint_manager.disable_temporarily(bp_id, memory)?;
            let _ = process.step()?;
            self.breakpoint_manager.reenable(bp_id, memory)?;
        }

        let stop_reason = process.continue_and_wait()?;

        // ブレークポイントヒット時はPCを1バイト戻す（INT3命令の分）
        if stop_reason == StopReason::Breakpoint {
            let registers = self.require_registers()?;
            let pc = registers.get_pc()?;
            registers.set_pc(pc - 1)?;

            // PCを戻した後、Async用のブレークポイントかチェック
            let adjusted_pc = pc - 1;
            if let Some(bp_id) = self.breakpoint_manager.find_by_address(adjusted_pc) {
                if let Some(bp) = self.breakpoint_manager.get(bp_id) {
                    match bp.bp_type {
                        crate::breakpoint::BreakpointType::AsyncEntry => {
                            // Entry: on_poll_entryを呼び出す
                            self.handle_async_entry(adjusted_pc)?;
                        }
                        crate::breakpoint::BreakpointType::AsyncExit => {
                            // Exit: on_poll_exitを呼び出す
                            self.handle_async_exit(adjusted_pc)?;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(stop_reason)
    }

    /// Async関数のエントリー処理
    fn handle_async_entry(&mut self, pc: u64) -> Result<()> {
        use kokia_async::Tid;

        // 現在のスレッドIDを取得（簡易版：PIDを使用）
        let pid = self.pid().ok_or_else(|| anyhow::anyhow!("No process attached"))?;
        let tid = Tid(pid);

        // レジスタから第1引数（self ポインタ）を取得
        // x86_64 System V ABI: 第1引数は RDI
        let registers = self.require_registers()?;
        let child_self = registers.get_rdi()?;

        // 親タスクをフレームスキャンで検出
        // バックトレースを取得し、フレーム1以降から最初の async 関数（{{closure}}）を探す
        let parent_task = self.scan_parent_async_function()?;

        // 子タスクの discriminant を読み取る
        let discriminant = self.read_discriminant(child_self);

        // PCから関数名を解決（デマングル済み）
        let function_name = self.reverse_resolve(pc)
            .map(|sym| sym.demangled_name);

        // AsyncTrackerのon_poll_entryを呼び出す
        if let Err(e) = self.async_tracker.on_poll_entry(tid, child_self, pc, parent_task, discriminant, function_name) {
            eprintln!("Warning: Failed to track async entry: {}", e);
        }

        Ok(())
    }

    /// 親の async 関数をフレームスキャンで検出する
    ///
    /// バックトレースからフレーム1以降の最初の async 関数（{{closure}}）を探し、
    /// その RDI レジスタ値（self ポインタ）を返す
    fn scan_parent_async_function(&self) -> Result<Option<u64>> {
        // バックトレースを取得
        let frames = self.backtrace()?;

        // フレーム1以降を検査（フレーム0は現在の関数）
        for frame in frames.iter().skip(1) {
            if let Some(ref func_name) = frame.function_name {
                // async 関数かどうかを判定（{{closure}} を含むか）
                if func_name.contains("{{closure}}") {
                    // このフレームの saved_rdi を返す
                    return Ok(frame.saved_rdi);
                }
            }
        }

        Ok(None)
    }

    /// Async関数のイグジット処理
    fn handle_async_exit(&mut self, _pc: u64) -> Result<()> {
        use kokia_async::Tid;

        // 現在のスレッドIDを取得
        let pid = self.pid().ok_or_else(|| anyhow::anyhow!("No process attached"))?;
        let tid = Tid(pid);

        // 戻り値（RAX）から Poll::Ready/Pending を判定
        // Poll<T> の discriminant は通常、最初のバイトに格納される
        // Pending = 0, Ready = 1
        let registers = self.require_registers()?;
        let rax = registers.get_rax()?;
        let is_ready = (rax & 0xFF) == 1;

        // スコープスタックが空かチェック（再同期が必要な可能性）
        let scope_stack = self.async_tracker.async_backtrace(tid);
        let needs_resync = scope_stack.is_empty();

        // 再同期が必要な場合、OS スタックから実際のタスクリストを取得
        if needs_resync {
            if let Ok(actual_tasks) = self.extract_async_tasks_from_stack() {
                self.async_tracker.resync_from_stack(tid, actual_tasks);
            }
        }

        // AsyncTrackerのon_poll_exitを呼び出す
        if let Err(e) = self.async_tracker.on_poll_exit(tid, _pc, is_ready) {
            eprintln!("Warning: Failed to track async exit: {}", e);
        }

        Ok(())
    }

    /// 1命令だけ実行する（ステップ実行）
    ///
    /// プロセスの1命令だけを実行し、次の停止イベントまで待機します。
    /// 関数呼び出しの中にも入ります（ステップイン）。
    /// ブレークポイントヒット時は、PCを自動的に1バイト戻します（INT3命令の分）。
    pub fn step(&mut self) -> Result<StopReason> {
        let process = self.process.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        // 現在のPCを取得
        let registers = self.require_registers()?;
        let current_pc = registers.get_pc()?;

        // 現在のPCにブレークポイントがあるかチェック
        let bp_at_current_pc = self.breakpoint_manager.find_by_address(current_pc);

        // ブレークポイント上にいる場合、一時的に無効化してから実行
        if let Some(bp_id) = bp_at_current_pc {
            self.breakpoint_manager.disable_temporarily(bp_id, memory)?;
            let stop_reason = process.step()?;
            self.breakpoint_manager.reenable(bp_id, memory)?;

            // ステップ実行後、新しいPCを取得
            let registers = self.require_registers()?;
            let new_pc = registers.get_pc()?;

            // ステップ先（PC-1）にブレークポイントがあるかチェック
            if let Some(_) = self.breakpoint_manager.find_by_address(new_pc - 1) {
                // ブレークポイントにヒットした
                registers.set_pc(new_pc - 1)?;
                return Ok(StopReason::Breakpoint);
            }

            return Ok(stop_reason);
        }

        // ブレークポイント上にいない場合は通常のステップ実行
        let stop_reason = process.step()?;

        // ステップ実行後、新しいPCを取得
        let registers = self.require_registers()?;
        let new_pc = registers.get_pc()?;

        // ステップ先（PC-1）にブレークポイントがあるかチェック
        if let Some(_) = self.breakpoint_manager.find_by_address(new_pc - 1) {
            // ブレークポイントにヒットした
            registers.set_pc(new_pc - 1)?;
            return Ok(StopReason::Breakpoint);
        }

        Ok(stop_reason)
    }

    /// プログラムカウンタを取得する
    pub fn get_pc(&self) -> Result<u64> {
        let registers = self.require_registers()?;
        registers.get_pc()
    }

    /// Asyncトラッカーを取得する
    pub fn async_tracker(&self) -> &AsyncTracker {
        &self.async_tracker
    }

    /// Asyncトラッカーを可変参照で取得する
    pub fn async_tracker_mut(&mut self) -> &mut AsyncTracker {
        &mut self.async_tracker
    }

    /// プロセスIDを取得する
    pub fn pid(&self) -> Option<i32> {
        self.pid
    }

    /// メモリアクセスを取得する
    pub fn memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    /// レジスタアクセスを取得する
    pub fn registers(&self) -> Option<&Registers> {
        self.registers.as_ref()
    }

    /// バックトレース（コールスタック）を取得する
    ///
    /// フレームポインタ（RBP）をチェーンして呼び出しスタックを辿ります。
    /// 各フレームでリターンアドレスからシンボルを解決します。
    pub fn backtrace(&self) -> Result<Vec<StackFrame>> {
        let registers = self.require_registers()?;
        let memory = self.memory.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        let mut frames = Vec::new();

        // 現在のフレーム（フレーム0）
        let current_pc = registers.get_pc()?;
        let current_rbp = registers.get_rbp()?;

        let function_name = self.reverse_resolve(current_pc)
            .map(|sym| sym.demangled_name.clone());

        let (file, line) = self.get_line_info(current_pc)
            .map(|(f, l)| (Some(f), Some(l)))
            .unwrap_or((None, None));

        // フレーム0の RDI は直接レジスタから取得
        let saved_rdi = registers.get_rdi().ok();

        frames.push(StackFrame {
            frame_number: 0,
            pc: current_pc,
            rbp: current_rbp,
            function_name,
            file,
            line,
            saved_rdi,
        });

        // フレームポインタをチェーンして辿る
        let mut rbp = current_rbp;
        let mut frame_number = 1;
        const MAX_FRAMES: usize = 100; // 無限ループ防止

        while frame_number < MAX_FRAMES {
            // RBP が 0 または小さすぎる場合は終了
            if rbp == 0 || rbp < 0x1000 {
                break;
            }

            // RBP が有効なメモリ範囲にあるかチェック
            if !memory.is_mapped(rbp as usize)? {
                break;
            }

            // RBP が指すメモリから前のRBPを読み取る
            let prev_rbp = match memory.read_u64(rbp as usize) {
                Ok(val) => val,
                Err(_) => break,
            };

            // RBP+8 からリターンアドレスを読み取る
            let return_address = match memory.read_u64((rbp + 8) as usize) {
                Ok(val) => val,
                Err(_) => break,
            };

            // リターンアドレスが無効な場合は終了
            if return_address == 0 || return_address < 0x1000 {
                break;
            }

            // リターンアドレスからシンボルを解決
            let function_name = self.reverse_resolve(return_address)
                .map(|sym| sym.demangled_name.clone());

            let (file, line) = self.get_line_info(return_address)
                .map(|(f, l)| (Some(f), Some(l)))
                .unwrap_or((None, None));

            // スタックフレームから self ポインタを探索
            // async 関数の場合、RDI（self）がスタックに保存されている
            let saved_rdi = self.scan_stack_for_self_ptr(rbp, memory);

            frames.push(StackFrame {
                frame_number,
                pc: return_address,
                rbp: prev_rbp,
                function_name,
                file,
                line,
                saved_rdi,
            });

            // 次のフレームへ
            rbp = prev_rbp;
            frame_number += 1;

            // 前のRBPが現在のRBP以下の場合（スタックが逆方向）は終了
            if prev_rbp <= current_rbp {
                break;
            }
        }

        Ok(frames)
    }

    /// スタックフレームから self ポインタ（RDI の保存値）を探索する
    ///
    /// async 関数のスタックフレーム内から妥当なポインタ値を探索します。
    /// スタックフレームの範囲（RBP-256 ～ RBP）を8バイトずつスキャンし、
    /// ヒープ領域を指す可能性のあるポインタ値を返します。
    ///
    /// # Arguments
    /// * `rbp` - フレームのベースポインタ
    /// * `memory` - メモリアクセス
    ///
    /// # Returns
    /// 最初に見つかった妥当なポインタ値、または None
    fn scan_stack_for_self_ptr(&self, rbp: u64, memory: &Memory) -> Option<u64> {
        // スタックフレームのサイズを制限（256バイト程度）
        const FRAME_SCAN_SIZE: u64 = 256;

        // RBP から下方向にスキャン（ローカル変数領域）
        // RBP-8, RBP-16, ... とスキャン
        for offset in (8..=FRAME_SCAN_SIZE).step_by(8) {
            if rbp < offset {
                break;
            }

            let addr = rbp - offset;

            // メモリが読み取り可能かチェック
            if let Ok(value) = memory.read_u64(addr as usize) {
                // ポインタ値として妥当かチェック
                // - NULL ではない
                // - 小さすぎない（0x1000 以上）
                // - マップされた領域を指している
                if value >= 0x1000 && value < 0x7fff_ffff_ffff {
                    // ヒープ領域やスタック領域を指している可能性が高い
                    // 最初に見つかったものを返す（簡易実装）
                    if memory.is_mapped(value as usize).unwrap_or(false) {
                        return Some(value);
                    }
                }
            }
        }

        None
    }

    /// タスクの discriminant（停止点インデックス）を読み取る
    ///
    /// 生成器オブジェクトのメモリから discriminant 値を読み取ります。
    /// Rust の async 関数は enum のような構造で、discriminant は最初の数バイトに配置されます。
    ///
    /// # Arguments
    /// * `task_ptr` - タスク（生成器）のアドレス
    ///
    /// # Returns
    /// discriminant 値（u32）、または読み取り失敗時は None
    pub fn read_discriminant(&self, task_ptr: u64) -> Option<u64> {
        let memory = self.memory.as_ref()?;

        // 生成器の discriminant は通常、構造体の先頭に配置される
        // サイズは u32 または u64 の場合がある
        // まず u32 として読み取ってみる
        if let Ok(discr) = memory.read_u32(task_ptr as usize) {
            return Some(discr as u64);
        }

        None
    }

    /// OS スタックから async 関数のタスクリストを抽出する
    ///
    /// バックトレースから async 関数（{{closure}}）のみを抽出し、
    /// それぞれの self ポインタのリストを返します（子→親の順）。
    ///
    /// # Returns
    /// タスク ID（self ポインタ）のリスト
    pub fn extract_async_tasks_from_stack(&self) -> Result<Vec<u64>> {
        let frames = self.backtrace()?;
        let mut tasks = Vec::new();

        for frame in frames {
            if let Some(ref func_name) = frame.function_name {
                // async 関数かどうかを判定（{{closure}} を含むか）
                if func_name.contains("{{closure}}") {
                    // saved_rdi があればタスク ID として追加
                    if let Some(task_id) = frame.saved_rdi {
                        tasks.push(task_id);
                    }
                }
            }
        }

        Ok(tasks)
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

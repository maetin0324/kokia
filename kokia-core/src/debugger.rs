//! デバッガのメインロジック

use crate::{breakpoint::BreakpointManager, Breakpoint, BreakpointId, Result};
use kokia_async::{GenFutureDetector, TaskTracker};
use kokia_dwarf::{DwarfLoader, LineInfoProvider, Symbol, SymbolResolver};
use kokia_target::{Memory, Process, Registers, StopReason};
use std::path::Path;

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
    /// GenFuture検出器
    genfuture_detector: GenFutureDetector,
    /// タスクトラッカー
    task_tracker: TaskTracker,
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
            genfuture_detector: GenFutureDetector::new()
                .expect("Failed to create GenFutureDetector"),
            task_tracker: TaskTracker::new(),
            breakpoint_manager: BreakpointManager::new(),
        }
    }

    /// プロセスにアタッチされているか確認し、Registersへの参照を取得
    fn require_registers(&self) -> Result<&Registers> {
        self.registers
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))
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
        self.symbol_resolver.as_ref()?.reverse_resolve(addr)
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
            .filter(|sym| self.genfuture_detector.is_async_related(&sym.name))
            .cloned()
            .collect()
    }

    /// ブレークポイントを設定する（アドレス指定）
    pub fn set_breakpoint(&mut self, address: u64) -> Result<BreakpointId> {
        let memory = self
            .memory
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        self.breakpoint_manager.add_and_enable(address, memory)
    }

    /// シンボル名からブレークポイントを設定する
    ///
    /// DWARF行番号情報を使って、関数の最初の有効なソース行にブレークポイントを設定します。
    /// PIEの場合、実行時ベースアドレスを自動的に加算します。
    /// 非PIEの場合、シンボルアドレスは既に絶対アドレスなので加算しません。
    pub fn set_breakpoint_by_symbol(&mut self, symbol_name: &str) -> Result<BreakpointId> {
        let memory = self
            .memory
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        // シンボルを解決してSymbolオブジェクトを取得
        let symbols = self.find_symbols(symbol_name);
        let symbol = symbols
            .iter()
            .find(|s| s.name == symbol_name)
            .ok_or_else(|| anyhow::anyhow!("Symbol not found: {}", symbol_name))?;

        let symbol_address = symbol.address;
        let symbol_size = symbol.size;

        // PIEかどうかを確認
        let is_pie = self
            .symbol_resolver
            .as_ref()
            .map(|r| r.is_pie())
            .unwrap_or(false);

        // ベースアドレスを取得（PIEの場合のみ使用）
        let base_address = if is_pie {
            memory.get_base_address()? as u64
        } else {
            0
        };

        // DWARF行番号情報を使って最初の有効な行のアドレスを取得
        let mut breakpoint_address = symbol_address;
        if let Some(loader) = &self.dwarf_loader {
            let line_provider = LineInfoProvider::new(loader);

            // シンボルのアドレス範囲で最初の有効な行を検索
            let end_addr = if symbol_size > 0 {
                symbol_address + symbol_size
            } else {
                symbol_address + 0x1000 // サイズ不明の場合は適当な範囲
            };

            match line_provider.find_first_line_in_range(symbol_address, end_addr) {
                Ok(Some(first_line_addr)) => {
                    eprintln!("[DEBUG] Found first line at offset 0x{:x} (symbol at 0x{:x})", first_line_addr, symbol_address);
                    breakpoint_address = first_line_addr;
                }
                Ok(None) => {
                    eprintln!("[DEBUG] No line info found, using symbol address 0x{:x}", symbol_address);
                }
                Err(e) => {
                    eprintln!("[DEBUG] Error finding line info: {}, using symbol address 0x{:x}", e, symbol_address);
                }
            }
        }

        // PIEの場合のみベースアドレスを加算
        let actual_address = if is_pie {
            base_address + breakpoint_address
        } else {
            // 非PIEの場合、シンボルアドレスは既に絶対アドレス
            breakpoint_address
        };

        eprintln!("[DEBUG] Setting breakpoint at 0x{:x} (PIE: {}, base: 0x{:x})", actual_address, is_pie, base_address);
        self.breakpoint_manager.add_and_enable(actual_address, memory)
    }

    /// ブレークポイントを削除する
    pub fn remove_breakpoint(&mut self, id: BreakpointId) -> Result<()> {
        let memory = self
            .memory
            .as_ref()
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
    pub fn continue_and_wait(&self) -> Result<StopReason> {
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;

        let stop_reason = process.continue_and_wait()?;

        // ブレークポイントヒット時はPCを1バイト戻す（INT3命令の分）
        if stop_reason == StopReason::Breakpoint {
            let registers = self.require_registers()?;
            let pc = registers.get_pc()?;
            registers.set_pc(pc - 1)?;
        }

        Ok(stop_reason)
    }

    /// プログラムカウンタを取得する
    pub fn get_pc(&self) -> Result<u64> {
        let registers = self.require_registers()?;
        registers.get_pc()
    }

    /// タスクトラッカーを取得する
    pub fn task_tracker(&self) -> &TaskTracker {
        &self.task_tracker
    }

    /// タスクトラッカーを可変参照で取得する
    pub fn task_tracker_mut(&mut self) -> &mut TaskTracker {
        &mut self.task_tracker
    }

    /// メモリアクセスを取得する
    pub fn memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    /// レジスタアクセスを取得する
    pub fn registers(&self) -> Option<&Registers> {
        self.registers.as_ref()
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

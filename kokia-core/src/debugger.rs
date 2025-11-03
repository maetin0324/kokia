//! デバッガのメインロジック

use crate::{breakpoint::BreakpointManager, Breakpoint, BreakpointId, Result};
use kokia_async::{GenFutureDetector, TaskTracker};
use kokia_dwarf::{DwarfLoader, Symbol, SymbolResolver};
use kokia_target::{Memory, Process, Registers};
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
    /// プロセスはexecve直後に停止状態で開始されます。
    /// ユーザーは continue コマンドでプロセスを実行開始できます。
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

    /// ブレークポイントを設定する
    pub fn set_breakpoint(&mut self, address: u64) -> Result<BreakpointId> {
        let memory = self
            .memory
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not attached to a process"))?;
        self.breakpoint_manager.add_and_enable(address, memory)
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

    /// プロセスを実行継続する
    pub fn continue_execution(&self) -> Result<()> {
        if let Some(process) = &self.process {
            process.continue_execution()?;
        }
        Ok(())
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

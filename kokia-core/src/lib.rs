//! Kokia デバッガのコア機能
//!
//! このクレートは、デバッガの中核となるロジックを提供します。
//! ターゲットプロセスの制御、デバッグ情報の解析、非同期関数のトレースを統合します。

pub mod debugger;
pub mod breakpoint;
pub mod command;
pub mod disasm;
pub mod errors;
pub mod parse;

pub use debugger::{Debugger, StackFrame};
pub use breakpoint::{Breakpoint, BreakpointId, BreakpointType};
pub use command::Command;

// 他のクレートから使用するために再エクスポート
pub use kokia_dwarf::Symbol;
pub use kokia_target::StopReason;
pub use kokia_async::{Tid, TaskInfo};

/// デバッガの結果型
pub type Result<T> = anyhow::Result<T>;

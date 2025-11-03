//! Kokia ターゲットプロセス制御
//!
//! このクレートは、デバッグ対象のプロセスを制御するための低レベル機能を提供します。
//! ptrace、レジスタアクセス、メモリアクセス、ブレークポイント設定などを行います。

pub mod process;
pub mod thread;
pub mod memory;
pub mod registers;
pub mod breakpoint;

pub use process::Process;
pub use thread::{Thread, ThreadId};
pub use memory::Memory;
pub use registers::Registers;
pub use breakpoint::{SoftwareBreakpoint, HardwareBreakpoint};

/// ターゲット制御の結果型
pub type Result<T> = anyhow::Result<T>;

//! Kokia DWARF デバッグ情報解析
//!
//! このクレートは、ELFファイルとDWARFデバッグ情報の解析機能を提供します。
//! シンボル名の解決、アドレスからソース行への変換、変数のロケーション評価などを行います。

pub mod loader;
pub mod symbols;
pub mod lines;
pub mod variables;

pub use loader::DwarfLoader;
pub use symbols::{Symbol, SymbolResolver};
pub use lines::{LineInfo, LineInfoProvider};
pub use variables::VariableLocator;

/// DWARF解析の結果型
pub type Result<T> = anyhow::Result<T>;

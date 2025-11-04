//! エラーメッセージ定数

/// プロセスに接続されていない場合のエラーメッセージ
pub const ERR_NOT_ATTACHED: &str = "Not attached to a process";

/// DWARF情報がロードされていない場合のエラーメッセージ
pub const ERR_DWARF_NOT_LOADED: &str = "DWARF information not loaded";

/// シンボルが見つからない場合のエラーメッセージ
pub const ERR_SYMBOL_NOT_FOUND: &str = "Symbol not found";

/// ブレークポイントが見つからない場合のエラーメッセージ
pub const ERR_BREAKPOINT_NOT_FOUND: &str = "Breakpoint not found";

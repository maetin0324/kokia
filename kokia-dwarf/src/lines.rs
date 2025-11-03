//! ソース行情報

use crate::Result;

/// ソース行情報
#[derive(Debug, Clone)]
pub struct LineInfo {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// ソース行情報の取得
pub struct LineInfoProvider {
    // TODO: addr2lineコンテキストを保持する
}

impl LineInfoProvider {
    /// ソース行情報プロバイダを作成する
    pub fn new() -> Self {
        Self {}
    }

    /// アドレスからソース行情報を取得する
    pub fn lookup(&self, _addr: u64) -> Result<Option<LineInfo>> {
        // TODO: addr2lineを使用してアドレスからソース行を検索する
        Ok(None)
    }
}

impl Default for LineInfoProvider {
    fn default() -> Self {
        Self::new()
    }
}

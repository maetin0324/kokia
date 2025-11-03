//! 変数ロケーション評価

use crate::Result;

/// 変数の値
#[derive(Debug, Clone)]
pub enum VariableValue {
    Integer(i64),
    UnsignedInteger(u64),
    Float(f64),
    Boolean(bool),
    String(String),
    Address(u64),
    Bytes(Vec<u8>),
}

/// 変数情報
#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub type_name: String,
    pub value: Option<VariableValue>,
}

/// 変数ロケーター
pub struct VariableLocator {
    // TODO: DWARF変数情報とロケーション式評価機を保持する
}

impl VariableLocator {
    /// 変数ロケーターを作成する
    pub fn new() -> Self {
        Self {}
    }

    /// 関数のローカル変数を取得する
    pub fn get_locals(&self, _pc: u64) -> Result<Vec<Variable>> {
        // TODO: DWARFからローカル変数の情報を取得し、ロケーション式を評価する
        Ok(Vec::new())
    }
}

impl Default for VariableLocator {
    fn default() -> Self {
        Self::new()
    }
}

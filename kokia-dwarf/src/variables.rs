//! 変数ロケーション評価

use crate::{DwarfLoader, Result};
use gimli::Reader;

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
    Unavailable,
}

impl std::fmt::Display for VariableValue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            VariableValue::Integer(i) => write!(f, "{}", i),
            VariableValue::UnsignedInteger(u) => write!(f, "{}", u),
            VariableValue::Float(fl) => write!(f, "{}", fl),
            VariableValue::Boolean(b) => write!(f, "{}", b),
            VariableValue::String(s) => write!(f, "\"{}\"", s),
            VariableValue::Address(addr) => write!(f, "0x{:x}", addr),
            VariableValue::Bytes(bytes) => {
                write!(f, "[")?;
                for (i, b) in bytes.iter().take(8).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:02x}", b)?;
                }
                if bytes.len() > 8 {
                    write!(f, ", ...")?;
                }
                write!(f, "]")
            }
            VariableValue::Unavailable => write!(f, "<unavailable>"),
        }
    }
}

/// 変数情報
#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub type_name: String,
    pub value: Option<VariableValue>,
    pub location: VariableLocation,
}

/// 変数のロケーション
#[derive(Debug, Clone)]
pub enum VariableLocation {
    /// フレームベースからのオフセット
    FrameOffset(i64),
    /// レジスタ
    Register(u16),
    /// 静的アドレス
    Address(u64),
    /// 最適化により削除された
    OptimizedOut,
    /// 不明
    Unknown,
}

/// ローカル変数情報
#[derive(Debug, Clone)]
pub struct LocalVariable {
    /// 変数名
    pub name: String,
    /// フレームベースからのオフセット（DW_OP_fbreg の場合）
    pub offset_from_frame_base: Option<i64>,
    /// 型名（簡略化版）
    pub type_name: Option<String>,
}

/// 変数ロケーター
pub struct VariableLocator<'a> {
    loader: &'a DwarfLoader,
}

impl<'a> VariableLocator<'a> {
    /// 変数ロケーターを作成する
    pub fn new(loader: &'a DwarfLoader) -> Self {
        Self { loader }
    }

    /// 関数のローカル変数を取得する
    pub fn get_locals(&self, pc: u64) -> Result<Vec<Variable>> {
        let dwarf = self.loader.dwarf();
        let mut variables = Vec::new();

        // 各コンパイルユニットを走査
        let mut iter = dwarf.units();
        while let Some(header) = iter.next()? {
            let unit = dwarf.unit(header)?;

            // PCを含む関数DIEを探す
            if let Some(function_die_offset) = self.find_function_at_pc(&unit, pc)? {
                // 関数DIEの子（ローカル変数）を列挙
                variables.extend(self.enumerate_local_variables(&unit, function_die_offset)?);
            }
        }

        Ok(variables)
    }

    /// PCを含む関数DIEを探す
    fn find_function_at_pc<R: Reader>(
        &self,
        unit: &gimli::Unit<R>,
        pc: u64,
    ) -> Result<Option<gimli::UnitOffset<R::Offset>>> {
        crate::utils::FunctionFinder::find_at_pc(unit, pc)
    }

    /// ローカル変数を列挙する
    fn enumerate_local_variables<R: Reader>(
        &self,
        unit: &gimli::Unit<R>,
        function_offset: gimli::UnitOffset<R::Offset>,
    ) -> Result<Vec<Variable>> {
        let mut variables = Vec::new();
        let mut tree = unit.entries_tree(Some(function_offset))?;
        let root = tree.root()?;

        // 関数DIEの子を走査
        let mut children = root.children();
        while let Some(child) = children.next()? {
            let entry = child.entry();

            // DW_TAG_variable または DW_TAG_formal_parameter (引数)
            if entry.tag() == gimli::DW_TAG_variable
                || entry.tag() == gimli::DW_TAG_formal_parameter
            {
                if let Some(var) = self.extract_variable_info(unit, entry)? {
                    variables.push(var);
                }
            }
        }

        Ok(variables)
    }

    /// 変数情報を抽出する
    fn extract_variable_info<R: Reader>(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<Variable>> {
        // 変数名を取得
        let name = match entry.attr_value(gimli::DW_AT_name)? {
            Some(gimli::AttributeValue::String(s)) => {
                s.to_string_lossy()?.into_owned()
            }
            Some(gimli::AttributeValue::DebugStrRef(_)) => {
                // DebugStrRef の場合は簡易的にスキップ
                return Ok(None);
            }
            _ => return Ok(None),
        };

        // 型名を取得（簡易実装）
        let type_name = self.get_type_name(unit, entry)?
            .unwrap_or_else(|| "<unknown>".to_string());

        // ロケーションを取得
        let location = self.get_variable_location(unit, entry)?;

        Ok(Some(Variable {
            name,
            type_name,
            value: None, // 値の読み取りは後で実装
            location,
        }))
    }

    /// 変数のロケーションを取得
    fn get_variable_location<R: Reader>(
        &self,
        _unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<VariableLocation> {
        let location_attr = match entry.attr_value(gimli::DW_AT_location)? {
            Some(attr) => attr,
            None => return Ok(VariableLocation::OptimizedOut),
        };

        match location_attr {
            gimli::AttributeValue::Exprloc(expr) => {
                // 簡易的なロケーション式の評価
                let mut data = expr.0;
                if let Ok(op) = data.read_u8() {
                    match op {
                        // DW_OP_fbreg: フレームベースからのオフセット
                        op if op == gimli::constants::DW_OP_fbreg.0 => {
                            let offset = data.read_sleb128().unwrap_or(0);
                            Ok(VariableLocation::FrameOffset(offset))
                        }
                        // DW_OP_addr: 静的アドレス
                        op if op == gimli::constants::DW_OP_addr.0 => {
                            let addr = data.read_u64().unwrap_or(0);
                            Ok(VariableLocation::Address(addr))
                        }
                        // DW_OP_regN: レジスタ
                        op if op >= gimli::constants::DW_OP_reg0.0 && op <= gimli::constants::DW_OP_reg31.0 => {
                            let reg = op - gimli::constants::DW_OP_reg0.0;
                            Ok(VariableLocation::Register(reg as u16))
                        }
                        _ => Ok(VariableLocation::Unknown),
                    }
                } else {
                    Ok(VariableLocation::Unknown)
                }
            }
            _ => Ok(VariableLocation::Unknown),
        }
    }

    /// 型名を取得（簡易実装）
    fn get_type_name<R: Reader>(
        &self,
        unit: &gimli::Unit<R>,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<Option<String>> {
        // DW_AT_type を取得
        let type_attr = match entry.attr_value(gimli::DW_AT_type)? {
            Some(gimli::AttributeValue::UnitRef(offset)) => offset,
            _ => return Ok(None),
        };

        // 型DIEを取得
        let mut entries = unit.entries_at_offset(type_attr)?;
        if let Some((_, type_entry)) = entries.next_dfs()? {
            // 型名を取得
            match type_entry.attr_value(gimli::DW_AT_name)? {
                Some(gimli::AttributeValue::String(s)) => {
                    return Ok(Some(s.to_string_lossy()?.into_owned()));
                }
                Some(gimli::AttributeValue::DebugStrRef(_)) => {
                    // DebugStrRef の場合は基本型のチェックにフォールスルー
                }
                _ => {}
            }

            // 基本型の場合、エンコーディングから名前を推測
            if type_entry.tag() == gimli::DW_TAG_base_type {
                return Ok(Some(self.infer_base_type_name(&type_entry)?));
            }
        }

        Ok(None)
    }

    /// 基本型の名前を推測
    fn infer_base_type_name<R: Reader>(
        &self,
        entry: &gimli::DebuggingInformationEntry<R>,
    ) -> Result<String> {
        let encoding = entry.attr_value(gimli::DW_AT_encoding)?;
        let size = entry.attr_value(gimli::DW_AT_byte_size)?;

        let type_name = match (encoding, size) {
            (Some(gimli::AttributeValue::Encoding(enc)), Some(gimli::AttributeValue::Udata(s))) => {
                match enc {
                    gimli::DW_ATE_signed => match s {
                        1 => "i8",
                        2 => "i16",
                        4 => "i32",
                        8 => "i64",
                        _ => "int",
                    },
                    gimli::DW_ATE_unsigned => match s {
                        1 => "u8",
                        2 => "u16",
                        4 => "u32",
                        8 => "u64",
                        _ => "uint",
                    },
                    gimli::DW_ATE_float => match s {
                        4 => "f32",
                        8 => "f64",
                        _ => "float",
                    },
                    gimli::DW_ATE_boolean => "bool",
                    _ => "<unknown>",
                }
            }
            _ => "<unknown>",
        };

        Ok(type_name.to_string())
    }
}

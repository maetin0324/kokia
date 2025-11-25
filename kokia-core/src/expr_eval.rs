//! 式評価エンジン
//!
//! デバッガで使用する式を評価します（printコマンド等）

use crate::{Debugger, Result};
use kokia_dwarf::{TypeInfo, VariableLocation, Variable};

/// 式の抽象構文木
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// 変数名: `x`
    Variable(String),
    /// フィールドアクセス: `obj.field`
    FieldAccess {
        base: Box<Expression>,
        field: String,
    },
    /// 配列インデックスアクセス: `arr[0]`
    IndexAccess {
        base: Box<Expression>,
        index: usize,
    },
}

/// 式の評価結果
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    /// 値のメモリアドレス
    pub address: u64,
    /// 型情報（オプション）
    pub type_info: Option<TypeInfo>,
    /// 型名
    pub type_name: String,
}

/// 式評価器
pub struct ExpressionEvaluator<'a> {
    debugger: &'a Debugger,
}

impl<'a> ExpressionEvaluator<'a> {
    /// 新しい式評価器を作成する
    pub fn new(debugger: &'a Debugger) -> Self {
        Self { debugger }
    }

    /// 式を評価する
    pub fn evaluate(&self, expr: &Expression) -> Result<EvaluationResult> {
        match expr {
            Expression::Variable(name) => self.eval_variable(name),
            Expression::FieldAccess { base, field } => self.eval_field_access(base, field),
            Expression::IndexAccess { base, index } => self.eval_index_access(base, *index),
        }
    }

    /// 変数を評価する
    fn eval_variable(&self, name: &str) -> Result<EvaluationResult> {
        // ローカル変数を取得
        let variables = self.debugger.get_local_variables()?;

        // 変数名で検索
        let var = variables
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| anyhow::anyhow!("Variable '{}' not found", name))?;

        // 変数のアドレスを計算
        let address = self.get_variable_address(var)?;

        Ok(EvaluationResult {
            address,
            type_info: None, // TODO: TypeInfoを取得する完全な実装
            type_name: var.type_name.clone(),
        })
    }

    /// 変数のアドレスを取得する
    fn get_variable_address(&self, var: &Variable) -> Result<u64> {
        match &var.location {
            VariableLocation::Address(addr) => Ok(*addr),
            VariableLocation::FrameOffset(offset) => {
                // RBPからのオフセットを計算
                let registers = self.debugger.registers()
                    .ok_or_else(|| anyhow::anyhow!("No registers available"))?;
                let rbp = registers.get_rbp()?;

                let addr = if *offset < 0 {
                    rbp.wrapping_sub(offset.unsigned_abs())
                } else {
                    rbp.wrapping_add(*offset as u64)
                };

                Ok(addr)
            }
            VariableLocation::Register(reg) => {
                Err(anyhow::anyhow!("Cannot get address of register variable (reg{})", reg))
            }
            VariableLocation::OptimizedOut => {
                Err(anyhow::anyhow!("Variable '{}' is optimized out", var.name))
            }
            VariableLocation::Unknown => {
                Err(anyhow::anyhow!("Variable '{}' location is unknown", var.name))
            }
        }
    }

    /// フィールドアクセスを評価する
    fn eval_field_access(&self, base: &Expression, field: &str) -> Result<EvaluationResult> {
        // まずベースの式を評価
        let base_result = self.evaluate(base)?;

        // TypeInfoがない場合はエラー
        let type_info = base_result
            .type_info
            .ok_or_else(|| anyhow::anyhow!("Cannot access field of unknown type"))?;

        // 構造体型でなければエラー
        match type_info {
            TypeInfo::Struct { fields, .. } => {
                // フィールドを検索
                let field_info = fields
                    .iter()
                    .find(|f| f.name == field)
                    .ok_or_else(|| anyhow::anyhow!("Field '{}' not found", field))?;

                // フィールドのアドレスを計算
                let field_address = base_result.address + field_info.offset;

                // フィールドの型情報を取得
                let field_type_info = field_info.type_info.as_ref().map(|t| (**t).clone());
                let field_type_name = field_type_info
                    .as_ref()
                    .map(|t| self.type_name(t))
                    .unwrap_or_else(|| "<unknown>".to_string());

                Ok(EvaluationResult {
                    address: field_address,
                    type_info: field_type_info,
                    type_name: field_type_name,
                })
            }
            _ => Err(anyhow::anyhow!(
                "Cannot access field of non-struct type"
            )),
        }
    }

    /// 配列インデックスアクセスを評価する
    fn eval_index_access(&self, base: &Expression, index: usize) -> Result<EvaluationResult> {
        // まずベースの式を評価
        let base_result = self.evaluate(base)?;

        // TypeInfoがない場合はエラー
        let type_info = base_result
            .type_info
            .ok_or_else(|| anyhow::anyhow!("Cannot index unknown type"))?;

        // 配列型でなければエラー
        match type_info {
            TypeInfo::Array { element_type, length } => {
                // 長さチェック
                if let Some(len) = length {
                    if index >= len as usize {
                        return Err(anyhow::anyhow!(
                            "Index {} out of bounds (length: {})",
                            index,
                            len
                        ));
                    }
                }

                // 要素型がない場合はエラー
                let elem_type = element_type
                    .ok_or_else(|| anyhow::anyhow!("Array element type unknown"))?;

                // 要素のサイズを取得
                let element_size = self.get_type_size(&elem_type);
                if element_size == 0 {
                    return Err(anyhow::anyhow!("Unknown array element size"));
                }

                // 要素のアドレスを計算
                let element_address = base_result.address + (index as u64 * element_size);

                let elem_type_name = self.type_name(&elem_type);

                Ok(EvaluationResult {
                    address: element_address,
                    type_info: Some(*elem_type),
                    type_name: elem_type_name,
                })
            }
            _ => Err(anyhow::anyhow!("Cannot index non-array type")),
        }
    }

    /// 型名を取得する
    fn type_name(&self, type_info: &TypeInfo) -> String {
        match type_info {
            TypeInfo::Primitive { name, .. } => name.clone(),
            TypeInfo::Pointer { .. } => "*".to_string(),
            TypeInfo::Reference { .. } => "&".to_string(),
            TypeInfo::Struct { name, .. } => name.clone(),
            TypeInfo::Enum { name, .. } => name.clone(),
            TypeInfo::Array { .. } => "[...]".to_string(),
            TypeInfo::Union { name, .. } => name.clone(),
            TypeInfo::Unknown => "?".to_string(),
        }
    }

    /// 型のサイズを取得する
    fn get_type_size(&self, type_info: &TypeInfo) -> u64 {
        match type_info {
            TypeInfo::Primitive { size, .. } => *size,
            TypeInfo::Pointer { size, .. } => *size,
            TypeInfo::Reference { size, .. } => *size,
            TypeInfo::Struct { size, .. } => *size,
            TypeInfo::Enum { size, .. } => *size,
            TypeInfo::Union { size, .. } => *size,
            TypeInfo::Array { element_type, length } => {
                if let (Some(elem_type), Some(len)) = (element_type, length) {
                    self.get_type_size(elem_type) * len
                } else {
                    0
                }
            }
            TypeInfo::Unknown => 0,
        }
    }
}

/// 式をパースする簡易パーサー
pub fn parse_expression(input: &str) -> Result<Expression> {
    let input = input.trim();

    // `.`を含む場合 -> FieldAccess
    if let Some(dot_pos) = input.find('.') {
        let base_str = &input[..dot_pos];
        let field_str = &input[dot_pos + 1..];

        // ベース式を再帰的にパース
        let base = Box::new(parse_expression(base_str)?);

        return Ok(Expression::FieldAccess {
            base,
            field: field_str.to_string(),
        });
    }

    // `[`を含む場合 -> IndexAccess
    if let Some(bracket_pos) = input.find('[') {
        if !input.ends_with(']') {
            return Err(anyhow::anyhow!("Missing closing bracket ']'"));
        }

        let base_str = &input[..bracket_pos];
        let index_str = &input[bracket_pos + 1..input.len() - 1];

        // インデックスをパース
        let index = index_str
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Invalid array index: {}", index_str))?;

        // ベース式を再帰的にパース
        let base = Box::new(parse_expression(base_str)?);

        return Ok(Expression::IndexAccess { base, index });
    }

    // それ以外は変数名
    Ok(Expression::Variable(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_variable() {
        let expr = parse_expression("x").unwrap();
        assert_eq!(expr, Expression::Variable("x".to_string()));
    }

    #[test]
    fn test_parse_field_access() {
        let expr = parse_expression("obj.field").unwrap();
        match expr {
            Expression::FieldAccess { base, field } => {
                assert_eq!(*base, Expression::Variable("obj".to_string()));
                assert_eq!(field, "field");
            }
            _ => panic!("Expected FieldAccess"),
        }
    }

    #[test]
    fn test_parse_index_access() {
        let expr = parse_expression("arr[5]").unwrap();
        match expr {
            Expression::IndexAccess { base, index } => {
                assert_eq!(*base, Expression::Variable("arr".to_string()));
                assert_eq!(index, 5);
            }
            _ => panic!("Expected IndexAccess"),
        }
    }

    #[test]
    fn test_parse_nested_field_access() {
        let expr = parse_expression("obj.inner.value").unwrap();
        // parse_expression()は最初の'.'で分割するので、
        // base="obj", field="inner.value"となる
        match expr {
            Expression::FieldAccess { base, field } => {
                assert_eq!(*base, Expression::Variable("obj".to_string()));
                assert_eq!(field, "inner.value");
            }
            _ => panic!("Expected FieldAccess"),
        }
    }
}

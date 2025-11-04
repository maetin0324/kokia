//! Generator (async fn) レイアウト解析
//!
//! このモジュールは、Rustのasync関数（generator）の内部構造を解析し、
//! discriminant（判別子）とactive variantのフィールドを取得します。

use anyhow::Result;

/// Generatorのフィールド情報
#[derive(Debug, Clone)]
pub struct GeneratorField {
    /// フィールド名
    pub name: String,
    /// self からのオフセット（バイト）
    pub offset: u64,
    /// サイズ（バイト）
    pub size: u64,
    /// 型名
    pub type_name: Option<String>,
}

/// Generator の discriminant（判別子）情報
#[derive(Debug, Clone)]
pub struct DiscriminantInfo {
    /// discriminant のオフセット
    pub offset: u64,
    /// discriminant のサイズ（バイト）
    pub size: u64,
}

/// Generator レイアウト解析器
pub struct GeneratorAnalyzer<'a> {
    dwarf: &'a gimli::Dwarf<gimli::EndianSlice<'a, gimli::RunTimeEndian>>,
}

impl<'a> GeneratorAnalyzer<'a> {
    /// 新しい解析器を作成
    pub fn new(dwarf: &'a gimli::Dwarf<gimli::EndianSlice<'a, gimli::RunTimeEndian>>) -> Self {
        Self { dwarf }
    }

    /// 指定アドレスのgenerator型のdiscriminant情報を取得
    ///
    /// # Arguments
    /// * `pc` - 現在のプログラムカウンタ（generator内のアドレス）
    ///
    /// # Returns
    /// discriminant情報、見つからない場合はNone
    pub fn get_discriminant_info(&self, pc: u64) -> Result<Option<DiscriminantInfo>> {
        // PCを含む関数を検索
        let function_die = self.find_function_at_pc(pc)?;
        if function_die.is_none() {
            return Ok(None);
        }

        // 関数からgenerator型を取得
        // TODO: 実装を完成させる
        // generator型は通常、関数の戻り値型や内部の型として現れる

        Ok(None)
    }

    /// Active variantのフィールド一覧を取得
    ///
    /// # Arguments
    /// * `_pc` - 現在のプログラムカウンタ
    /// * `_discriminant_value` - 現在のdiscriminant値
    ///
    /// # Returns
    /// フィールド情報のベクタ
    pub fn get_variant_fields(
        &self,
        _pc: u64,
        _discriminant_value: u64,
    ) -> Result<Vec<GeneratorField>> {
        // TODO: 実装を完成させる
        Ok(Vec::new())
    }

    /// PCを含む関数DIEを検索
    fn find_function_at_pc(&self, pc: u64) -> Result<Option<gimli::UnitOffset>> {
        let mut iter = self.dwarf.units();

        while let Some(header) = iter.next()? {
            let unit = self.dwarf.unit(header)?;
            if let Some(offset) = kokia_dwarf::FunctionFinder::find_at_pc(&unit, pc)? {
                return Ok(Some(offset));
            }
        }

        Ok(None)
    }
}

/// フィールド名を正規化（実装依存の接尾辞を除去）
///
/// `__await_3`, `<local>@5` などの実装依存名を人間可読な形式に変換します。
pub fn normalize_field_name(name: &str) -> String {
    // __await_N パターンを除去
    if name.starts_with("__") && name.contains("await") {
        return format!("<{}>", name);
    }

    // <local>@N パターンの @ 以降を除去
    if let Some(idx) = name.find('@') {
        return name[..idx].to_string();
    }

    // タプルフィールド .0, .1 などはそのまま
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_field_name() {
        assert_eq!(normalize_field_name("value"), "value");
        assert_eq!(normalize_field_name("__await_3"), "<__await_3>");
        assert_eq!(normalize_field_name("local@5"), "local");
        assert_eq!(normalize_field_name(".0"), ".0");
    }
}

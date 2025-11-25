//! 変数値のフォーマット
//!
//! 型情報に基づいて変数の値を人間が読みやすい形式でフォーマットします。

use crate::type_info::{TypeInfo, FieldInfo as TypeFieldInfo};
use crate::Result;
use std::collections::HashSet;

/// 値フォーマッター
///
/// メモリアドレスと型情報から値を読み取り、フォーマットします。
pub struct ValueFormatter<'a> {
    memory: &'a dyn MemoryReader,
}

/// 型名から基本型を判定
#[derive(Debug, Clone, PartialEq)]
pub enum BasicType {
    /// &str
    Str,
    /// String
    String,
    /// Vec<T>
    Vec { element_type: String },
    /// Option<T>
    Option { inner_type: String },
    /// Result<T, E>
    Result { ok_type: String, err_type: String },
    /// その他
    Other,
}

impl BasicType {
    /// 型名から基本型を判定する
    pub fn from_type_name(type_name: &str) -> Self {
        // &str
        if type_name == "&str" || type_name.contains("&str") {
            return BasicType::Str;
        }

        // String
        if type_name == "alloc::string::String"
            || type_name == "String"
            || type_name.contains("::string::String") {
            return BasicType::String;
        }

        // Vec<T>
        if let Some(element_type) = Self::extract_vec_element_type(type_name) {
            return BasicType::Vec { element_type };
        }

        // Option<T>
        if let Some(inner_type) = Self::extract_option_inner_type(type_name) {
            return BasicType::Option { inner_type };
        }

        // Result<T, E>
        if let Some((ok_type, err_type)) = Self::extract_result_types(type_name) {
            return BasicType::Result { ok_type, err_type };
        }

        BasicType::Other
    }

    /// Vec<T>の要素型を抽出
    fn extract_vec_element_type(type_name: &str) -> Option<String> {
        // "alloc::vec::Vec<i32>" -> "i32"
        // "Vec<i32>" -> "i32"
        if type_name.contains("::vec::Vec<") || type_name.starts_with("Vec<") {
            let start = type_name.find('<')? + 1;
            let end = type_name.rfind('>')?;
            return Some(type_name[start..end].trim().to_string());
        }
        None
    }

    /// Option<T>の内部型を抽出
    fn extract_option_inner_type(type_name: &str) -> Option<String> {
        // "core::option::Option<i32>" -> "i32"
        // "Option<i32>" -> "i32"
        if type_name.contains("::option::Option<") || type_name.starts_with("Option<") {
            let start = type_name.find('<')? + 1;
            let end = type_name.rfind('>')?;
            return Some(type_name[start..end].trim().to_string());
        }
        None
    }

    /// Result<T, E>のOk型とErr型を抽出
    fn extract_result_types(type_name: &str) -> Option<(String, String)> {
        // "core::result::Result<i32, String>" -> ("i32", "String")
        // "Result<i32, String>" -> ("i32", "String")
        if type_name.contains("::result::Result<") || type_name.starts_with("Result<") {
            let start = type_name.find('<')? + 1;
            let end = type_name.rfind('>')?;
            let inner = type_name[start..end].trim();

            // カンマで分割（ネストした<>を考慮）
            let comma_pos = Self::find_top_level_comma(inner)?;
            let ok_type = inner[..comma_pos].trim().to_string();
            let err_type = inner[comma_pos + 1..].trim().to_string();

            return Some((ok_type, err_type));
        }
        None
    }

    /// トップレベルのカンマを見つける（ネストした<>を考慮）
    fn find_top_level_comma(s: &str) -> Option<usize> {
        let mut depth = 0;
        for (i, ch) in s.chars().enumerate() {
            match ch {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => return Some(i),
                _ => {}
            }
        }
        None
    }
}

/// メモリ読み取りトレイト
///
/// デバッガのメモリインターフェースを抽象化します。
pub trait MemoryReader {
    fn read_u8(&self, addr: usize) -> Result<u8>;
    fn read_u16(&self, addr: usize) -> Result<u16>;
    fn read_u32(&self, addr: usize) -> Result<u32>;
    fn read_u64(&self, addr: usize) -> Result<u64>;
    fn read(&self, addr: usize, size: usize) -> Result<Vec<u8>>;
}

/// フォーマット制御オプション
#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// インデントレベル
    pub indent: usize,
    /// 最大再帰深さ
    pub max_depth: usize,
    /// 訪問済みアドレス（循環参照検出用）
    pub visited: HashSet<u64>,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent: 0,
            max_depth: 10,
            visited: HashSet::new(),
        }
    }
}

impl<'a> ValueFormatter<'a> {
    /// 新しいフォーマッターを作成する
    pub fn new(memory: &'a dyn MemoryReader) -> Self {
        Self { memory }
    }

    /// TypeInfo を使って値をフォーマットする（高度版）
    pub fn format_with_type_info(
        &self,
        address: u64,
        type_info: &TypeInfo,
        mut options: FormatOptions,
    ) -> Result<String> {
        // 再帰深さチェック
        if options.indent / 2 >= options.max_depth {
            return Ok("<max depth reached>".to_string());
        }

        // 循環参照チェック
        if options.visited.contains(&address) {
            return Ok("<circular reference>".to_string());
        }

        options.visited.insert(address);

        match type_info {
            TypeInfo::Primitive { name, .. } => {
                // プリミティブ型は型名から判断
                self.format_by_type(address, name)
            }
            TypeInfo::Pointer { pointee_type, .. } => {
                // ポインタの値（アドレス）を表示
                let ptr_value = self.memory.read_u64(address as usize)?;
                if let Some(pointee) = pointee_type {
                    Ok(format!("0x{:x} -> {}", ptr_value, self.type_name(pointee)))
                } else {
                    Ok(format!("0x{:x}", ptr_value))
                }
            }
            TypeInfo::Reference { referent_type, .. } => {
                // 参照の先を読み取る
                let ref_addr = self.memory.read_u64(address as usize)?;
                if let Some(referent) = referent_type {
                    self.format_with_type_info(ref_addr, referent, options)
                } else {
                    Ok(format!("&0x{:x}", ref_addr))
                }
            }
            TypeInfo::Struct { name, fields, .. } => {
                self.format_struct(address, name, fields, options)
            }
            TypeInfo::Enum { name, variants, .. } => {
                // Enumは簡易フォーマット
                Ok(format!("<{} enum with {} variants>", name, variants.len()))
            }
            TypeInfo::Array { element_type, length } => {
                if let Some(elem_type) = element_type {
                    self.format_array(address, elem_type, *length, options)
                } else {
                    Ok("[<unknown array>]".to_string())
                }
            }
            TypeInfo::Union { name, .. } => {
                Ok(format!("<{} union>", name))
            }
            TypeInfo::Unknown => Ok("<unknown type>".to_string()),
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

    /// 構造体をフォーマットする
    fn format_struct(
        &self,
        address: u64,
        name: &str,
        fields: &[TypeFieldInfo],
        options: FormatOptions,
    ) -> Result<String> {
        if fields.is_empty() {
            return Ok(format!("{} {{}}", name));
        }

        let indent_str = " ".repeat(options.indent);
        let mut result = format!("{} {{\n", name);

        for field in fields {
            let field_addr = address + field.offset;

            // フィールドの値をフォーマット
            let field_value = if let Some(ref type_info) = field.type_info {
                let mut field_options = options.clone();
                field_options.indent += 2;
                self.format_with_type_info(field_addr, type_info, field_options)
                    .unwrap_or_else(|_| "<error>".to_string())
            } else {
                format!("<no type info>")
            };

            result.push_str(&format!("{}  {}: {},\n", indent_str, field.name, field_value));
        }

        result.push_str(&format!("{}}}", indent_str));
        Ok(result)
    }

    /// 配列をフォーマットする
    fn format_array(
        &self,
        address: u64,
        element_type: &TypeInfo,
        length: Option<u64>,
        options: FormatOptions,
    ) -> Result<String> {
        let len = length.unwrap_or(0) as usize;
        const MAX_ELEMENTS: usize = 20;
        let display_len = len.min(MAX_ELEMENTS);

        // 要素のサイズを取得
        let element_size = self.get_type_size(element_type);
        if element_size == 0 {
            return Ok(format!("[<unknown element size>, len: {}]", len));
        }

        let mut elements = Vec::new();
        for i in 0..display_len {
            let elem_addr = address + (i as u64 * element_size);
            let elem_value = self.format_with_type_info(elem_addr, element_type, options.clone())
                .unwrap_or_else(|_| "<error>".to_string());
            elements.push(elem_value);
        }

        let elements_str = elements.join(", ");
        if len > MAX_ELEMENTS {
            Ok(format!("[{}, ...] (len: {})", elements_str, len))
        } else {
            Ok(format!("[{}]", elements_str))
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

    /// 型名に基づいて値をフォーマットする
    pub fn format_by_type(&self, address: u64, type_name: &str) -> Result<String> {
        let basic_type = BasicType::from_type_name(type_name);

        match basic_type {
            BasicType::Str => self.format_str(address),
            BasicType::String => self.format_string(address),
            BasicType::Vec { element_type } => {
                // 要素型のサイズを推定
                let element_size = self.estimate_element_size(&element_type);
                self.format_vec_primitive(address, element_size)
            }
            BasicType::Option { .. } => {
                // 簡易版（値のサイズは8と仮定）
                self.format_option_simple(address, 8)
            }
            BasicType::Result { .. } => {
                // 簡易版（Ok/Errのサイズは8と仮定）
                self.format_result_simple(address, 8, 8)
            }
            BasicType::Other => {
                // その他の型は生バイトで表示
                Ok(format!("<{}>", type_name))
            }
        }
    }

    /// 要素型のサイズを推定する
    fn estimate_element_size(&self, element_type: &str) -> usize {
        match element_type {
            "u8" | "i8" | "bool" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" | "f32" => 4,
            "u64" | "i64" | "f64" | "usize" | "isize" => 8,
            _ => 8, // デフォルトは8バイト
        }
    }

    /// &str をフォーマットする
    ///
    /// &str の内部表現: { ptr: *const u8, len: usize }
    /// 構造体の先頭8バイトがptr、次の8バイトがlen
    pub fn format_str(&self, address: u64) -> Result<String> {
        // ptr (8 bytes)
        let ptr = self.memory.read_u64(address as usize)?;

        // len (8 bytes)
        let len = self.memory.read_u64((address + 8) as usize)? as usize;

        // 最大長を制限（安全性のため）
        const MAX_STR_LEN: usize = 1024;
        let actual_len = len.min(MAX_STR_LEN);

        // 文字列データを読み取る
        let bytes = self.memory.read(ptr as usize, actual_len)?;

        // UTF-8としてデコード
        match std::str::from_utf8(&bytes) {
            Ok(s) => {
                if len > MAX_STR_LEN {
                    Ok(format!("\"{}...\" (truncated, actual len: {})", s, len))
                } else {
                    Ok(format!("\"{}\"", s))
                }
            }
            Err(_) => Ok(format!("<invalid UTF-8, {} bytes>", len)),
        }
    }

    /// String をフォーマットする
    ///
    /// String の内部表現: { ptr: *const u8, len: usize, capacity: usize }
    pub fn format_string(&self, address: u64) -> Result<String> {
        // ptr (8 bytes)
        let ptr = self.memory.read_u64(address as usize)?;

        // len (8 bytes)
        let len = self.memory.read_u64((address + 8) as usize)? as usize;

        // capacity (8 bytes)
        let capacity = self.memory.read_u64((address + 16) as usize)? as usize;

        // 最大長を制限（安全性のため）
        const MAX_STR_LEN: usize = 1024;
        let actual_len = len.min(MAX_STR_LEN);

        // 文字列データを読み取る
        let bytes = self.memory.read(ptr as usize, actual_len)?;

        // UTF-8としてデコード
        match std::str::from_utf8(&bytes) {
            Ok(s) => {
                if len > MAX_STR_LEN {
                    Ok(format!("\"{}...\" (len: {}, cap: {})", s, len, capacity))
                } else {
                    Ok(format!("\"{}\" (cap: {})", s, capacity))
                }
            }
            Err(_) => Ok(format!("<invalid UTF-8, len: {}, cap: {}>", len, capacity)),
        }
    }

    /// Vec<T> をフォーマットする
    ///
    /// Vec の内部表現: { ptr: *mut T, len: usize, capacity: usize }
    pub fn format_vec_primitive(&self, address: u64, element_size: usize) -> Result<String> {
        // ptr (8 bytes)
        let ptr = self.memory.read_u64(address as usize)?;

        // len (8 bytes)
        let len = self.memory.read_u64((address + 8) as usize)? as usize;

        // capacity (8 bytes)
        let capacity = self.memory.read_u64((address + 16) as usize)? as usize;

        // 要素数を制限（安全性のため）
        const MAX_ELEMENTS: usize = 20;
        let display_len = len.min(MAX_ELEMENTS);

        let mut elements = Vec::new();
        for i in 0..display_len {
            let elem_addr = ptr + (i * element_size) as u64;

            // 要素のサイズに応じて読み取り
            let value = match element_size {
                1 => self.memory.read_u8(elem_addr as usize)? as u64,
                2 => self.memory.read_u16(elem_addr as usize)? as u64,
                4 => self.memory.read_u32(elem_addr as usize)? as u64,
                8 => self.memory.read_u64(elem_addr as usize)?,
                _ => return Ok(format!("[...] (len: {}, cap: {})", len, capacity)),
            };

            elements.push(value.to_string());
        }

        let elements_str = elements.join(", ");
        if len > MAX_ELEMENTS {
            Ok(format!("[{}, ...] (len: {}, cap: {})", elements_str, len, capacity))
        } else {
            Ok(format!("[{}] (len: {}, cap: {})", elements_str, len, capacity))
        }
    }

    /// Option<T> をフォーマットする（簡易版）
    ///
    /// Option のレイアウト:
    /// - None: discriminant = 0
    /// - Some(v): discriminant = 1, value follows
    pub fn format_option_simple(&self, address: u64, _value_size: usize) -> Result<String> {
        // discriminant (通常1バイトまたは4バイト)
        let discriminant = self.memory.read_u32(address as usize)?;

        if discriminant == 0 {
            Ok("None".to_string())
        } else {
            // 値の詳細フォーマットは今後の拡張で実装
            Ok("Some(<value>)".to_string())
        }
    }

    /// Result<T, E> をフォーマットする（簡易版）
    ///
    /// Result のレイアウト:
    /// - Ok(v): discriminant = 0
    /// - Err(e): discriminant = 1
    pub fn format_result_simple(&self, address: u64, _ok_size: usize, _err_size: usize) -> Result<String> {
        // discriminant
        let discriminant = self.memory.read_u32(address as usize)?;

        if discriminant == 0 {
            Ok("Ok(<value>)".to_string())
        } else {
            Ok("Err(<error>)".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMemory {
        data: Vec<u8>,
    }

    impl MemoryReader for MockMemory {
        fn read_u8(&self, addr: usize) -> Result<u8> {
            Ok(self.data[addr])
        }

        fn read_u16(&self, addr: usize) -> Result<u16> {
            let bytes = &self.data[addr..addr+2];
            Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
        }

        fn read_u32(&self, addr: usize) -> Result<u32> {
            let bytes = &self.data[addr..addr+4];
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }

        fn read_u64(&self, addr: usize) -> Result<u64> {
            let bytes = &self.data[addr..addr+8];
            Ok(u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6], bytes[7],
            ]))
        }

        fn read(&self, addr: usize, size: usize) -> Result<Vec<u8>> {
            Ok(self.data[addr..addr+size].to_vec())
        }
    }

    #[test]
    fn test_format_str() {
        // メモリレイアウト:
        // 0x0: &str { ptr: 0x20, len: 5 }
        // 0x20: "Hello"
        let mut data = vec![0u8; 256];

        // &str at address 0
        data[0..8].copy_from_slice(&0x20u64.to_le_bytes());  // ptr
        data[8..16].copy_from_slice(&5u64.to_le_bytes());    // len

        // String data at address 0x20
        data[0x20..0x25].copy_from_slice(b"Hello");

        let memory = MockMemory { data };
        let formatter = ValueFormatter::new(&memory);

        let result = formatter.format_str(0).unwrap();
        assert_eq!(result, "\"Hello\"");
    }
}

//! 値デコード機能
//!
//! メモリから読み取ったバイト列を、型情報に基づいて適切にフォーマットします。

/// デコード設定
#[derive(Debug, Clone)]
pub struct DecodeConfig {
    /// 最大深さ（再帰制限）
    pub max_depth: usize,
    /// 配列/Vecの最大表示要素数
    pub max_array_elements: usize,
    /// 文字列の最大表示バイト数
    pub max_string_bytes: usize,
    /// バイト列の最大表示バイト数
    pub max_bytes_display: usize,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_array_elements: 16,
            max_string_bytes: 256,
            max_bytes_display: 64,
        }
    }
}

/// デコード結果の表示値
#[derive(Debug, Clone)]
pub enum DisplayValue {
    /// 整数
    Int(i64),
    /// 符号なし整数
    Uint(u64),
    /// 浮動小数点
    Float(f64),
    /// 真偽値
    Bool(bool),
    /// 文字
    Char(char),
    /// 文字列
    Str(String, bool), // (値, truncated)
    /// バイト列
    Bytes(Vec<u8>, bool), // (値, truncated)
    /// ポインタ
    Ptr(u64),
    /// 配列
    Array(Vec<DisplayValue>, bool), // (要素, truncated)
    /// 構造体
    Struct {
        name: String,
        fields: Vec<(String, DisplayValue)>,
    },
    /// 列挙型
    Enum {
        name: String,
        variant: String,
        fields: Vec<(String, DisplayValue)>,
    },
    /// 利用不可
    Unavailable,
    /// オプション
    Option(Option<Box<DisplayValue>>),
    /// Result
    Result {
        is_ok: bool,
        value: Box<DisplayValue>,
    },
}

impl std::fmt::Display for DisplayValue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DisplayValue::Int(v) => write!(f, "{}", v),
            DisplayValue::Uint(v) => write!(f, "{}", v),
            DisplayValue::Float(v) => write!(f, "{}", v),
            DisplayValue::Bool(v) => write!(f, "{}", v),
            DisplayValue::Char(c) => write!(f, "'{}'", c),
            DisplayValue::Str(s, truncated) => {
                write!(f, "\"{}\"", s)?;
                if *truncated {
                    write!(f, "...")?;
                }
                Ok(())
            }
            DisplayValue::Bytes(bytes, truncated) => {
                write!(f, "[")?;
                for (i, b) in bytes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:#04x}", b)?;
                }
                if *truncated {
                    write!(f, ", ...")?;
                }
                write!(f, "]")
            }
            DisplayValue::Ptr(addr) => write!(f, "0x{:x}", addr),
            DisplayValue::Array(elements, truncated) => {
                write!(f, "[")?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                if *truncated {
                    write!(f, ", ...")?;
                }
                write!(f, "]")
            }
            DisplayValue::Struct { name, fields } => {
                write!(f, "{} {{ ", name)?;
                for (i, (field_name, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", field_name, value)?;
                }
                write!(f, " }}")
            }
            DisplayValue::Enum {
                name,
                variant,
                fields,
            } => {
                write!(f, "{}::{}", name, variant)?;
                if !fields.is_empty() {
                    write!(f, "(")?;
                    for (i, (field_name, value)) in fields.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        if !field_name.is_empty() {
                            write!(f, "{}: ", field_name)?;
                        }
                        write!(f, "{}", value)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            DisplayValue::Unavailable => write!(f, "<unavailable>"),
            DisplayValue::Option(opt) => match opt {
                Some(val) => write!(f, "Some({})", val),
                None => write!(f, "None"),
            },
            DisplayValue::Result { is_ok, value } => {
                if *is_ok {
                    write!(f, "Ok({})", value)
                } else {
                    write!(f, "Err({})", value)
                }
            }
        }
    }
}

/// 値デコーダー
pub struct ValueDecoder {
    config: DecodeConfig,
}

impl ValueDecoder {
    /// 新しい値デコーダーを作成する
    pub fn new(config: DecodeConfig) -> Self {
        Self { config }
    }

    /// デフォルト設定で値デコーダーを作成する
    pub fn default() -> Self {
        Self {
            config: DecodeConfig::default(),
        }
    }

    /// プリミティブ型をデコードする
    pub fn decode_primitive(&self, bytes: &[u8], type_name: &str) -> DisplayValue {
        match type_name {
            "i8" => {
                if bytes.len() >= 1 {
                    DisplayValue::Int(i8::from_le_bytes([bytes[0]]) as i64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "i16" => {
                if bytes.len() >= 2 {
                    let arr: [u8; 2] = bytes[..2].try_into().unwrap();
                    DisplayValue::Int(i16::from_le_bytes(arr) as i64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "i32" => {
                if bytes.len() >= 4 {
                    let arr: [u8; 4] = bytes[..4].try_into().unwrap();
                    DisplayValue::Int(i32::from_le_bytes(arr) as i64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "i64" | "isize" => {
                if bytes.len() >= 8 {
                    let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                    DisplayValue::Int(i64::from_le_bytes(arr))
                } else {
                    DisplayValue::Unavailable
                }
            }
            "u8" => {
                if bytes.len() >= 1 {
                    DisplayValue::Uint(bytes[0] as u64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "u16" => {
                if bytes.len() >= 2 {
                    let arr: [u8; 2] = bytes[..2].try_into().unwrap();
                    DisplayValue::Uint(u16::from_le_bytes(arr) as u64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "u32" => {
                if bytes.len() >= 4 {
                    let arr: [u8; 4] = bytes[..4].try_into().unwrap();
                    DisplayValue::Uint(u32::from_le_bytes(arr) as u64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "u64" | "usize" => {
                if bytes.len() >= 8 {
                    let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                    DisplayValue::Uint(u64::from_le_bytes(arr))
                } else {
                    DisplayValue::Unavailable
                }
            }
            "f32" => {
                if bytes.len() >= 4 {
                    let arr: [u8; 4] = bytes[..4].try_into().unwrap();
                    DisplayValue::Float(f32::from_le_bytes(arr) as f64)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "f64" => {
                if bytes.len() >= 8 {
                    let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                    DisplayValue::Float(f64::from_le_bytes(arr))
                } else {
                    DisplayValue::Unavailable
                }
            }
            "bool" => {
                if bytes.len() >= 1 {
                    DisplayValue::Bool(bytes[0] != 0)
                } else {
                    DisplayValue::Unavailable
                }
            }
            "char" => {
                if bytes.len() >= 4 {
                    let arr: [u8; 4] = bytes[..4].try_into().unwrap();
                    let code = u32::from_le_bytes(arr);
                    if let Some(c) = char::from_u32(code) {
                        DisplayValue::Char(c)
                    } else {
                        DisplayValue::Unavailable
                    }
                } else {
                    DisplayValue::Unavailable
                }
            }
            _ => DisplayValue::Unavailable,
        }
    }

    /// バイト列を文字列としてデコードする
    pub fn decode_str(&self, bytes: &[u8]) -> DisplayValue {
        let limit = bytes.len().min(self.config.max_string_bytes);
        let truncated = bytes.len() > self.config.max_string_bytes;

        match std::str::from_utf8(&bytes[..limit]) {
            Ok(s) => DisplayValue::Str(s.to_string(), truncated),
            Err(_) => {
                // UTF-8として無効な場合はバイト列として表示
                self.decode_bytes(bytes)
            }
        }
    }

    /// バイト列をデコードする
    pub fn decode_bytes(&self, bytes: &[u8]) -> DisplayValue {
        let limit = bytes.len().min(self.config.max_bytes_display);
        let truncated = bytes.len() > self.config.max_bytes_display;
        DisplayValue::Bytes(bytes[..limit].to_vec(), truncated)
    }

    /// ポインタをデコードする（アドレスのみ）
    pub fn decode_pointer(&self, bytes: &[u8]) -> DisplayValue {
        if bytes.len() >= 8 {
            let arr: [u8; 8] = bytes[..8].try_into().unwrap();
            let addr = u64::from_le_bytes(arr);
            DisplayValue::Ptr(addr)
        } else {
            DisplayValue::Unavailable
        }
    }

    /// Vecをデコードする（{ptr, len, cap}構造）
    ///
    /// # Arguments
    /// * `bytes` - Vecの構造体バイト列（ptr, len, cap）
    /// * `read_mem` - メモリ読み取りコールバック
    pub fn decode_vec<F>(&self, bytes: &[u8], mut read_mem: F) -> DisplayValue
    where
        F: FnMut(u64, usize) -> Result<Vec<u8>, String>,
    {
        if bytes.len() < 24 {
            // Vec<T>は{ptr: *const T, len: usize, cap: usize}で24バイト（64bit）
            return DisplayValue::Unavailable;
        }

        // ptrを読み取る
        let ptr_bytes: [u8; 8] = bytes[0..8].try_into().unwrap();
        let ptr = u64::from_le_bytes(ptr_bytes);

        // lenを読み取る
        let len_bytes: [u8; 8] = bytes[8..16].try_into().unwrap();
        let len = u64::from_le_bytes(len_bytes) as usize;

        // capを読み取る（現時点では使用しない）
        // let cap_bytes: [u8; 8] = bytes[16..24].try_into().unwrap();
        // let cap = u64::from_le_bytes(cap_bytes) as usize;

        // ptrがNULLの場合は空のVec
        if ptr == 0 {
            return DisplayValue::Array(Vec::new(), false);
        }

        // 要素数が多すぎる場合は制限
        let display_len = len.min(self.config.max_array_elements);
        let truncated = len > self.config.max_array_elements;

        // 要素の型サイズは仮定：ここでは1バイトと仮定（後で改良）
        // 実際にはDWARF型情報から取得する必要がある
        let elem_size = 1;

        // メモリから要素を読み取る
        match read_mem(ptr, display_len * elem_size) {
            Ok(elem_bytes) => {
                // バイト列として表示（後で型情報を使って改良）
                DisplayValue::Bytes(elem_bytes, truncated)
            }
            Err(_) => DisplayValue::Unavailable,
        }
    }

    /// Boxをデコードする（ポインタのみ）
    ///
    /// # Arguments
    /// * `bytes` - Boxのポインタバイト列
    /// * `_read_mem` - メモリ読み取りコールバック（将来の拡張用）
    pub fn decode_box<F>(&self, bytes: &[u8], _read_mem: F) -> DisplayValue
    where
        F: FnMut(u64, usize) -> Result<Vec<u8>, String>,
    {
        if bytes.len() < 8 {
            return DisplayValue::Unavailable;
        }

        // ポインタを読み取る
        let ptr_bytes: [u8; 8] = bytes[..8].try_into().unwrap();
        let ptr = u64::from_le_bytes(ptr_bytes);

        if ptr == 0 {
            return DisplayValue::Unavailable;
        }

        // 実際には内部の型情報が必要だが、ここではアドレスのみ表示
        DisplayValue::Ptr(ptr)
    }

    /// Optionをデコードする（discriminant + value）
    ///
    /// # Arguments
    /// * `bytes` - Optionのバイト列（discriminant + value）
    pub fn decode_option(&self, bytes: &[u8]) -> DisplayValue {
        if bytes.is_empty() {
            return DisplayValue::Unavailable;
        }

        // 最初のバイトがdiscriminant（0=None, 1=Some）
        let discriminant = bytes[0];

        if discriminant == 0 {
            DisplayValue::Option(None)
        } else if discriminant == 1 && bytes.len() > 1 {
            // Someの値をデコード（簡易版：u64として扱う）
            if bytes.len() >= 9 {
                let value_bytes: [u8; 8] = bytes[1..9].try_into().unwrap();
                let value = u64::from_le_bytes(value_bytes);
                DisplayValue::Option(Some(Box::new(DisplayValue::Uint(value))))
            } else {
                DisplayValue::Option(Some(Box::new(DisplayValue::Unavailable)))
            }
        } else {
            DisplayValue::Unavailable
        }
    }

    /// Resultをデコードする（discriminant + value）
    ///
    /// # Arguments
    /// * `bytes` - Resultのバイト列（discriminant + value）
    pub fn decode_result(&self, bytes: &[u8]) -> DisplayValue {
        if bytes.is_empty() {
            return DisplayValue::Unavailable;
        }

        // 最初のバイトがdiscriminant（0=Ok, 1=Err）
        let discriminant = bytes[0];

        if bytes.len() > 1 {
            // 値をデコード（簡易版：u64として扱う）
            if bytes.len() >= 9 {
                let value_bytes: [u8; 8] = bytes[1..9].try_into().unwrap();
                let value = u64::from_le_bytes(value_bytes);
                let is_ok = discriminant == 0;
                DisplayValue::Result {
                    is_ok,
                    value: Box::new(DisplayValue::Uint(value)),
                }
            } else {
                let is_ok = discriminant == 0;
                DisplayValue::Result {
                    is_ok,
                    value: Box::new(DisplayValue::Unavailable),
                }
            }
        } else {
            DisplayValue::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_primitive() {
        let decoder = ValueDecoder::default();

        // i32のテスト
        let bytes = 42i32.to_le_bytes();
        let val = decoder.decode_primitive(&bytes, "i32");
        match val {
            DisplayValue::Int(v) => assert_eq!(v, 42),
            _ => panic!("Expected Int"),
        }

        // u64のテスト
        let bytes = 12345u64.to_le_bytes();
        let val = decoder.decode_primitive(&bytes, "u64");
        match val {
            DisplayValue::Uint(v) => assert_eq!(v, 12345),
            _ => panic!("Expected Uint"),
        }

        // boolのテスト
        let val = decoder.decode_primitive(&[1], "bool");
        match val {
            DisplayValue::Bool(v) => assert!(v),
            _ => panic!("Expected Bool"),
        }
    }

    #[test]
    fn test_decode_str() {
        let decoder = ValueDecoder::default();

        let bytes = b"Hello, World!";
        let val = decoder.decode_str(bytes);
        match val {
            DisplayValue::Str(s, truncated) => {
                assert_eq!(s, "Hello, World!");
                assert!(!truncated);
            }
            _ => panic!("Expected Str"),
        }
    }

    #[test]
    fn test_display_value() {
        let val = DisplayValue::Int(42);
        assert_eq!(format!("{}", val), "42");

        let val = DisplayValue::Str("test".to_string(), false);
        assert_eq!(format!("{}", val), "\"test\"");

        let val = DisplayValue::Bool(true);
        assert_eq!(format!("{}", val), "true");
    }

    #[test]
    fn test_decode_pointer() {
        let decoder = ValueDecoder::default();

        let ptr_value = 0x7fff_1234_5678u64;
        let bytes = ptr_value.to_le_bytes();
        let val = decoder.decode_pointer(&bytes);

        match val {
            DisplayValue::Ptr(addr) => assert_eq!(addr, ptr_value),
            _ => panic!("Expected Ptr"),
        }
    }

    #[test]
    fn test_decode_option() {
        let decoder = ValueDecoder::default();

        // None の場合
        let none_bytes = vec![0u8];
        let val = decoder.decode_option(&none_bytes);
        match val {
            DisplayValue::Option(None) => {},
            _ => panic!("Expected Option(None)"),
        }

        // Some(42) の場合
        let mut some_bytes = vec![1u8];
        some_bytes.extend_from_slice(&42u64.to_le_bytes());
        let val = decoder.decode_option(&some_bytes);
        match val {
            DisplayValue::Option(Some(_)) => {},
            _ => panic!("Expected Option(Some)"),
        }
    }

    #[test]
    fn test_decode_result() {
        let decoder = ValueDecoder::default();

        // Ok(42) の場合
        let mut ok_bytes = vec![0u8];
        ok_bytes.extend_from_slice(&42u64.to_le_bytes());
        let val = decoder.decode_result(&ok_bytes);
        match val {
            DisplayValue::Result { is_ok: true, .. } => {},
            _ => panic!("Expected Result::Ok"),
        }

        // Err(1) の場合
        let mut err_bytes = vec![1u8];
        err_bytes.extend_from_slice(&1u64.to_le_bytes());
        let val = decoder.decode_result(&err_bytes);
        match val {
            DisplayValue::Result { is_ok: false, .. } => {},
            _ => panic!("Expected Result::Err"),
        }
    }

    #[test]
    fn test_decode_vec() {
        let decoder = ValueDecoder::default();

        // 空のVec
        let mut vec_bytes = vec![0u8; 24]; // ptr=0, len=0, cap=0
        let val = decoder.decode_vec(&vec_bytes, |_, _| Ok(vec![]));
        match val {
            DisplayValue::Array(elements, _) => assert!(elements.is_empty()),
            _ => panic!("Expected Array"),
        }

        // 要素を持つVec（メモリ読み取り成功）
        let ptr = 0x1000u64;
        let len = 3usize;
        let cap = 4usize;

        vec_bytes = Vec::new();
        vec_bytes.extend_from_slice(&ptr.to_le_bytes());
        vec_bytes.extend_from_slice(&len.to_le_bytes());
        vec_bytes.extend_from_slice(&cap.to_le_bytes());

        let val = decoder.decode_vec(&vec_bytes, |addr, size| {
            assert_eq!(addr, ptr);
            assert_eq!(size, 3); // len * elem_size (1)
            Ok(vec![1, 2, 3])
        });

        match val {
            DisplayValue::Bytes(bytes, _) => assert_eq!(bytes, vec![1, 2, 3]),
            _ => panic!("Expected Bytes"),
        }
    }

    #[test]
    fn test_decode_box() {
        let decoder = ValueDecoder::default();

        // NULL ポインタ
        let null_bytes = 0u64.to_le_bytes();
        let val = decoder.decode_box(&null_bytes, |_, _| Ok(vec![]));
        match val {
            DisplayValue::Unavailable => {},
            _ => panic!("Expected Unavailable"),
        }

        // 有効なポインタ
        let ptr = 0x2000u64;
        let ptr_bytes = ptr.to_le_bytes();
        let val = decoder.decode_box(&ptr_bytes, |_, _| Ok(vec![]));
        match val {
            DisplayValue::Ptr(addr) => assert_eq!(addr, ptr),
            _ => panic!("Expected Ptr"),
        }
    }
}

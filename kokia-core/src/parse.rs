//! パース関連のユーティリティ関数

use anyhow::Result;

/// アドレス文字列をu64にパース
///
/// 16進数（0xプレフィックス付き）または10進数をサポート
///
/// # Examples
/// ```
/// use kokia_core::parse::parse_address;
///
/// assert_eq!(parse_address("0x1234").unwrap(), 0x1234);
/// assert_eq!(parse_address("1234").unwrap(), 1234);
/// ```
pub fn parse_address(s: &str) -> Result<u64> {
    let s = s.trim();

    if s.starts_with("0x") || s.starts_with("0X") {
        // 16進数
        u64::from_str_radix(&s[2..], 16)
            .map_err(|e| anyhow::anyhow!("Invalid hexadecimal address '{}': {}", s, e))
    } else {
        // 10進数を試す
        s.parse::<u64>()
            .or_else(|_| {
                // 10進数でもダメなら16進数として解釈を試みる
                u64::from_str_radix(s, 16)
            })
            .map_err(|e| anyhow::anyhow!("Invalid address '{}': {}", s, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_address_hex() {
        assert_eq!(parse_address("0x1234").unwrap(), 0x1234);
        assert_eq!(parse_address("0X1234").unwrap(), 0x1234);
        assert_eq!(parse_address("0xabcd").unwrap(), 0xabcd);
        assert_eq!(parse_address("0xABCD").unwrap(), 0xabcd);
    }

    #[test]
    fn test_parse_address_dec() {
        assert_eq!(parse_address("1234").unwrap(), 1234);
        assert_eq!(parse_address("9999").unwrap(), 9999);
    }

    #[test]
    fn test_parse_address_invalid() {
        assert!(parse_address("xyz").is_err());
        assert!(parse_address("0xghij").is_err());
    }
}

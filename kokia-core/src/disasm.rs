//! 逆アセンブル機能
//!
//! 関数のバイト列を逆アセンブルしてret命令のアドレスを検出します。

use crate::Result;
use capstone::prelude::*;

/// 関数内のret命令のアドレスを検出する
///
/// # Arguments
/// * `code` - 関数のバイト列
/// * `base_addr` - 関数の開始アドレス
///
/// # Returns
/// ret命令の絶対アドレスのリスト
pub fn find_ret_instructions(code: &[u8], base_addr: u64) -> Result<Vec<u64>> {
    let cs = Capstone::new()
        .x86()
        .mode(arch::x86::ArchMode::Mode64)
        .syntax(arch::x86::ArchSyntax::Intel)
        .detail(true)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create Capstone: {}", e))?;

    let insns = cs
        .disasm_all(code, base_addr)
        .map_err(|e| anyhow::anyhow!("Failed to disassemble: {}", e))?;

    let mut ret_addresses = Vec::new();

    for insn in insns.as_ref() {
        // ret命令を検出
        // mnemonic が "ret" または "retq"
        let mnemonic = insn.mnemonic().unwrap_or("");
        if mnemonic == "ret" || mnemonic == "retq" {
            ret_addresses.push(insn.address());
        }
    }

    Ok(ret_addresses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_ret_simple() {
        // 簡単な関数: mov rax, 1; ret
        let code = vec![
            0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
            0xc3, // ret
        ];
        let rets = find_ret_instructions(&code, 0x1000).unwrap();
        assert_eq!(rets.len(), 1);
        assert_eq!(rets[0], 0x1007);
    }
}

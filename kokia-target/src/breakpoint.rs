//! ブレークポイント機能

use crate::Result;

/// INT3命令のオペコード
const INT3_OPCODE: u8 = 0xCC;

/// ソフトウェアブレークポイント（INT3命令）
pub struct SoftwareBreakpoint {
    address: u64,
    original_byte: u8,
    enabled: bool,
}

impl SoftwareBreakpoint {
    /// ブレークポイントを作成する
    pub fn new(address: u64) -> Self {
        Self {
            address,
            original_byte: 0,
            enabled: false,
        }
    }

    /// ブレークポイントのアドレスを取得する
    pub fn address(&self) -> u64 {
        self.address
    }

    /// ブレークポイントが有効かどうか
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 元のバイトを取得する
    pub fn original_byte(&self) -> u8 {
        self.original_byte
    }

    /// ブレークポイントを設定する
    ///
    /// 指定されたアドレスの命令を0xCC（INT3）で置き換えます。
    pub fn enable(&mut self, memory: &crate::Memory) -> Result<()> {
        if self.enabled {
            return Ok(());
        }

        // 元のバイトを保存
        self.original_byte = memory.read_u8(self.address as usize)?;

        // INT3命令で置き換え
        memory.write_u8(self.address as usize, INT3_OPCODE)?;

        self.enabled = true;
        Ok(())
    }

    /// ブレークポイントを解除する
    ///
    /// INT3命令を元のバイトで置き換えます。
    pub fn disable(&mut self, memory: &crate::Memory) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // 元のバイトで置き換え
        memory.write_u8(self.address as usize, self.original_byte)?;

        self.enabled = false;
        Ok(())
    }
}

/// ハードウェアブレークポイント
pub struct HardwareBreakpoint {
    address: u64,
    index: usize,
}

impl HardwareBreakpoint {
    /// ハードウェアブレークポイントを作成する
    pub fn new(address: u64, index: usize) -> Self {
        Self { address, index }
    }

    /// ブレークポイントのアドレスを取得する
    pub fn address(&self) -> u64 {
        self.address
    }

    /// デバッグレジスタのインデックスを取得する
    pub fn index(&self) -> usize {
        self.index
    }
}

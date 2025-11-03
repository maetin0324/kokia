//! ブレークポイント管理

use crate::Result;
use kokia_target::{Memory, SoftwareBreakpoint};
use std::collections::HashMap;

/// ブレークポイントID
pub type BreakpointId = usize;

/// ブレークポイント
#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: BreakpointId,
    pub address: u64,
    pub enabled: bool,
}

/// ブレークポイントマネージャ
///
/// 論理的なブレークポイント情報とソフトウェアブレークポイント（INT3）を
/// 一緒に管理します。
pub struct BreakpointManager {
    breakpoints: HashMap<BreakpointId, (Breakpoint, SoftwareBreakpoint)>,
    next_id: BreakpointId,
}

impl BreakpointManager {
    /// 新しいブレークポイントマネージャを作成する
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            next_id: 1,
        }
    }

    /// ブレークポイントを追加し、有効化する
    pub fn add_and_enable(&mut self, address: u64, memory: &Memory) -> Result<BreakpointId> {
        let id = self.next_id;
        self.next_id += 1;

        let bp = Breakpoint {
            id,
            address,
            enabled: true,
        };

        let mut sw_bp = SoftwareBreakpoint::new(address);
        sw_bp.enable(memory)?;

        self.breakpoints.insert(id, (bp, sw_bp));
        Ok(id)
    }

    /// ブレークポイントを削除し、無効化する
    pub fn remove_and_disable(&mut self, id: BreakpointId, memory: &Memory) -> Result<()> {
        if let Some((_bp, mut sw_bp)) = self.breakpoints.remove(&id) {
            sw_bp.disable(memory)?;
        }
        Ok(())
    }

    /// ブレークポイントを取得する
    pub fn get(&self, id: BreakpointId) -> Option<&Breakpoint> {
        self.breakpoints.get(&id).map(|(bp, _)| bp)
    }

    /// 全てのブレークポイントを取得する
    pub fn all(&self) -> impl Iterator<Item = &Breakpoint> {
        self.breakpoints.values().map(|(bp, _)| bp)
    }

    /// ブレークポイントの数を取得する
    pub fn count(&self) -> usize {
        self.breakpoints.len()
    }
}

impl Default for BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

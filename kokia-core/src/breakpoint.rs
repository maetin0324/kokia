//! ブレークポイント管理

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
pub struct BreakpointManager {
    breakpoints: HashMap<BreakpointId, Breakpoint>,
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

    /// ブレークポイントを追加する
    pub fn add(&mut self, address: u64) -> BreakpointId {
        let id = self.next_id;
        self.next_id += 1;

        let bp = Breakpoint {
            id,
            address,
            enabled: true,
        };
        self.breakpoints.insert(id, bp);
        id
    }

    /// ブレークポイントを取得する
    pub fn get(&self, id: BreakpointId) -> Option<&Breakpoint> {
        self.breakpoints.get(&id)
    }

    /// ブレークポイントを削除する
    pub fn remove(&mut self, id: BreakpointId) -> Option<Breakpoint> {
        self.breakpoints.remove(&id)
    }

    /// 全てのブレークポイントを取得する
    pub fn all(&self) -> impl Iterator<Item = &Breakpoint> {
        self.breakpoints.values()
    }
}

impl Default for BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

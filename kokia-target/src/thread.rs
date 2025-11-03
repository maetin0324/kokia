//! スレッド管理機能

/// スレッドID
pub type ThreadId = i32;

/// デバッグ対象のスレッド
pub struct Thread {
    tid: ThreadId,
}

impl Thread {
    /// スレッドを作成する
    pub fn new(tid: ThreadId) -> Self {
        Self { tid }
    }

    /// スレッドIDを取得する
    pub fn tid(&self) -> ThreadId {
        self.tid
    }
}

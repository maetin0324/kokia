//! 論理スタック（awaitチェーン）構築

use crate::TaskId;

/// 論理スタックフレーム（async関数の呼び出し）
#[derive(Debug, Clone)]
pub struct LogicalFrame {
    pub task_id: TaskId,
    pub function_name: String,
    pub source_location: Option<(String, u32)>,
    pub discriminant: Option<u32>,
}

/// 論理スタック（awaitチェーン）
#[derive(Debug, Clone)]
pub struct LogicalStack {
    frames: Vec<LogicalFrame>,
}

impl LogicalStack {
    /// 新しい論理スタックを作成する
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
        }
    }

    /// フレームを追加する
    pub fn push(&mut self, frame: LogicalFrame) {
        self.frames.push(frame);
    }

    /// フレームを削除する
    pub fn pop(&mut self) -> Option<LogicalFrame> {
        self.frames.pop()
    }

    /// 全てのフレームを取得する
    pub fn frames(&self) -> &[LogicalFrame] {
        &self.frames
    }

    /// スタックが空かどうか
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// スタックの深さ
    pub fn depth(&self) -> usize {
        self.frames.len()
    }
}

impl Default for LogicalStack {
    fn default() -> Self {
        Self::new()
    }
}

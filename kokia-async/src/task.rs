//! タスクトラッキング機能

use std::collections::HashMap;
use crate::LogicalStack;

/// タスクID（Futureのselfポインタ）
pub type TaskId = u64;

/// タスク情報
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: TaskId,
    pub logical_stack: LogicalStack,
}

/// タスクトラッカー
pub struct TaskTracker {
    tasks: HashMap<TaskId, TaskInfo>,
}

impl TaskTracker {
    /// 新しいタスクトラッカーを作成する
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// タスクを登録する
    pub fn register(&mut self, id: TaskId) {
        self.tasks.entry(id).or_insert_with(|| TaskInfo {
            id,
            logical_stack: LogicalStack::new(),
        });
    }

    /// タスクを取得する
    pub fn get(&self, id: TaskId) -> Option<&TaskInfo> {
        self.tasks.get(&id)
    }

    /// タスクを可変参照で取得する
    pub fn get_mut(&mut self, id: TaskId) -> Option<&mut TaskInfo> {
        self.tasks.get_mut(&id)
    }

    /// 全てのタスクを取得する
    pub fn all_tasks(&self) -> impl Iterator<Item = &TaskInfo> {
        self.tasks.values()
    }

    /// タスクを削除する
    pub fn remove(&mut self, id: TaskId) -> Option<TaskInfo> {
        self.tasks.remove(&id)
    }
}

impl Default for TaskTracker {
    fn default() -> Self {
        Self::new()
    }
}

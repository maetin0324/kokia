//! タスクトラッキング機能

use std::collections::HashMap;
use std::time::Instant;
use crate::LogicalStack;

/// スレッドID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tid(pub i32);

/// タスクID（Futureのselfポインタ）
pub type TaskId = u64;

/// EdgeID (parent, child, callsite のハッシュ)
pub type EdgeId = u128;

/// CallsiteID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallsiteId(pub u128);

/// 複数の値からハッシュ値を計算するヘルパーマクロ
macro_rules! compute_hash {
    ($($value:expr),+ $(,)?) => {{
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        $(
            $value.hash(&mut hasher);
        )+
        hasher.finish() as u128
    }};
}

/// タスク情報
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: TaskId,
    pub type_name: Option<String>,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub current_discriminant: Option<u64>,
    pub last_rip: Option<u64>,
    pub is_root: bool,
    pub completed: bool,
    pub logical_stack: LogicalStack,
}

impl TaskInfo {
    /// 新しいタスク情報を作成する
    pub fn new(id: TaskId) -> Self {
        let now = Instant::now();
        Self {
            id,
            type_name: None,
            first_seen: now,
            last_seen: now,
            current_discriminant: None,
            last_rip: None,
            is_root: false,
            completed: false,
            logical_stack: LogicalStack::new(),
        }
    }

    /// 最終観測時刻を更新する
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
}

/// 呼び出しサイト情報
#[derive(Debug, Clone)]
pub struct Callsite {
    /// 親タスクID
    pub parent: TaskId,
    /// ジェネレータの停止点インデックス
    pub suspend_idx: Option<u32>,
    /// ソースファイル名
    pub file: Option<String>,
    /// ソース行番号
    pub line: Option<u32>,
}

impl Callsite {
    /// 新しい呼び出しサイトを作成する
    pub fn new(parent: TaskId) -> Self {
        Self {
            parent,
            suspend_idx: None,
            file: None,
            line: None,
        }
    }

    /// CallsiteID を計算する
    pub fn compute_id(&self) -> CallsiteId {
        CallsiteId(compute_hash!(&self.parent, &self.suspend_idx, &self.file, &self.line))
    }
}

/// エッジ情報（親タスクが子タスクをawaitする関係）
#[derive(Debug, Clone)]
pub struct Edge {
    /// 親タスクID
    pub parent: TaskId,
    /// 子タスクID
    pub child: TaskId,
    /// 呼び出しサイトID
    pub callsite: CallsiteId,
    /// 最初に観測された時刻
    pub first_seen: Instant,
    /// 最後に観測された時刻
    pub last_seen: Instant,
    /// 完了フラグ（Poll::Ready が観測されたらtrue）
    pub completed: bool,
}

impl Edge {
    /// 新しいエッジを作成する
    pub fn new(parent: TaskId, child: TaskId, callsite: CallsiteId) -> Self {
        let now = Instant::now();
        Self {
            parent,
            child,
            callsite,
            first_seen: now,
            last_seen: now,
            completed: false,
        }
    }

    /// EdgeID を計算する
    pub fn compute_id(&self) -> EdgeId {
        compute_hash!(&self.parent, &self.child, &self.callsite.0)
    }

    /// 最終観測時刻を更新する
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
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
    pub fn register(&mut self, task: TaskInfo) {
        self.tasks.entry(task.id).or_insert(task);
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

/// エッジトラッカー
pub struct EdgeTracker {
    edges: HashMap<EdgeId, Edge>,
}

impl EdgeTracker {
    /// 新しいエッジトラッカーを作成する
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    /// エッジを登録または更新する
    pub fn register_or_update(&mut self, parent: TaskId, child: TaskId, callsite: CallsiteId) -> EdgeId {
        let edge = Edge::new(parent, child, callsite);
        let edge_id = edge.compute_id();

        self.edges.entry(edge_id)
            .and_modify(|e| e.touch())
            .or_insert(edge);

        edge_id
    }

    /// エッジを取得する
    pub fn get(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(&id)
    }

    /// エッジを可変参照で取得する
    pub fn get_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.get_mut(&id)
    }

    /// 全てのエッジを取得する
    pub fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    /// 親タスクのエッジを取得する
    pub fn edges_by_parent(&self, parent: TaskId) -> impl Iterator<Item = &Edge> {
        self.edges.values().filter(move |e| e.parent == parent)
    }

    /// 子タスクのエッジを取得する
    pub fn edges_by_child(&self, child: TaskId) -> impl Iterator<Item = &Edge> {
        self.edges.values().filter(move |e| e.child == child)
    }

    /// エッジを完了としてマークする
    pub fn mark_completed(&mut self, edge_id: EdgeId) {
        if let Some(edge) = self.edges.get_mut(&edge_id) {
            edge.completed = true;
        }
    }
}

impl Default for EdgeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// 呼び出しサイトトラッカー
pub struct CallsiteTracker {
    callsites: HashMap<CallsiteId, Callsite>,
}

impl CallsiteTracker {
    /// 新しい呼び出しサイトトラッカーを作成する
    pub fn new() -> Self {
        Self {
            callsites: HashMap::new(),
        }
    }

    /// 呼び出しサイトを登録する
    pub fn register(&mut self, callsite: Callsite) -> CallsiteId {
        let id = callsite.compute_id();
        self.callsites.entry(id).or_insert(callsite);
        id
    }

    /// 呼び出しサイトを取得する
    pub fn get(&self, id: CallsiteId) -> Option<&Callsite> {
        self.callsites.get(&id)
    }

    /// 全ての呼び出しサイトを取得する
    pub fn all_callsites(&self) -> impl Iterator<Item = &Callsite> {
        self.callsites.values()
    }
}

impl Default for CallsiteTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// スレッドローカルのpollスコープスタック
#[derive(Debug, Clone)]
pub struct PollScope {
    /// ネストしたpoll呼び出しのスタック（子→親の順）
    stack: Vec<TaskId>,
}

impl PollScope {
    /// 新しいpollスコープを作成する
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
        }
    }

    /// タスクをスタックにプッシュする
    pub fn push(&mut self, task_id: TaskId) {
        self.stack.push(task_id);
    }

    /// スタックからタスクをポップする
    pub fn pop(&mut self) -> Option<TaskId> {
        self.stack.pop()
    }

    /// スタックの最上位（最も深いネスト）のタスクを取得する
    pub fn top(&self) -> Option<TaskId> {
        self.stack.last().copied()
    }

    /// スタック全体を取得する
    pub fn stack(&self) -> &[TaskId] {
        &self.stack
    }

    /// スタックをクリアする
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// OS スタックから取得したタスクリストで再同期する
    ///
    /// 実際の OS スタックと内部のスコープスタックを同期させます。
    /// 最長共通接頭辞を見つけ、差分を調整します。
    ///
    /// # Arguments
    /// * `actual_stack` - OS スタックから取得した実際のタスクリスト（子→親の順）
    pub fn resync(&mut self, actual_stack: Vec<TaskId>) {
        // 最長共通接頭辞を見つける
        let mut common_len = 0;
        for (i, (&expected, &actual)) in self.stack.iter().zip(actual_stack.iter()).enumerate() {
            if expected == actual {
                common_len = i + 1;
            } else {
                break;
            }
        }

        // 共通部分以降を削除
        self.stack.truncate(common_len);

        // actual_stack の残りを追加
        for &task_id in &actual_stack[common_len..] {
            self.stack.push(task_id);
        }
    }
}

impl Default for PollScope {
    fn default() -> Self {
        Self::new()
    }
}

/// スレッドごとのpollスコープ管理
pub struct ThreadPollScopeManager {
    scopes: HashMap<Tid, PollScope>,
}

impl ThreadPollScopeManager {
    /// 新しいスレッドpollスコープマネージャを作成する
    pub fn new() -> Self {
        Self {
            scopes: HashMap::new(),
        }
    }

    /// 指定スレッドのpollスコープを取得する（なければ作成）
    pub fn get_or_create(&mut self, tid: Tid) -> &mut PollScope {
        self.scopes.entry(tid).or_insert_with(PollScope::new)
    }

    /// 指定スレッドのpollスコープを取得する
    pub fn get(&self, tid: Tid) -> Option<&PollScope> {
        self.scopes.get(&tid)
    }

    /// 指定スレッドのpollスコープを削除する
    pub fn remove(&mut self, tid: Tid) {
        self.scopes.remove(&tid);
    }

    /// すべてのスコープをクリアする
    pub fn clear(&mut self) {
        self.scopes.clear();
    }
}

impl Default for ThreadPollScopeManager {
    fn default() -> Self {
        Self::new()
    }
}

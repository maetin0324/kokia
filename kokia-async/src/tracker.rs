//! Async タスクトラッカー
//!
//! GenFuture::poll の entry/exit イベントを処理し、タスクグラフを構築する

use crate::{
    TaskId, TaskInfo, TaskTracker,
    EdgeTracker, Edge,
    CallsiteTracker, Callsite, CallsiteId,
    ThreadPollScopeManager, Tid,
    GenFutureDetector,
};
use crate::Result;

/// Async タスクトラッカー
pub struct AsyncTracker {
    /// タスクトラッカー
    task_tracker: TaskTracker,
    /// エッジトラッカー
    edge_tracker: EdgeTracker,
    /// 呼び出しサイトトラッカー
    callsite_tracker: CallsiteTracker,
    /// スレッドPollスコープマネージャ
    scope_manager: ThreadPollScopeManager,
    /// GenFuture検出器
    detector: GenFutureDetector,
}

impl AsyncTracker {
    /// 新しいAsyncTrackerを作成する
    pub fn new() -> Result<Self> {
        Ok(Self {
            task_tracker: TaskTracker::new(),
            edge_tracker: EdgeTracker::new(),
            callsite_tracker: CallsiteTracker::new(),
            scope_manager: ThreadPollScopeManager::new(),
            detector: GenFutureDetector::new()?,
        })
    }

    /// GenFuture::poll entry イベントを処理する
    ///
    /// # Arguments
    /// * `tid` - スレッドID
    /// * `child_self` - 子タスクのselfポインタ（RDIレジスタから取得）
    /// * `rip` - 命令ポインタ
    /// * `parent_task` - フレームスキャンで検出された親タスク（Option）
    /// * `discriminant` - 子タスクの discriminant（停止点インデックス）
    /// * `function_name` - タスクの関数名（デマングル済み）
    pub fn on_poll_entry(&mut self, tid: Tid, child_self: u64, rip: u64, parent_task: Option<u64>, discriminant: Option<u64>, function_name: Option<String>) -> Result<()> {
        let child = child_self;

        // 1) 親探索（優先: フレームスキャン → スコープスタック）
        let parent = parent_task.or_else(|| {
            self.scope_manager.get(tid).and_then(|scope| scope.top())
        });

        // 2) タスク登録・属性更新
        if let Some(t) = self.task_tracker.get_mut(child) {
            t.touch();
            t.last_rip = Some(rip);
            if let Some(d) = discriminant {
                t.current_discriminant = Some(d);
            }
            // 関数名が未設定の場合のみ更新
            if t.type_name.is_none() && function_name.is_some() {
                t.type_name = function_name.clone();
            }
        } else {
            let mut task = TaskInfo::new(child);
            task.last_rip = Some(rip);
            task.type_name = function_name;
            if let Some(d) = discriminant {
                task.current_discriminant = Some(d);
            }
            self.task_tracker.register(task);
        }

        // TODO: addr2line でソースコード位置を取得
        let (file, line) = (None, None);

        // 3) エッジ登録（callsite 同定）
        if let Some(parent_id) = parent {
            let parent_discriminant = self.task_tracker.get(parent_id)
                .and_then(|t| t.current_discriminant);

            let callsite = Callsite {
                parent: parent_id,
                suspend_idx: parent_discriminant.map(|d| d as u32),
                file,
                line,
            };

            let callsite_id = self.callsite_tracker.register(callsite);
            self.edge_tracker.register_or_update(parent_id, child, callsite_id);
        } else {
            // 親が見つからない場合はrootタスク
            if let Some(task) = self.task_tracker.get_mut(child) {
                task.is_root = true;
            }
        }

        // 4) 動的スコープ push
        let scope = self.scope_manager.get_or_create(tid);
        scope.push(child);

        // 5) exit ret アドレスに一過性BPを配置
        // TODO: ret ブレークポイントの設定を実装

        Ok(())
    }

    /// GenFuture::poll exit イベントを処理する
    ///
    /// # Arguments
    /// * `tid` - スレッドID
    /// * `_rip` - 命令ポインタ
    /// * `is_ready` - Poll::Ready かどうか（false なら Poll::Pending）
    pub fn on_poll_exit(&mut self, tid: Tid, _rip: u64, is_ready: bool) -> Result<()> {
        // スタックからタスクをポップ
        let child = self.scope_manager.get_or_create(tid).pop();

        if let Some(child_id) = child {

            if is_ready {
                // 親がいれば当該 callsite を completed に
                let parent = self.find_parent_task(tid)?;

                if let Some(parent_id) = parent {
                    // 最後の callsite_id を見つけて、edge_id を収集
                    // TODO: より正確な callsite 特定を実装
                    let edge_ids: Vec<_> = self.edge_tracker.edges_by_parent(parent_id)
                        .filter(|edge| edge.child == child_id)
                        .map(|edge| edge.compute_id())
                        .collect();

                    // 収集した edge_id をマーク
                    for edge_id in edge_ids {
                        self.edge_tracker.mark_completed(edge_id);
                    }
                }

                // タスクを完了としてマーク
                if let Some(task) = self.task_tracker.get_mut(child_id) {
                    task.completed = true;
                }
            }
        } else {
            // スタックが空の場合は再同期が必要
            // Note: 再同期は外部（debugger）から resync_from_stack() を呼び出して実行する
        }

        Ok(())
    }

    /// OS スタックからスコープスタックを再同期する
    ///
    /// 実際の OS スタックから取得したタスクリストで、内部のスコープスタックを同期します。
    /// panic や unwind で exit BP を経由せずに関数から抜けた場合に使用します。
    ///
    /// # Arguments
    /// * `tid` - スレッド ID
    /// * `actual_tasks` - OS スタックから取得した実際のタスクリスト（子→親の順）
    pub fn resync_from_stack(&mut self, tid: Tid, actual_tasks: Vec<u64>) {
        let scope = self.scope_manager.get_or_create(tid);
        scope.resync(actual_tasks);
    }

    /// 親の GenFuture::poll をスタックからスキャンする
    ///
    /// TODO: 実際のフレームスキャンを実装する
    fn scan_parent_genfuture(&self, _tid: Tid) -> Result<Option<TaskId>> {
        // 現時点ではスタブ
        Ok(None)
    }

    /// 親タスクを探索する（フレームスキャン優先、次にスコープスタック）
    ///
    /// この関数は以下の順序で親タスクを探索します：
    /// 1. フレームスキャンによる検出（`scan_parent_genfuture`）
    /// 2. スコープスタックの最上位タスク
    /// 3. どちらも見つからない場合は None
    fn find_parent_task(&self, tid: Tid) -> Result<Option<TaskId>> {
        Ok(self.scan_parent_genfuture(tid)?
            .or_else(|| {
                self.scope_manager.get(tid)
                    .and_then(|scope| scope.top())
            }))
    }

    /// タスクトラッカーへの参照を取得する
    pub fn task_tracker(&self) -> &TaskTracker {
        &self.task_tracker
    }

    /// エッジトラッカーへの参照を取得する
    pub fn edge_tracker(&self) -> &EdgeTracker {
        &self.edge_tracker
    }

    /// 呼び出しサイトトラッカーへの参照を取得する
    pub fn callsite_tracker(&self) -> &CallsiteTracker {
        &self.callsite_tracker
    }

    /// GenFuture検出器への参照を取得する
    pub fn detector(&self) -> &GenFutureDetector {
        &self.detector
    }

    /// 論理スタック（子→親）を取得する
    ///
    /// 指定したスレッドの現在の async バックトレースを返す
    pub fn async_backtrace(&self, tid: Tid) -> Vec<TaskId> {
        self.scope_manager.get(tid)
            .map(|scope| scope.stack().to_vec())
            .unwrap_or_default()
    }

    /// すべてのタスクを取得する
    pub fn all_tasks(&self) -> Vec<&TaskInfo> {
        self.task_tracker.all_tasks().collect()
    }

    /// すべてのエッジを取得する
    pub fn all_edges(&self) -> Vec<&Edge> {
        self.edge_tracker.all_edges().collect()
    }

    /// 指定した親のエッジを取得する
    pub fn edges_by_parent(&self, parent: TaskId) -> Vec<&Edge> {
        self.edge_tracker.edges_by_parent(parent).collect()
    }

    /// タスクを取得する
    pub fn get_task(&self, task_id: TaskId) -> Option<&TaskInfo> {
        self.task_tracker.get(task_id)
    }

    /// 呼び出しサイトを取得する
    pub fn get_callsite(&self, callsite_id: CallsiteId) -> Option<&Callsite> {
        self.callsite_tracker.get(callsite_id)
    }
}

impl Default for AsyncTracker {
    fn default() -> Self {
        Self::new().expect("Failed to create AsyncTracker")
    }
}

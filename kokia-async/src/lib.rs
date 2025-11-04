//! Kokia 非同期関数デバッグ機能
//!
//! このクレートは、Rustの非同期関数をデバッグするための機能を提供します。
//! GenFuture::pollの監視、論理スタック（awaitチェーン）の構築、
//! 生成器の状態（discriminant）の読み取りなどを行います。

pub mod genfuture;
pub mod logical_stack;
pub mod task;
pub mod tracker;

pub use genfuture::GenFutureDetector;
pub use logical_stack::{LogicalStack, LogicalFrame};
pub use task::{
    Tid, TaskId, TaskInfo, TaskTracker,
    EdgeId, Edge, EdgeTracker,
    CallsiteId, Callsite, CallsiteTracker,
    PollScope, ThreadPollScopeManager,
};
pub use tracker::AsyncTracker;

/// async機能の結果型
pub type Result<T> = anyhow::Result<T>;

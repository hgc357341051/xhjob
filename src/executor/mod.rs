//! Task executors: HTTP and Shell.

pub mod http;
pub mod shell;

pub use http::HttpExecutor;
pub use shell::ShellExecutor;

use crate::errors::Result;
use crate::store::{Task, TaskResult, TaskStore};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Unified executor trait.
pub trait Executor: Send + Sync {
    /// Run the task synchronously (from within an async context).
    /// Returns the execution result.
    fn execute<'a>(
        &'a self,
        task: &'a Task,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>>;

    /// 带取消标志的执行。默认实现忽略 `cancel_flag`，直接委托给 [`execute`]。
    /// 需要支持运行时取消的执行器（如 ShellExecutor）覆盖此方法，
    /// 在执行期间定期检查 `cancel_flag`，若被设置则立即终止子进程。
    fn execute_with_cancel<'a>(
        &'a self,
        task: &'a Task,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        let _ = cancel_flag;
        self.execute(task)
    }

    /// execution_lease: like execute_with_cancel, but also accepts an optional
    /// store reference so the executor can persist worker_pid + worker_starttime
    /// at spawn time (before the task completes). This is critical for crash
    /// recovery: if the daemon dies while a task is running, the store must
    /// already contain the worker_pid so reset_running_to_pending can detect
    /// the orphan child. Default impl ignores the store and delegates to
    /// execute_with_cancel (backward compat for executors that don't spawn
    /// child processes).
    fn execute_with_lease<'a>(
        &'a self,
        task: &'a Task,
        cancel_flag: Option<Arc<AtomicBool>>,
        store: Option<Arc<dyn TaskStore>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        let _ = store;
        self.execute_with_cancel(task, cancel_flag)
    }
}

/// Dispatch a task to the appropriate executor based on its type.
/// `cancel_flag` 为 `Some` 时，支持运行时取消（当前仅 ShellExecutor 实现）。
/// `store` 为 `Some` 时，executor 在 spawn 子进程后立即写入 worker_pid +
/// worker_starttime（execution_lease），使崩溃恢复能检测孤儿进程。
pub fn dispatch(
    task: &Task,
    cancel_flag: Option<Arc<AtomicBool>>,
    store: Option<Arc<dyn TaskStore>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>> {
    match task.task_type {
        crate::store::TaskType::Http => HttpExecutor.execute_with_lease(task, cancel_flag, store),
        crate::store::TaskType::Shell => ShellExecutor.execute_with_lease(task, cancel_flag, store),
    }
}

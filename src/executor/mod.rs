//! Task executors: HTTP and Shell.

pub mod http;
pub mod shell;

pub use http::HttpExecutor;
pub use shell::ShellExecutor;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use crate::errors::Result;
use crate::store::{Task, TaskResult};

/// Unified executor trait.
pub trait Executor: Send + Sync {
    /// Run the task synchronously (from within an async context).
    /// Returns the execution result.
    fn execute<'a>(&'a self, task: &'a Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>>;

    /// 带取消标志的执行。默认实现忽略 `cancel_flag`，直接委托给 [`execute`]。
    /// 需要支持运行时取消的执行器（如 ShellExecutor）覆盖此方法，
    /// 在执行期间定期检查 `cancel_flag`，若被设置则立即终止子进程。
    fn execute_with_cancel<'a>(&'a self, task: &'a Task, cancel_flag: Option<Arc<AtomicBool>>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        let _ = cancel_flag;
        self.execute(task)
    }
}

/// Dispatch a task to the appropriate executor based on its type.
/// `cancel_flag` 为 `Some` 时，支持运行时取消（当前仅 ShellExecutor 实现）。
pub fn dispatch(task: &Task, cancel_flag: Option<Arc<AtomicBool>>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>> {
    match task.task_type {
        crate::store::TaskType::Http => HttpExecutor.execute_with_cancel(task, cancel_flag),
        crate::store::TaskType::Shell => ShellExecutor.execute_with_cancel(task, cancel_flag),
    }
}

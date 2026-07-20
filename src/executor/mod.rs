//! Task executors: HTTP and Shell.

pub mod http;
pub mod shell;

pub use http::HttpExecutor;
pub use shell::ShellExecutor;

use crate::errors::Result;
use crate::store::{Task, TaskResult};

/// Unified executor trait.
pub trait Executor: Send + Sync {
    /// Run the task synchronously (from within an async context).
    /// Returns the execution result.
    fn execute(&self, task: &Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>>;
}

/// Dispatch a task to the appropriate executor based on its type.
pub fn dispatch(task: &Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>> {
    match task.task_type {
        crate::store::TaskType::Http => HttpExecutor.execute(task),
        crate::store::TaskType::Shell => ShellExecutor.execute(task),
    }
}

//! Default in-memory task store.

use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::errors::Result;
use super::{Task, TaskResult, TaskState, TaskStore};

pub struct InMemoryStore {
    tasks: RwLock<HashMap<String, Task>>,
    results: RwLock<HashMap<String, TaskResult>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            results: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self { Self::new() }
}

impl TaskStore for InMemoryStore {
    fn insert_task(&self, task: Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            guard.insert(task.id.clone(), task);
            Ok(())
        })
    }

    fn update_state(&self, id: &str, state: TaskState, started_at: Option<u64>, finished_at: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            if let Some(t) = guard.get_mut(&id) {
                t.state = state;
                if let Some(s) = started_at { t.started_at = Some(s); }
                if let Some(f) = finished_at { t.finished_at = Some(f); }
            }
            Ok(())
        })
    }

    fn save_result(&self, task_id: &str, result: TaskResult) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        Box::pin(async move {
            let mut guard = self.results.write().await;
            guard.insert(task_id, result);
            Ok(())
        })
    }

    fn load_active_tasks(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>> {
        Box::pin(async move {
            let guard = self.tasks.read().await;
            Ok(guard.values().filter(|t| !t.state.is_terminal()).cloned().collect())
        })
    }

    fn load_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let guard = self.tasks.read().await;
            Ok(guard.get(&id).cloned())
        })
    }

    fn load_result(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let guard = self.results.read().await;
            Ok(guard.get(&id).cloned())
        })
    }

    fn count_running_instances(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let guard = self.tasks.read().await;
            // In-memory: each task ID has only one task instance, but we treat
            // RUNNING state as "1 running instance".
            let count = guard.get(&id).filter(|t| t.state.is_running()).map(|_| 1u32).unwrap_or(0);
            Ok(count)
        })
    }

    fn update_next_fire(&self, id: &str, next_fire: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            if let Some(t) = guard.get_mut(&id) {
                t.next_fire = next_fire;
            }
            Ok(())
        })
    }

    fn set_attempts_and_error(&self, id: &str, attempts: u32, last_error: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            if let Some(t) = guard.get_mut(&id) {
                t.attempts = attempts;
                t.last_error = last_error;
            }
            Ok(())
        })
    }

    fn delete_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            guard.remove(&id);
            let mut rg = self.results.write().await;
            rg.remove(&id);
            Ok(())
        })
    }
}

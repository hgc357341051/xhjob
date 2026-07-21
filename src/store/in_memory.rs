//! Default in-memory task store.

use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::errors::{Result, XhjobError};
use super::{Task, TaskResult, TaskState, TaskStore, TaskSummary};

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

    fn increment_execution_count(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = guard.get_mut(&id)
                .ok_or_else(|| XhjobError::TaskNotFound(id.clone()))?;
            task.execution_count += 1;
            Ok(task.execution_count)
        })
    }

    fn remove_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            guard.remove(&id)
                .ok_or_else(|| XhjobError::TaskNotFound(id.clone()))?;
            let mut rg = self.results.write().await;
            rg.remove(&id);
            Ok(())
        })
    }

    fn set_paused(&self, id: &str, paused: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = guard.get_mut(&id)
                .ok_or_else(|| XhjobError::TaskNotFound(id.clone()))?;
            task.paused = paused;
            Ok(())
        })
    }

    fn cancel_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = guard.get_mut(&id)
                .ok_or_else(|| XhjobError::TaskNotFound(id.clone()))?;
            match task.state {
                TaskState::Pending => {
                    task.state = TaskState::Cancelled;
                    task.finished_at = Some(crate::store::now_ts());
                }
                TaskState::Running => {
                    task.cancel_requested = true;
                }
                _ => {
                    return Err(XhjobError::InvalidTask(format!(
                        "task {} already in terminal state: {:?}",
                        id, task.state
                    )));
                }
            }
            Ok(())
        })
    }

    fn cleanup_expired_results(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let tasks = self.tasks.read().await;
            let mut results = self.results.write().await;
            let now = crate::store::now_ts();
            let mut to_remove = Vec::new();
            for (id, _result) in results.iter() {
                if let Some(task) = tasks.get(id) {
                    if task.result_ttl > 0 {
                        if let Some(finished_at) = task.finished_at {
                            if now > finished_at.saturating_add(task.result_ttl) {
                                to_remove.push(id.clone());
                            }
                        }
                    }
                }
            }
            let mut deleted = 0u64;
            for id in to_remove {
                if results.remove(&id).is_some() {
                    deleted += 1;
                }
            }
            Ok(deleted)
        })
    }

    fn list_tasks(&self, state_filter: Option<TaskState>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>> {
        Box::pin(async move {
            let map = self.tasks.read().await;
            let mut result = Vec::new();
            for task in map.values() {
                if let Some(filter) = state_filter {
                    if task.state != filter { continue; }
                }
                result.push(TaskSummary::from(task));
            }
            // Stable: order by created_at ASC for deterministic output.
            result.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(result)
        })
    }

    fn requeue_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = match guard.get_mut(&id) {
                Some(t) => t,
                None => return Ok(false),
            };
            // Only requeue terminal Cancelled / Failed / Expired tasks.
            let requeueable = matches!(task.state,
                TaskState::Cancelled | TaskState::Failed | TaskState::Expired);
            if !requeueable {
                return Ok(false);
            }
            task.state = TaskState::Pending;
            task.attempts = 0;
            task.last_error = None;
            task.started_at = None;
            task.finished_at = None;
            task.cancel_requested = false;
            task.next_fire = Some(crate::store::now_ts());
            Ok(true)
        })
    }

    fn reschedule_task(&self, id: &str, new_cron: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        let new_cron = new_cron.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = match guard.get_mut(&id) {
                Some(t) => t,
                None => return Ok(false), // task not found
            };
            // Only reschedule cron tasks (interval / runAt tasks return false).
            if task.cron.is_none() {
                return Ok(false);
            }
            // Don't reschedule terminal tasks.
            if task.state.is_terminal() {
                return Ok(false);
            }
            // Validate the new cron by computing the next fire time.
            let now = crate::store::now_ts();
            let new_next = crate::scheduler::cron::next_fire(&new_cron, now, task.timezone.as_deref())
                .map_err(|e| XhjobError::CronParse(format!("invalid cron '{}': {}", new_cron, e)))?;
            task.cron = Some(new_cron);
            task.next_fire = Some(new_next);
            // state / execution_count / attempts / meta preserved.
            Ok(true)
        })
    }

    fn reset_running_to_pending(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let now = crate::store::now_ts();
            let mut reset = 0u64;
            for task in guard.values_mut() {
                if task.state == TaskState::Running && task.acks_late {
                    task.state = TaskState::Pending;
                    task.next_fire = Some(now);
                    // Clear started_at / finished_at so the next execution
                    // records fresh timestamps.
                    task.started_at = None;
                    task.finished_at = None;
                    reset += 1;
                }
            }
            Ok(reset)
        })
    }
}

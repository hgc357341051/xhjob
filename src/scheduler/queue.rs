//! Background task queue (reference: Celery queue).
//!
//! Memory queue + store-backed persistence. The daemon pulls pending tasks
//! and dispatches them to the appropriate executor via the coroutine pool.

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::errors::Result;
use crate::store::{Task, TaskState, TaskStore, now_ts};
use crate::executor::dispatch as dispatch_task;
use crate::pool::coroutine_pool;
use super::overlap::OverlapController;

pub struct TaskQueue {
    store: Arc<dyn TaskStore>,
    overlap: Arc<OverlapController>,
    /// Pending task ids awaiting execution (in-memory fast path).
    pending: Mutex<Vec<(i32, String)>>, // (priority, task_id), max-heap semantics
}

impl TaskQueue {
    pub fn new(store: Arc<dyn TaskStore>, overlap: Arc<OverlapController>) -> Self {
        Self {
            store,
            overlap,
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Enqueue a task by id (already persisted in the store).
    pub async fn enqueue(&self, task_id: &str, priority: i32) -> Result<()> {
        let mut g = self.pending.lock().await;
        g.push((priority, task_id.to_string()));
        // Sort by priority descending (higher priority first)
        g.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(())
    }

    /// Drain the next pending task id (highest priority first).
    async fn drain_next(&self) -> Option<String> {
        let mut g = self.pending.lock().await;
        if g.is_empty() { return None; }
        // Since we sort desc on enqueue, the first element is highest priority
        let (_priority, id) = g.remove(0);
        Some(id)
    }

    /// Process one task: overlap check -> dispatch -> state update.
    pub async fn process_one(&self) -> Result<()> {
        let task_id = match self.drain_next().await {
            Some(id) => id,
            None => return Ok(()),
        };
        let task = match self.store.load_task(&task_id).await? {
            Some(t) => t,
            None => return Ok(()), // task was deleted
        };

        // Skip if terminal
        if task.state.is_terminal() {
            return Ok(());
        }

        // Overlap check
        if !self.overlap.should_fire(&self.store, &task).await? {
            return Ok(()); // skipped
        }

        // Mark as RUNNING
        self.overlap.on_start(&task.id).await;
        self.store.update_state(
            &task.id,
            TaskState::Running,
            Some(now_ts()),
            None,
        ).await?;

        // Dispatch via coroutine pool
        let store = Arc::clone(&self.store);
        let overlap = Arc::clone(&self.overlap);
        let task_clone = task.clone();
        coroutine_pool::global().spawn(async move {
            let result = dispatch_task(&task_clone).await;
            let finished = now_ts();
            match result {
                Ok(r) => {
                    let _ = store.save_result(&task_clone.id, r).await;
                    // Determine success/failure
                    let success = match task_clone.task_type {
                        crate::store::TaskType::Http => {
                            // 2xx = success
                            store.load_result(&task_clone.id).await
                                .ok().flatten()
                                .and_then(|r| r.status_code)
                                .map(|c| (200..300).contains(&c))
                                .unwrap_or(true)
                        }
                        crate::store::TaskType::Shell => {
                            store.load_result(&task_clone.id).await
                                .ok().flatten()
                                .and_then(|r| r.exit_code)
                                .map(|c| c == 0)
                                .unwrap_or(true)
                        }
                    };
                    if success {
                        let _ = store.update_state(
                            &task_clone.id,
                            TaskState::Success,
                            None,
                            Some(finished),
                        ).await;
                    } else {
                        // Failure: schedule a retry (or mark FAILED if exhausted)
                        let err_msg = match task_clone.task_type {
                            crate::store::TaskType::Http => {
                                let code = store.load_result(&task_clone.id).await
                                    .ok().flatten()
                                    .and_then(|r| r.status_code)
                                    .unwrap_or(0);
                                format!("http status {}", code)
                            }
                            crate::store::TaskType::Shell => {
                                let code = store.load_result(&task_clone.id).await
                                    .ok().flatten()
                                    .and_then(|r| r.exit_code)
                                    .unwrap_or(-1);
                                format!("shell exit {}", code)
                            }
                        };
                        let _ = crate::retry::schedule_retry(&store, &task_clone, err_msg).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(task_id = %task_clone.id, error = %e, "task execution failed");
                    let _ = crate::retry::schedule_retry(&store, &task_clone, format!("{}", e)).await;
                }
            }
            overlap.on_finish(&task_clone.id).await;
        });

        Ok(())
    }

    /// Run the queue loop. Pulls pending tasks and processes them.
    /// Stops when `shutdown` becomes true.
    pub async fn run(self: Arc<Self>, shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut shutdown_rx = shutdown;
        let mut retry_scan_ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        retry_scan_ticker.tick().await; // discard first immediate tick
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if let Err(e) = self.process_one().await {
                        tracing::warn!(error = %e, "queue process_one error");
                    }
                }
                _ = retry_scan_ticker.tick() => {
                    if let Err(e) = self.scan_retries().await {
                        tracing::warn!(error = %e, "retry scan error");
                    }
                }
                res = shutdown_rx.changed() => {
                    if res.is_err() || *shutdown_rx.borrow() {
                        tracing::info!("task queue shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Scan for tasks in PENDING state whose `next_fire` has arrived (retry-scheduled).
    /// Re-enqueues them so they can be processed again.
    async fn scan_retries(&self) -> Result<()> {
        let now = now_ts();
        let active = self.store.load_active_tasks().await?;
        for task in active {
            // Only consider retry-scheduled tasks (PENDING + next_fire set + no cron).
            // Cron tasks are handled by CronScheduler.
            if task.state != TaskState::Pending { continue; }
            if task.cron.is_some() { continue; }
            match task.next_fire {
                Some(t) if t <= now => {
                    let _ = self.enqueue(&task.id, task.priority).await;
                    // Clear next_fire so we don't re-enqueue the same retry multiple times.
                    let _ = self.store.update_next_fire(&task.id, None).await;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

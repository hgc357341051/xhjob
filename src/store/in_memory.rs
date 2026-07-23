//! Default in-memory task store.

use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::errors::{Result, XhjobError};
use super::{Task, TaskResult, TaskState, TaskStore, TaskSummary, TaskEvent, ChainRecord, GroupRecord, ChordRecord, WorkerStats};

pub struct InMemoryStore {
    tasks: RwLock<HashMap<String, Task>>,
    results: RwLock<HashMap<String, TaskResult>>,
    events: RwLock<Vec<TaskEvent>>,
    chains: RwLock<HashMap<String, ChainRecord>>,
    groups: RwLock<HashMap<String, GroupRecord>>,
    chords: RwLock<HashMap<String, ChordRecord>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            results: RwLock::new(HashMap::new()),
            events: RwLock::new(Vec::new()),
            chains: RwLock::new(HashMap::new()),
            groups: RwLock::new(HashMap::new()),
            chords: RwLock::new(HashMap::new()),
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
            // Drop the tasks write guard before awaiting the results lock to
            // avoid holding two locks across an await boundary.
            drop(guard);
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
            // Drop the tasks write guard before awaiting the results lock to
            // avoid holding two locks across an await boundary.
            drop(guard);
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
                    // H5 fix: record a Cancelled event so the audit log is
                    // complete for directly-cancelled pending tasks.
                    drop(guard);
                    let _ = self.record_event(&id, super::EventType::Cancelled, None, crate::store::now_ts() as i64).await;
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
            let now = crate::store::now_ts();
            // Snapshot the (result_ttl, finished_at) we need to consult, then
            // drop the tasks read guard before awaiting the results write lock
            // to avoid holding two locks across an await boundary.
            let task_info: HashMap<String, (u64, Option<u64>)> = {
                let tasks = self.tasks.read().await;
                tasks.values()
                    .map(|t| (t.id.clone(), (t.result_ttl, t.finished_at)))
                    .collect()
            };
            let mut results = self.results.write().await;
            let mut to_remove = Vec::new();
            for (id, _result) in results.iter() {
                if let Some((ttl, finished_at)) = task_info.get(id) {
                    if *ttl > 0 {
                        if let Some(fa) = finished_at {
                            if now > fa.saturating_add(*ttl) {
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

    fn list_tasks<'a>(&'a self, state_filter: Option<TaskState>, tag_filter: Option<&'a str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>> {
        Box::pin(async move {
            let map = self.tasks.read().await;
            let mut result = Vec::new();
            for task in map.values() {
                if let Some(filter) = state_filter {
                    if task.state != filter { continue; }
                }
                if let Some(tag) = tag_filter {
                    if !task.tags.iter().any(|t| t == tag) { continue; }
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
            // 仅终态 Cancelled / Failed / Expired / Success 任务可重新入队。
            let requeueable = matches!(task.state,
                TaskState::Cancelled | TaskState::Failed | TaskState::Expired | TaskState::Success);
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
                if task.state == TaskState::Running || task.state == TaskState::Interrupted {
                    // P0 fix (C1): reset ALL running tasks on startup, not
                    // just acks_late ones. A daemon crash leaves Running tasks
                    // with no worker executing them — without this reset they
                    // stay Running forever and are never re-enqueued (scan
                    // only picks up Pending). The old code only reset
                    // acks_late=true tasks, meaning the majority of tasks
                    // (acks_late defaults to false) were silently orphaned
                    // on every unclean restart.
                    //
                    // Also reset Interrupted tasks: these are tasks that were
                    // Running when the daemon was gracefully shut down (the
                    // daemon marks them Interrupted on shutdown so users can
                    // distinguish "interrupted by shutdown" from "crashed").
                    // On restart they should be re-enqueued.
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

    fn mark_running_as_interrupted(&self, reason: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        let reason = reason.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let now = crate::store::now_ts() as i64;
            let mut transitioned: Vec<String> = Vec::new();
            for (id, task) in guard.iter_mut() {
                if task.state == TaskState::Running {
                    task.state = TaskState::Interrupted;
                    task.finished_at = Some(now as u64);
                    transitioned.push(id.clone());
                }
            }
            let count = transitioned.len() as u64;
            drop(guard);
            // Record an Interrupted event per task outside the tasks write
            // lock to avoid re-entrancy with the events lock.
            for id in &transitioned {
                if let Err(e) = self.record_event(
                    id,
                    super::EventType::Interrupted,
                    Some(&reason),
                    now,
                ).await {
                    tracing::warn!(task_id = %id, error = %e, "record_event Interrupted failed");
                }
            }
            Ok(count)
        })
    }

    // ----- Event log (A17) -----

    fn record_event(&self, task_id: &str, event_type: super::EventType, payload: Option<&str>, ts: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        let payload = payload.map(|s| s.to_string());
        Box::pin(async move {
            let mut guard = self.events.write().await;
            guard.push(TaskEvent { task_id, event_type, payload, ts });
            Ok(())
        })
    }

    fn list_events(&self, since_ts: i64, task_id_filter: Option<&str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>> {
        let task_id_filter = task_id_filter.map(|s| s.to_string());
        Box::pin(async move {
            let guard = self.events.read().await;
            let mut out: Vec<TaskEvent> = guard.iter()
                .filter(|e| e.ts >= since_ts)
                .filter(|e| match &task_id_filter {
                    Some(id) => &e.task_id == id,
                    None => true,
                })
                .cloned()
                .collect();
            out.sort_by(|a, b| a.ts.cmp(&b.ts));
            Ok(out)
        })
    }

    fn cleanup_expired_events(&self, ttl_secs: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let now = crate::store::now_ts() as i64;
            let cutoff = now.saturating_sub(ttl_secs as i64);
            let mut guard = self.events.write().await;
            let before = guard.len();
            guard.retain(|e| e.ts >= cutoff);
            let after = guard.len();
            Ok((before - after) as u64)
        })
    }

    // ----- Task chain (C15) -----

    fn create_chain(&self, chain_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let mut guard = self.chains.write().await;
            guard.insert(chain_id.clone(), ChainRecord {
                chain_id,
                tasks,
                current_step: 0,
                state: "pending".to_string(),
                created_at,
                updated_at: created_at,
            });
            Ok(())
        })
    }

    fn get_chain(&self, chain_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        Box::pin(async move {
            let guard = self.chains.read().await;
            Ok(guard.get(&chain_id).cloned())
        })
    }

    fn update_chain_step(&self, chain_id: &str, current_step: u32, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let mut guard = self.chains.write().await;
            if let Some(c) = guard.get_mut(&chain_id) {
                c.current_step = current_step;
                c.state = state;
                c.updated_at = updated_at;
            }
            Ok(())
        })
    }

    fn list_chains_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>> {
        let state = state.to_string();
        Box::pin(async move {
            let guard = self.chains.read().await;
            let mut out: Vec<ChainRecord> = guard.values()
                .filter(|c| c.state == state)
                .cloned()
                .collect();
            out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(out)
        })
    }

    // ----- Task group (C16) -----

    fn create_group(&self, group_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let mut guard = self.groups.write().await;
            guard.insert(group_id.clone(), GroupRecord {
                group_id,
                tasks,
                state: "pending".to_string(),
                created_at,
                updated_at: created_at,
            });
            Ok(())
        })
    }

    fn get_group(&self, group_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>> {
        let group_id = group_id.to_string();
        Box::pin(async move {
            let guard = self.groups.read().await;
            Ok(guard.get(&group_id).cloned())
        })
    }

    fn update_group_state(&self, group_id: &str, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let mut guard = self.groups.write().await;
            if let Some(g) = guard.get_mut(&group_id) {
                g.state = state;
                g.updated_at = updated_at;
            }
            Ok(())
        })
    }

    fn list_groups_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>> {
        let state = state.to_string();
        Box::pin(async move {
            let guard = self.groups.read().await;
            let mut out: Vec<GroupRecord> = guard.values()
                .filter(|g| g.state == state)
                .cloned()
                .collect();
            out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(out)
        })
    }

    // ----- Task chord (C16+) -----

    fn create_chord(&self, id: &str, header_task_ids: &[String], callback_json: &str, created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        let header_task_ids = header_task_ids.to_vec();
        let callback_json = callback_json.to_string();
        Box::pin(async move {
            let mut guard = self.chords.write().await;
            guard.insert(id.clone(), ChordRecord {
                id,
                header_task_ids,
                callback_json,
                callback_task_id: None,
                state: "pending".to_string(),
                created_at,
                updated_at: created_at,
            });
            Ok(())
        })
    }

    fn get_chord(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChordRecord>>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let guard = self.chords.read().await;
            Ok(guard.get(&id).cloned())
        })
    }

    fn update_chord_state(&self, id: &str, state: &str, callback_task_id: Option<String>, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let mut guard = self.chords.write().await;
            if let Some(c) = guard.get_mut(&id) {
                c.state = state;
                if let Some(cid) = callback_task_id {
                    c.callback_task_id = Some(cid);
                }
                c.updated_at = updated_at;
            }
            Ok(())
        })
    }

    // ----- Progress / inspect (Task 1-5) -----

    fn update_progress(&self, id: &str, percent: u8, meta: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = guard.get_mut(&id)
                .ok_or_else(|| XhjobError::TaskNotFound(id.clone()))?;
            task.progress = Some(percent);
            task.progress_meta = meta;
            Ok(())
        })
    }

    fn list_active_summary(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>> {
        Box::pin(async move {
            let guard = self.tasks.read().await;
            let mut out: Vec<TaskSummary> = guard.values()
                .filter(|t| t.state == TaskState::Running)
                .map(TaskSummary::from)
                .collect();
            out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(out)
        })
    }

    fn list_registered_summary(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>> {
        Box::pin(async move {
            let guard = self.tasks.read().await;
            let mut out: Vec<TaskSummary> = guard.values()
                .filter(|t| t.cron.is_some() || t.interval.is_some())
                .map(TaskSummary::from)
                .collect();
            out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(out)
        })
    }

    fn list_scheduled_summary(&self, now: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>> {
        Box::pin(async move {
            let guard = self.tasks.read().await;
            let mut out: Vec<TaskSummary> = guard.values()
                .filter(|t| t.next_fire.map(|nf| nf > now).unwrap_or(false))
                .map(TaskSummary::from)
                .collect();
            out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(out)
        })
    }

    fn worker_stats(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkerStats>> + Send + '_>> {
        Box::pin(async move {
            let guard = self.tasks.read().await;
            let mut stats = WorkerStats {
                total: 0,
                pending: 0,
                running: 0,
                success: 0,
                failed: 0,
                queue_depth: 0,
            };
            let now = crate::store::now_ts();
            for t in guard.values() {
                stats.total += 1;
                match t.state {
                    TaskState::Pending => stats.pending += 1,
                    TaskState::Running => stats.running += 1,
                    TaskState::Success => stats.success += 1,
                    TaskState::Failed => stats.failed += 1,
                    _ => {}
                }
                if t.next_fire.map(|nf| nf > now).unwrap_or(false) {
                    stats.queue_depth += 1;
                }
            }
            Ok(stats)
        })
    }

    fn modify_job(&self, id: &str, patch: &serde_json::Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        let patch = patch.clone();
        Box::pin(async move {
            let mut guard = self.tasks.write().await;
            let task = match guard.get_mut(&id) {
                Some(t) => t,
                None => return Ok(false),
            };
            if task.state.is_terminal() {
                return Ok(false);
            }
            let now = crate::store::now_ts();
            let mut trigger_changed = false;
            if let Some(obj) = patch.as_object() {
                for (key, val) in obj {
                    match key.as_str() {
                        "cron" => {
                            task.cron = val.as_str().map(|s| s.to_string());
                            trigger_changed = true;
                        }
                        "or_cron" => {
                            task.or_cron = val.as_array().map(|arr| {
                                arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                            });
                            trigger_changed = true;
                        }
                        "interval" => {
                            task.interval = val.as_u64();
                            trigger_changed = true;
                        }
                        "run_at" => {
                            task.run_at = val.as_i64();
                            trigger_changed = true;
                        }
                        "timezone" => {
                            task.timezone = val.as_str().map(|s| s.to_string());
                            trigger_changed = true;
                        }
                        "priority" => { if let Some(p) = val.as_i64() { task.priority = p as i32; } }
                        "max_executions" => { if let Some(m) = val.as_u64() { task.max_executions = m as u32; } }
                        "paused" => { if let Some(p) = val.as_bool() { task.paused = p; } }
                        "timeout" => { if let Some(t) = val.as_u64() { task.timeout = t; } }
                        "soft_timeout" => { task.soft_timeout = val.as_u64(); }
                        "retry_max" => { if let Some(r) = val.as_u64() { task.retry_max = r as u32; } }
                        "retry_delay" => { if let Some(r) = val.as_u64() { task.retry_delay = r; } }
                        "retry_backoff" => { if let Some(b) = val.as_bool() { task.retry_backoff = b; } }
                        "expires" => { if let Some(e) = val.as_u64() { task.expires = e; } }
                        "jitter" => { if let Some(j) = val.as_u64() { task.jitter = j; } }
                        "coalesce" => { if let Some(c) = val.as_bool() { task.coalesce = c; } }
                        "misfire_grace_time" => { if let Some(m) = val.as_u64() { task.misfire_grace_time = m; } }
                        "tags" => {
                            if let Some(arr) = val.as_array() {
                                task.tags = arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                            }
                        }
                        "meta" => { task.meta = val.as_str().map(|s| s.to_string()); }
                        "skip_dates" => {
                            if let Some(arr) = val.as_array() {
                                task.skip_dates = arr.iter().filter_map(|v| v.as_i64()).collect();
                            }
                            trigger_changed = true;
                        }
                        "workdays_only" => { if let Some(w) = val.as_bool() { task.workdays_only = w; } }
                        // Immutable fields (id, owner, state, attempts, created_at) are ignored.
                        _ => {}
                    }
                }
            }
            // Recompute next_fire if a trigger field changed.
            if trigger_changed {
                let cron_exprs: Vec<String> = task.cron.as_ref().cloned().into_iter()
                    .chain(task.or_cron.clone().unwrap_or_default().into_iter().filter(|s| !s.is_empty()))
                    .collect();
                if !cron_exprs.is_empty() {
                    let mut min_next: Option<u64> = None;
                    for expr in &cron_exprs {
                        match crate::scheduler::cron::next_fire(expr, now, task.timezone.as_deref()) {
                            Ok(t) => { min_next = Some(min_next.map_or(t, |m| m.min(t))); }
                            Err(e) => {
                                return Err(XhjobError::CronParse(format!("invalid cron '{}': {}", expr, e)));
                            }
                        }
                    }
                    task.next_fire = min_next;
                } else if let Some(secs) = task.interval {
                    task.next_fire = Some(now + secs);
                } else if let Some(ts) = task.run_at {
                    task.next_fire = Some(ts as u64);
                }
            }
            Ok(true)
        })
    }
}

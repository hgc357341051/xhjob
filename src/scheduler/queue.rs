//! Background task queue (reference: Celery queue).
//!
//! Memory queue + store-backed persistence. The daemon pulls pending tasks
//! and dispatches them to the appropriate executor via the coroutine pool.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::Mutex;
use crate::errors::Result;
use crate::store::{TaskState, TaskStore, now_ts};
use crate::executor::dispatch as dispatch_task;
use crate::pool::coroutine_pool;
use crate::scheduler::RateLimiter;
use super::overlap::OverlapController;

pub struct TaskQueue {
    store: Arc<dyn TaskStore>,
    overlap: Arc<OverlapController>,
    /// Pending task ids awaiting execution (in-memory fast path).
    pending: Mutex<Vec<(i32, String)>>, // (priority, task_id), max-heap semantics
    /// 正在执行的任务的 cancel 标志。task_id -> 共享的 AtomicBool。
    /// `signal_cancel` 设置对应标志，executor 在执行循环中轮询检测以终止子进程。
    /// 任务派发前注册，派发完成后注销。
    cancel_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Per-task rate limiter (C12). Sliding-window counter shared between
    /// scan_once (cron/interval triggers) and process_one (dispatch path).
    /// Reference: Celery rate_limit.
    rate_limiter: Arc<RateLimiter>,
}

impl TaskQueue {
    pub fn new(store: Arc<dyn TaskStore>, overlap: Arc<OverlapController>) -> Self {
        Self {
            store,
            overlap,
            pending: Mutex::new(Vec::new()),
            cancel_flags: Mutex::new(HashMap::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
        }
    }

    /// 注册一个 cancel 标志，供 executor 在执行期间轮询。
    /// 若该 task_id 已存在标志则覆盖（理论上不会发生）。
    async fn register_cancel_flag(&self, task_id: &str, flag: Arc<AtomicBool>) {
        let mut g = self.cancel_flags.lock().await;
        g.insert(task_id.to_string(), flag);
    }

    /// 注销 cancel 标志（任务派发完成后调用）。
    async fn unregister_cancel_flag(&self, task_id: &str) {
        let mut g = self.cancel_flags.lock().await;
        g.remove(task_id);
    }

    /// 对正在执行的任务设置 cancel 标志，使 executor 终止子进程。
    /// 返回是否找到并设置了标志（true 表示任务正在执行且已通知取消）。
    /// 对 Pending 任务无需调用此方法——`cancel_task` 已直接将其转 Cancelled 终态。
    /// Reference: Celery revoke (terminate=true).
    pub async fn signal_cancel(&self, task_id: &str) -> bool {
        let g = self.cancel_flags.lock().await;
        if let Some(flag) = g.get(task_id) {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        } else {
            false
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
    pub async fn process_one(self: Arc<Self>) -> Result<()> {
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

        let now = now_ts();
        // expires 检查（C6）：如果 Pending 任务已超过 expires 窗口
        // （created_at + expires < now），直接转 Expired 终态。这是防御性
        // 检查（scan_once 也会做），避免任务在 scan_once 间隔内被错误派发。
        // 仅对 Pending 任务生效，不影响 Running 任务。
        // Reference: APScheduler expires.
        if task.expires > 0 && task.state == TaskState::Pending {
            let expiry_ts = task.created_at.saturating_add(task.expires);
            if now > expiry_ts {
                let _ = self.store.update_state(
                    &task.id,
                    TaskState::Expired,
                    None,
                    Some(now),
                ).await;
                // 记录 Expired 事件（A17）—— 终态过期。
                let _ = self.store.record_event(
                    &task.id,
                    crate::store::EventType::Expired,
                    None,
                    now as i64,
                ).await;
                return Ok(());
            }
        }
        // start_date 检查：未到开始时间的任务跳过派发，保持 Pending。
        // 这样 scan_once 的 expires 检查可以正常工作，避免任务被提前触发
        // 后立即转 Success 终态，导致 expires 永远无法生效。
        // Reference: APScheduler start_date.
        if let Some(start_ts) = task.start_date {
            if (now as i64) < start_ts {
                // 未到 start_date：延迟重新入队，等 start_date 到达后再派发。
                // 这对"仅设置 start_date 而无 cron/interval/run_at"的一次性
                // 任务至关重要——否则任务被 drain_next 消费后永远不会被
                // scan_once 重新 enqueue（无 trigger）。
                let delay = (start_ts as u64).saturating_sub(now);
                let q = Arc::clone(&self);
                let tid = task.id.clone();
                let pri = task.priority;
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    let _ = q.enqueue(&tid, pri).await;
                });
                return Ok(());
            }
        }

        // rate_limit 检查（C12）：滑动窗口限流。超限时记录 RateLimited
        // 事件，推进 next_fire（避免 scan_once 重复 enqueue），并延迟
        // window 秒后重新入队。对一次性任务也生效（确保限流期间不丢失）。
        // Reference: Celery rate_limit.
        if !self.rate_limiter.check_and_record(&task, now).await {
            let _ = self.store.record_event(
                &task.id,
                crate::store::EventType::RateLimited,
                None,
                now as i64,
            ).await;
            // 推进 next_fire 到 window 后（cron/interval 任务避免 scan_once 重复 enqueue）
            let window = task.rate_limit_window;
            if task.cron.is_some() || task.interval.is_some() {
                let _ = self.store.update_next_fire(&task.id, Some(now + window)).await;
            }
            // 延迟重新入队
            let q = Arc::clone(&self);
            let tid = task.id.clone();
            let pri = task.priority;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(window)).await;
                let _ = q.enqueue(&tid, pri).await;
            });
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
        // Record a `Started` event (A17).
        let _ = self.store.record_event(
            &task.id,
            crate::store::EventType::Started,
            None,
            now_ts() as i64,
        ).await;

        // Dispatch via coroutine pool
        let store = Arc::clone(&self.store);
        let overlap = Arc::clone(&self.overlap);
        let queue_arc = Arc::clone(&self);
        let task_clone = task.clone();
        coroutine_pool::global().spawn(async move {
            // 创建 cancel 标志并注册到 queue，使 handle_cancel_op 能够
            // 通过 signal_cancel 通知 executor 终止子进程。
            let cancel_flag = Arc::new(AtomicBool::new(false));
            queue_arc.register_cancel_flag(&task_clone.id, Arc::clone(&cancel_flag)).await;

            let result = dispatch_task(&task_clone, Some(Arc::clone(&cancel_flag))).await;

            // 派发完成，注销 cancel 标志（后续 signal_cancel 无需再通知）。
            queue_arc.unregister_cancel_flag(&task_clone.id).await;
            let finished = now_ts();
            // Save the result if dispatch succeeded, so the result is visible
            // regardless of whether cancel was requested during execution.
            // For dispatch errors, build a synthetic TaskResult so the retry
            // policy treats it as a network error (retryable).
            let stored: crate::store::TaskResult;
            let dispatch_err: Option<String>;
            match result {
                Ok(r) => {
                    if task_clone.ignore_result {
                        // Fire-and-forget (C9): skip save_result entirely.
                        // Use the dispatch TaskResult directly for
                        // success/failure determination so the state machine
                        // still transitions correctly.
                        stored = r;
                    } else {
                        let _ = store.save_result(&task_clone.id, r).await;
                        // Reload the persisted TaskResult so retry policy sees the
                        // same data the success/failure check used.
                        stored = store.load_result(&task_clone.id).await
                            .ok().flatten()
                            .unwrap_or_default();
                    }
                    dispatch_err = None;
                }
                Err(e) => {
                    tracing::warn!(task_id = %task_clone.id, error = %e, "task execution failed");
                    stored = crate::store::TaskResult::default();
                    dispatch_err = Some(format!("{}", e));
                }
            }

            // If cancel was requested during execution, transition to Cancelled
            // terminal state. No retry, no cron re-trigger.
            // Reference: Celery revoke.
            let cancel_requested = store.load_task(&task_clone.id).await
                .ok().flatten()
                .map(|t| t.cancel_requested)
                .unwrap_or(false);
            if cancel_requested {
                let _ = store.update_state(
                    &task_clone.id,
                    TaskState::Cancelled,
                    None,
                    Some(now_ts()),
                ).await;
                // Record a `Cancelled` event (A17).
                let _ = store.record_event(
                    &task_clone.id,
                    crate::store::EventType::Cancelled,
                    None,
                    now_ts() as i64,
                ).await;
                overlap.on_finish(&task_clone.id).await;
                return;
            }

            // Dispatch error path: no TaskResult was produced. Consult retry
            // policy with the synthetic result (network error -> retryable).
            if let Some(err_str) = dispatch_err {
                let synthetic = crate::store::TaskResult::default();
                // acks_on_failure (C13): when false, dispatch errors are
                // retried indefinitely (ignoring retry_max). Same override
                // as the failure path below.
                let effective_retry_max = if task_clone.acks_on_failure {
                    task_clone.retry_max
                } else {
                    u32::MAX
                };
                let policy = crate::retry::RetryPolicy::new(
                    effective_retry_max, task_clone.retry_delay,
                );
                if !policy.should_retry(&task_clone, &synthetic) {
                    let _ = store.update_state(
                        &task_clone.id,
                        TaskState::Failed,
                        None,
                        Some(now_ts()),
                    ).await;
                    let _ = store.set_attempts_and_error(
                        &task_clone.id,
                        task_clone.attempts + 1,
                        Some(format!("not retryable: {}", err_str)),
                    ).await;
                } else {
                    let _ = crate::retry::schedule_retry(&store, &task_clone, err_str, effective_retry_max).await;
                }
                overlap.on_finish(&task_clone.id).await;
                return;
            }

            // Determine success/failure
            let success = match task_clone.task_type {
                crate::store::TaskType::Http => {
                    // 2xx = success
                    stored.status_code
                        .map(|c| (200..300).contains(&c))
                        .unwrap_or(true)
                }
                crate::store::TaskType::Shell => {
                    stored.exit_code.map(|c| c == 0).unwrap_or(true)
                }
            };
            if success {
                if task_clone.cron.is_some() || task_clone.interval.is_some() {
                    // 周期任务（cron / interval）: 先递增 execution_count，
                    // 然后检查 max_executions。只有达到上限才进入 Success
                    // 终态；否则保持 Pending 状态，让 scan_once 在下一个
                    // 周期重新触发。
                    //
                    // 注意：run_at 是一次性任务（DateTrigger），不在此分支
                    // 内。它在 scan_once 中直接转 Success 终态，process_one
                    // 仍然走非周期任务分支（也是 Success 终态），行为保持
                    // 一致。
                    //
                    // 历史背景：之前的顺序（先 Success 终态 -> 再递增 count）
                    // 是有 bug 的：Success 终态导致 load_active_tasks 把
                    // 任务过滤掉，execution_count 永远无法递增到 2 以上。
                    // interval 任务之前被错误归入"非 cron 任务"分支，触发
                    // 一次后立即 Success 终态，周期触发被提前终止。
                    if let Ok(new_count) = store.increment_execution_count(&task_clone.id).await {
                        if task_clone.max_executions > 0 && new_count >= task_clone.max_executions {
                            let _ = store.update_state(
                                &task_clone.id,
                                TaskState::Success,
                                None,
                                Some(finished),
                            ).await;
                            // Record a `Succeeded` event (A17) — terminal success.
                            let _ = store.record_event(
                                &task_clone.id,
                                crate::store::EventType::Succeeded,
                                None,
                                finished as i64,
                            ).await;
                        } else {
                            let _ = store.update_state(
                                &task_clone.id,
                                TaskState::Pending,
                                None,
                                Some(finished),
                            ).await;
                            // Record a `Succeeded` event (A17) — non-terminal
                            // success (周期任务将由 scan_once 重新触发)。
                            let _ = store.record_event(
                                &task_clone.id,
                                crate::store::EventType::Succeeded,
                                None,
                                finished as i64,
                            ).await;
                        }
                    }
                } else {
                    // 非周期任务（包括 run_at 一次性任务）: 立即进入 Success
                    // 终态，然后递增 execution_count 作为记账。原始逻辑保持
                    // 不变。
                    let _ = store.update_state(
                        &task_clone.id,
                        TaskState::Success,
                        None,
                        Some(finished),
                    ).await;
                    let _ = store.record_event(
                        &task_clone.id,
                        crate::store::EventType::Succeeded,
                        None,
                        finished as i64,
                    ).await;
                    if let Ok(new_count) = store.increment_execution_count(&task_clone.id).await {
                        if task_clone.max_executions > 0 && new_count >= task_clone.max_executions {
                            let _ = store.update_state(
                                &task_clone.id,
                                TaskState::Success,
                                None,
                                Some(crate::store::now_ts()),
                            ).await;
                        }
                    }
                }
            } else {
                // Failure: build a human-readable error summary and
                // consult the retry policy. If the result is not
                // retryable (e.g. HTTP 4xx), mark FAILED immediately
                // rather than going through schedule_retry.
                let err_msg = match task_clone.task_type {
                    crate::store::TaskType::Http => {
                        let code = stored.status_code.unwrap_or(0);
                        format!("http status {}", code)
                    }
                    crate::store::TaskType::Shell => {
                        let code = stored.exit_code.unwrap_or(-1);
                        format!("shell exit {}", code)
                    }
                };
                // acks_on_failure (C13): when false, failures are retried
                // indefinitely (ignoring retry_max). We simulate this by
                // passing retry_max=u32::MAX to the policy so should_retry
                // always returns true. acks_on_failure=true (default)
                // preserves the original retry_max semantics.
                let effective_retry_max = if task_clone.acks_on_failure {
                    task_clone.retry_max
                } else {
                    u32::MAX
                };
                let policy = crate::retry::RetryPolicy::new(
                    effective_retry_max, task_clone.retry_delay,
                );
                if !policy.should_retry(&task_clone, &stored) {
                    // Not retryable: fail permanently right now.
                    let _ = store.update_state(
                        &task_clone.id,
                        TaskState::Failed,
                        None,
                        Some(now_ts()),
                    ).await;
                    let _ = store.set_attempts_and_error(
                        &task_clone.id,
                        task_clone.attempts + 1,
                        Some(format!("not retryable: {}", err_msg)),
                    ).await;
                    let _ = store.record_event(
                        &task_clone.id,
                        crate::store::EventType::Failed,
                        Some(&format!("{{\"error\":{}}}", serde_json::to_string(&err_msg).unwrap_or_default())),
                        now_ts() as i64,
                    ).await;
                } else {
                    // Retryable: schedule retry (which itself may
                    // permanently fail if attempts are exhausted).
                    let _ = crate::retry::schedule_retry(&store, &task_clone, err_msg.clone(), effective_retry_max).await;
                    // Record a `Failed` event (A17) for the individual
                    // attempt; the retry will be processed separately.
                    let _ = store.record_event(
                        &task_clone.id,
                        crate::store::EventType::Failed,
                        Some(&format!("{{\"error\":{}}}", serde_json::to_string(&err_msg).unwrap_or_default())),
                        now_ts() as i64,
                    ).await;
                }
            }
            // Chain (C15) + group (C16) + worker_limits (C5 + C8) hooks.
            // Fire on terminal state transitions; the worker_limits counter
            // is incremented for every completion regardless of final state.
            let final_state = store.load_task(&task_clone.id).await
                .ok().flatten()
                .map(|t| t.state)
                .unwrap_or(task_clone.state);
            if final_state.is_terminal() {
                notify_chain_and_group(&queue_arc, &store, &task_clone, final_state).await;
            }
            record_worker_limits();
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
                    if let Err(e) = Arc::clone(&self).process_one().await {
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

/// Extract a chain_id / group_id marker from a task's `meta` JSON string.
/// Returns `None` if `meta` is missing, not a JSON object, or does not
/// contain the requested key.
fn extract_meta_id(task: &crate::store::Task, key: &str) -> Option<String> {
    let s = task.meta.as_ref()?;
    let s = s.trim();
    if s.is_empty() || s == "null" { return None; }
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Hook fired after a task reaches a terminal state. Inspects the task's
/// `meta` for `xhjob_chain_id` / `xhjob_group_id` markers (set by
/// `daemon_main::handle_chain_op` / `handle_group_op`) and advances the
/// chain or refreshes the group state accordingly.
///
/// Chain (C15):
/// - On Success: advance to the next step. If a next step is returned,
///   build it, tag it with the chain_id, persist, and enqueue.
/// - On Failed / Cancelled: mark the chain as failed (remaining steps
///   are skipped).
///
/// Group (C16): refresh the group state from the live task states
/// regardless of which terminal state this task reached.
async fn notify_chain_and_group(
    queue: &Arc<TaskQueue>,
    store: &Arc<dyn TaskStore>,
    task: &crate::store::Task,
    final_state: TaskState,
) {
    if let Some(chain_id) = extract_meta_id(task, "xhjob_chain_id") {
        match final_state {
            TaskState::Success => {
                advance_chain_and_dispatch(queue, store, &chain_id).await;
            }
            TaskState::Failed | TaskState::Cancelled => {
                if let Err(e) = crate::scheduler::chain::mark_failed(store, &chain_id).await {
                    tracing::warn!(error = %e, chain_id = %chain_id, "chain mark_failed failed");
                }
            }
            _ => {}
        }
    }
    if let Some(group_id) = extract_meta_id(task, "xhjob_group_id") {
        if let Err(e) = crate::scheduler::group::refresh_state(store, &group_id).await {
            tracing::warn!(error = %e, group_id = %group_id, "group refresh_state failed");
        }
    }
}

/// Advance the chain to the next step. If a next step config is returned,
/// build it as a Task, tag its meta with `xhjob_chain_id`, persist, and
/// enqueue via the in-memory queue.
async fn advance_chain_and_dispatch(
    queue: &Arc<TaskQueue>,
    store: &Arc<dyn TaskStore>,
    chain_id: &str,
) {
    match crate::scheduler::chain::advance(store, chain_id).await {
        Ok(Some(next_config)) => {
            let builder = match crate::task::TaskBuilder::from_json(&next_config.to_string()) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, chain_id = chain_id, "chain next step json parse failed");
                    return;
                }
            };
            let mut task = match builder.build() {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, chain_id = chain_id, "chain next step build failed");
                    return;
                }
            };
            // Preserve / install the chain_id marker in meta so the next
            // completion can advance the chain again.
            let meta_obj = match task.meta.take() {
                Some(s) if !s.is_empty() && s != "null" => {
                    match serde_json::from_str::<serde_json::Value>(&s) {
                        Ok(mut v) if v.is_object() => {
                            v.as_object_mut().unwrap().insert(
                                "xhjob_chain_id".to_string(),
                                serde_json::json!(chain_id),
                            );
                            Some(v.to_string())
                        }
                        _ => Some(serde_json::json!({"xhjob_chain_id": chain_id}).to_string()),
                    }
                }
                _ => Some(serde_json::json!({"xhjob_chain_id": chain_id}).to_string()),
            };
            task.meta = meta_obj;
            let task_id = task.id.clone();
            let priority = task.priority;
            if let Err(e) = store.insert_task(task).await {
                tracing::warn!(error = %e, chain_id = chain_id, "chain next step insert failed");
                return;
            }
            if let Err(e) = queue.enqueue(&task_id, priority).await {
                tracing::warn!(error = %e, chain_id = chain_id, "chain next step enqueue failed");
            }
        }
        Ok(None) => {
            tracing::info!(chain_id = chain_id, "chain completed");
        }
        Err(e) => {
            tracing::warn!(error = %e, chain_id = chain_id, "chain advance failed");
        }
    }
}

/// Increment the worker_limits task-execution counter (C5 + C8). If either
/// the max_tasks_per_child or max_memory_per_child limit is reached, spawn
/// a background task that exits the daemon after a brief delay so the
/// current state update can drain. The supervisor (systemd / supervisord /
/// PHP `xhjob_start`) is responsible for restarting.
fn record_worker_limits() {
    let limits = match crate::utils::limits::worker_limits() {
        Some(l) => l,
        None => return,
    };
    let tasks_reached = limits.record_task_execution();
    let mem_reached = limits.check_memory_limit();
    if !tasks_reached && !mem_reached {
        return;
    }
    if tasks_reached {
        tracing::info!(
            limit = limits.max_tasks_per_child,
            executed = limits.tasks_executed.load(std::sync::atomic::Ordering::Relaxed),
            "max_tasks_per_child reached, initiating daemon shutdown"
        );
    }
    if mem_reached {
        tracing::info!(
            limit = limits.max_memory_per_child,
            "max_memory_per_child reached, initiating daemon shutdown"
        );
    }
    tokio::spawn(async {
        // Give the current state update + IPC response a moment to flush.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, Task, TaskType};

    /// Verify that the in-memory pending queue is ordered by priority DESC
    /// (higher priority first), regardless of insertion order.
    /// Reference: Celery priority.
    #[tokio::test]
    async fn test_priority_ordering_high_priority_drained_first() {
        let store = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = TaskQueue::new(store, overlap);

        // Enqueue low-priority task first
        queue.enqueue("low-prio-task", 1).await.unwrap();
        // Enqueue high-priority task second
        queue.enqueue("high-prio-task", 10).await.unwrap();

        // Drain should return the high-priority task first despite being enqueued later.
        let first = queue.drain_next().await.unwrap();
        assert_eq!(first, "high-prio-task");

        // Then the low-priority task.
        let second = queue.drain_next().await.unwrap();
        assert_eq!(second, "low-prio-task");
    }

    /// When priorities are equal, the stable sort preserves insertion order
    /// (FIFO). This matches the "priority DESC, created_at ASC" semantics
    /// since tasks are typically enqueued in created_at order.
    #[tokio::test]
    async fn test_priority_ordering_equal_priority_preserves_insertion_order() {
        let store = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = TaskQueue::new(store, overlap);

        queue.enqueue("first-task", 5).await.unwrap();
        queue.enqueue("second-task", 5).await.unwrap();

        let first = queue.drain_next().await.unwrap();
        assert_eq!(first, "first-task");

        let second = queue.drain_next().await.unwrap();
        assert_eq!(second, "second-task");
    }

    /// `drain_next` on an empty queue returns `None`.
    #[tokio::test]
    async fn test_drain_next_empty_returns_none() {
        let store = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = TaskQueue::new(store, overlap);

        assert!(queue.drain_next().await.is_none());
    }

    /// End-to-end priority ordering: insert two pending tasks into the store
    /// (low-priority first, high-priority second), then run scan_retries +
    /// drain_next and verify the high-priority task is dispatched first.
    #[tokio::test]
    async fn test_scan_retries_respects_priority_ordering() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();

        // Low-priority task inserted first.
        let mut t1 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "low"}));
        t1.id = "t-low".to_string();
        t1.state = TaskState::Pending;
        t1.priority = 1;
        t1.next_fire = Some(now);
        t1.created_at = now;
        store.insert_task(t1).await.unwrap();

        // High-priority task inserted second.
        let mut t2 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "high"}));
        t2.id = "t-high".to_string();
        t2.state = TaskState::Pending;
        t2.priority = 10;
        t2.next_fire = Some(now);
        t2.created_at = now + 1;
        store.insert_task(t2).await.unwrap();

        let overlap = Arc::new(OverlapController::new());
        let queue = TaskQueue::new(Arc::clone(&store), overlap);

        // scan_retries enqueues both pending tasks (sorted by priority DESC).
        queue.scan_retries().await.unwrap();

        // First drained should be the high-priority task.
        let first = queue.drain_next().await.unwrap();
        assert_eq!(first, "t-high");

        let second = queue.drain_next().await.unwrap();
        assert_eq!(second, "t-low");
    }

    /// Cron tasks should NOT transition to Success terminal state on each
    /// successful execution. Instead, execution_count is incremented first,
    /// and only when max_executions is reached does the task become Success.
    ///
    /// This guards against a regression of the maxExecutions timing bug where
    /// `update_state(Success)` was called before `increment_execution_count`,
    /// causing `scan_once` (which filters terminal tasks via
    /// `load_active_tasks`) to drop the task before it could be re-triggered,
    /// so `execution_count` could never advance past 1.
    ///
    /// Reference: APScheduler max_instances / coalesce.
    #[tokio::test]
    async fn test_cron_task_success_not_terminal_until_max() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));

        // Cron task with max_executions=3 and a shell command that succeeds
        // (exit 0). After one successful execution the task should remain
        // Pending (non-terminal) so the next cron tick can re-trigger it.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "true"}));
        task.id = "t-cron-max-exec".to_string();
        task.cron = Some("*/1 * * * *".to_string());
        task.max_executions = 3;
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts());
        store.insert_task(task).await.unwrap();

        // Enqueue and process once. process_one spawns the dispatch on the
        // global coroutine pool and returns immediately.
        queue.enqueue("t-cron-max-exec", 0).await.unwrap();
        Arc::clone(&queue).process_one().await.unwrap();

        // Poll until the dispatch completes (state transitions away from
        // Running). Cap at ~5s to avoid hanging the test on a regression.
        let mut tries = 0;
        loop {
            let t = store.load_task("t-cron-max-exec").await.unwrap().unwrap();
            if t.state != TaskState::Running { break; }
            tries += 1;
            if tries > 500 {
                panic!("task still Running after ~5s of polling; state={:?}",
                    store.load_task("t-cron-max-exec").await.unwrap().unwrap().state);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-cron-max-exec").await.unwrap().unwrap();
        // Cron task with max_executions=3 should NOT be terminal after 1 execution.
        assert_eq!(
            final_task.state,
            TaskState::Pending,
            "cron task should remain Pending after success when max_executions not reached"
        );
        assert!(
            !final_task.state.is_terminal(),
            "cron task should not be in terminal state before max_executions is reached"
        );
        assert_eq!(
            final_task.execution_count, 1,
            "execution_count should be 1 after one successful execution"
        );
    }

    /// ignoreResult (C9): when ignore_result=true, the queue skips
    /// `save_result` so `load_result` returns None even after the task
    /// completes successfully. The state machine still runs.
    /// Reference: Celery ignore_result.
    #[tokio::test]
    async fn test_ignore_result_skips_save_result() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));

        // Shell task with ignore_result=true. "echo hi" succeeds and would
        // normally produce a result row with stdout="hi\n".
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-ignore-result".to_string();
        task.ignore_result = true;
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts());
        store.insert_task(task).await.unwrap();

        queue.enqueue("t-ignore-result", 0).await.unwrap();
        Arc::clone(&queue).process_one().await.unwrap();

        // Poll until the dispatch completes (state transitions away from
        // Running). Cap at ~5s to avoid hanging the test on a regression.
        let mut tries = 0;
        loop {
            let t = store.load_task("t-ignore-result").await.unwrap().unwrap();
            if t.state != TaskState::Running { break; }
            tries += 1;
            if tries > 500 {
                panic!("task still Running after ~5s of polling");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-ignore-result").await.unwrap().unwrap();
        // State machine still ran: task reached Success terminal.
        assert_eq!(final_task.state, TaskState::Success,
            "task should reach Success terminal even with ignore_result=true");
        // But no result row was persisted.
        let result = store.load_result("t-ignore-result").await.unwrap();
        assert!(result.is_none(),
            "load_result should return None when ignore_result=true (no row saved)");
    }

    /// ignoreResult (C9): when ignore_result is false (default), the queue
    /// calls `save_result` as usual so `load_result` returns Some after
    /// completion. Verifies backward compatibility.
    /// Reference: Celery ignore_result.
    #[tokio::test]
    async fn test_ignore_result_default_false_still_saves() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));

        // Default ignore_result=false. "echo hi" produces stdout="hi\n".
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-save-result".to_string();
        task.ignore_result = false;
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts());
        store.insert_task(task).await.unwrap();

        queue.enqueue("t-save-result", 0).await.unwrap();
        Arc::clone(&queue).process_one().await.unwrap();

        // Poll until completion.
        let mut tries = 0;
        loop {
            let t = store.load_task("t-save-result").await.unwrap().unwrap();
            if t.state != TaskState::Running { break; }
            tries += 1;
            if tries > 500 {
                panic!("task still Running after ~5s of polling");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-save-result").await.unwrap().unwrap();
        assert_eq!(final_task.state, TaskState::Success,
            "task should reach Success terminal");

        // Result row WAS persisted.
        let result = store.load_result("t-save-result").await.unwrap();
        assert!(result.is_some(),
            "load_result should return Some when ignore_result=false (default)");
        let r = result.unwrap();
        assert_eq!(r.exit_code, Some(0),
            "exit_code should be 0 for 'echo hi'");
        assert!(r.stdout.as_deref().unwrap_or("").contains("hi"),
            "stdout should contain 'hi', got: {:?}",
            r.stdout);
    }
}

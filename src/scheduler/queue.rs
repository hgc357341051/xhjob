//! Background task queue (reference: Celery queue).
//!
//! Memory queue + store-backed persistence. The daemon pulls pending tasks
//! and dispatches them to the appropriate executor via the coroutine pool.

use super::overlap::OverlapController;
use crate::errors::Result;
use crate::executor::dispatch as dispatch_task;
use crate::pool::coroutine_pool;
use crate::scheduler::RateLimiter;
use crate::store::{now_ts, TaskState, TaskStore};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// RAII guard that decrements the in-flight counter on drop.
/// Ensures the counter is balanced even if the task future panics
/// or returns early. (P1 fix for graceful shutdown drain.)
struct InFlightGuard(Arc<std::sync::atomic::AtomicU64>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

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
    /// P1 fix: count of currently in-flight task futures. Incremented when
    /// a task future is spawned, decremented when it completes (success /
    /// failure / panic). Used by `wait_for_idle` during daemon shutdown so
    /// we actually drain running tasks instead of unconditionally sleeping
    /// a fixed 200ms and abandoning anything still executing.
    in_flight: Arc<std::sync::atomic::AtomicU64>,
}

impl TaskQueue {
    pub fn new(store: Arc<dyn TaskStore>, overlap: Arc<OverlapController>) -> Self {
        Self {
            store,
            overlap,
            pending: Mutex::new(Vec::new()),
            cancel_flags: Mutex::new(HashMap::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            in_flight: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Current number of in-flight task futures (P1 fix for graceful shutdown).
    pub fn in_flight_count(&self) -> u64 {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Wait for all in-flight tasks to finish, up to `max_wait`. Returns the
    /// number of tasks still running when the deadline expired (0 = fully
    /// drained). Called from daemon shutdown to actually drain running tasks
    /// instead of abandoning them after a fixed sleep.
    pub async fn wait_for_idle(&self, max_wait: std::time::Duration) -> u64 {
        let deadline = tokio::time::Instant::now() + max_wait;
        loop {
            let n = self.in_flight_count();
            if n == 0 {
                return 0;
            }
            if tokio::time::Instant::now() >= deadline {
                return n;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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

    /// Maximum number of pending tasks in the in-memory queue. When this
    /// limit is reached, new enqueues are rejected with an error (back-pressure),
    /// preventing unbounded memory growth during cron storms / external
    /// dispatch floods. Default 10000; tunable via XHJOB_MAX_PENDING.
    const MAX_PENDING_DEFAULT: usize = 10_000;

    /// Enqueue a task by id (already persisted in the store).
    pub async fn enqueue(&self, task_id: &str, priority: i32) -> Result<()> {
        let max_pending = std::env::var("XHJOB_MAX_PENDING")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(Self::MAX_PENDING_DEFAULT);
        let mut g = self.pending.lock().await;
        // P0 fix: bound the pending queue to prevent unbounded growth.
        // If full, return an error — callers (cron scan, retry scan, dispatch)
        // will log a warning and the task remains in the store as Pending;
        // scan_retries will re-enqueue it on the next tick.
        if g.len() >= max_pending {
            return Err(crate::errors::XhjobError::store(format!(
                "pending queue full ({} >= {}); task {} will be retried on next scan",
                g.len(),
                max_pending,
                task_id
            )));
        }
        g.push((priority, task_id.to_string()));
        // Sort by priority descending (higher priority first)
        g.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(())
    }

    /// Drain the next pending task id (highest priority first).
    async fn drain_next(&self) -> Option<String> {
        let mut g = self.pending.lock().await;
        if g.is_empty() {
            return None;
        }
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
                if let Err(e) = self
                    .store
                    .update_state(&task.id, TaskState::Expired, None, Some(now))
                    .await
                {
                    tracing::warn!(task_id = %task.id, error = %e, "update_state to Expired failed");
                }
                // 记录 Expired 事件（A17）—— 终态过期。
                if let Err(e) = self
                    .store
                    .record_event(&task.id, crate::store::EventType::Expired, None, now as i64)
                    .await
                {
                    tracing::warn!(task_id = %task.id, error = %e, "record_event Expired failed");
                }
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
                    if let Err(e) = q.enqueue(&tid, pri).await {
                        tracing::warn!(task_id = %tid, error = %e, "start_date re-enqueue failed");
                    }
                });
                return Ok(());
            }
        }

        // rate_limit 检查（C12）：滑动窗口限流。超限时记录 RateLimited
        // 事件，推进 next_fire（避免 scan_once 重复 enqueue），并延迟
        // window 秒后重新入队。对一次性任务也生效（确保限流期间不丢失）。
        // Reference: Celery rate_limit.
        if !self.rate_limiter.check_and_record(&task, now).await {
            if let Err(e) = self
                .store
                .record_event(
                    &task.id,
                    crate::store::EventType::RateLimited,
                    None,
                    now as i64,
                )
                .await
            {
                tracing::warn!(task_id = %task.id, error = %e, "record_event RateLimited failed");
            }
            // 推进 next_fire 到 window 后（cron/interval 任务避免 scan_once 重复 enqueue）
            let window = task.rate_limit_window;
            if task.cron.is_some() || task.interval.is_some() {
                if let Err(e) = self
                    .store
                    .update_next_fire(&task.id, Some(now + window))
                    .await
                {
                    tracing::warn!(task_id = %task.id, error = %e, "update_next_fire for rate_limit failed");
                }
            }
            // 延迟重新入队
            let q = Arc::clone(&self);
            let tid = task.id.clone();
            let pri = task.priority;
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(window)).await;
                if let Err(e) = q.enqueue(&tid, pri).await {
                    tracing::warn!(task_id = %tid, error = %e, "rate_limit re-enqueue failed");
                }
            });
            return Ok(());
        }

        // Overlap check: re-enqueue instead of dropping, so non-recurring
        // tasks (run_at one-shots, retry-scheduled) are not permanently lost.
        // Recurring tasks re-fire via scan_once; non-recurring have no other
        // trigger. `should_fire` returns false only while a Running instance
        // exists (counter > 0); once it completes (`on_finish` decrements the
        // counter to 0), `should_fire` returns true and the re-enqueued task
        // dispatches. The 200ms delay bounds the spin loop.
        if !self.overlap.should_fire(&self.store, &task).await? {
            let q = Arc::clone(&self);
            let tid = task.id.clone();
            let pri = task.priority;
            tokio::spawn(async move {
                // Short delay to avoid a hot spin loop while the overlapping
                // instance is still running. The overlap counter decrements via
                // on_finish when the running instance completes; this
                // re-enqueue gives the task another chance to dispatch.
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Err(e) = q.enqueue(&tid, pri).await {
                    tracing::warn!(task_id = %tid, error = %e, "overlap re-enqueue failed");
                }
            });
            return Ok(());
        }

        // Mark as RUNNING.
        //
        // P0 fix (overlap counter leak on update_state error): `on_start`
        // MUST run AFTER `update_state(Running)` succeeds, not before. The
        // `?` on `update_state` early-returns on a transient store error
        // (SQLite WAL contention, disk full); if `on_start` had already
        // incremented the overlap counter, `on_finish` would never run (it
        // only runs inside the `task_future` dispatched below, which is never
        // reached on this early-return). For a task with `max_instances=1` +
        // `allow_overlap=false`, a single transient `update_state` failure
        // would permanently leave the counter at 1, so `should_fire` would
        // return false forever and the task could NEVER be dispatched again
        // until daemon restart.
        //
        // Ordering invariant: `should_fire` (called above) is checked before
        // either call, so it reflects prior running instances. Here the task
        // is first marked Running in the store, then counted as overlapping.
        // The `task_future` (which actually runs the task and calls
        // `on_finish` in every completion path: cancel / dispatch-error /
        // normal) is dispatched only after both succeed, so the counter stays
        // balanced: one `on_start` per dispatch, one `on_finish` per
        // completion. The store-based `count_running_instances` check inside
        // `should_fire` covers the brief window between `update_state(Running)`
        // and `on_start` (it returns 1 once the row is Running), preventing a
        // double-dispatch in that window.
        self.store
            .update_state(&task.id, TaskState::Running, Some(now_ts()), None)
            .await?;
        self.overlap.on_start(&task.id).await;
        // Record a `Started` event (A17).
        if let Err(e) = self
            .store
            .record_event(
                &task.id,
                crate::store::EventType::Started,
                None,
                now_ts() as i64,
            )
            .await
        {
            tracing::warn!(task_id = %task.id, error = %e, "record_event Started failed");
        }

        // Dispatch via pool (async by default, thread when XHJOB_POOL_MODE=thread).
        // Thread mode (1:1): each task runs in a dedicated OS worker thread via
        // block_on, concurrency is bounded by thread count (default=num_cpus).
        // Async mode (M:N, default): async tasks on tokio runtime, max concurrency 1024.
        let store = Arc::clone(&self.store);
        let overlap = Arc::clone(&self.overlap);
        let queue_arc = Arc::clone(&self);
        let task_clone = task.clone();
        let task_future = async move {
            // 创建 cancel 标志并注册到 queue，使 handle_cancel_op 能够
            // 通过 signal_cancel 通知 executor 终止子进程。
            let cancel_flag = Arc::new(AtomicBool::new(false));
            queue_arc
                .register_cancel_flag(&task_clone.id, Arc::clone(&cancel_flag))
                .await;

            let result = dispatch_task(
                &task_clone,
                Some(Arc::clone(&cancel_flag)),
                Some(Arc::clone(&store)),
            )
            .await;

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
                        // P2 fix: keep a copy of the original dispatch result
                        // so that if load_result fails (transient SQLite error,
                        // WAL checkpoint conflict, disk hiccup) we fall back to
                        // the real result instead of TaskResult::default().
                        // The previous unwrap_or_default() left all fields None,
                        // which made the success check below use
                        // unwrap_or(true) and could mark a FAILED task (e.g.
                        // HTTP 500) as Success — a state-inconsistency bug.
                        let original = r.clone();
                        if let Err(e) = store.save_result(&task_clone.id, r).await {
                            tracing::warn!(task_id = %task_clone.id, error = %e, "save_result failed");
                        }
                        // Reload the persisted TaskResult so retry policy sees the
                        // same data the success/failure check used.
                        stored = store
                            .load_result(&task_clone.id)
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or(original);
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
            let cancel_requested = store
                .load_task(&task_clone.id)
                .await
                .ok()
                .flatten()
                .map(|t| t.cancel_requested)
                .unwrap_or(false);
            if cancel_requested {
                if let Err(e) = store
                    .update_state(&task_clone.id, TaskState::Cancelled, None, Some(now_ts()))
                    .await
                {
                    tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Cancelled failed");
                }
                // Record a `Cancelled` event (A17).
                if let Err(e) = store
                    .record_event(
                        &task_clone.id,
                        crate::store::EventType::Cancelled,
                        None,
                        now_ts() as i64,
                    )
                    .await
                {
                    tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Cancelled failed");
                }
                crate::utils::metrics::record_cancel();
                overlap.on_finish(&task_clone.id).await;
                return;
            }

            // Dispatch error path: no TaskResult was produced. Consult retry
            // policy with the synthetic result (network error -> retryable).
            if let Some(err_str) = dispatch_err {
                // Watchdog-clobber guard (High-severity): if the watchdog
                // already transitioned this task to `Interrupted` (the
                // executor was hung past `timeout * factor` and the watchdog
                // signalled cancel, which is exactly what caused this
                // dispatch Err("cancelled")), do NOT overwrite that state
                // with `Failed` or schedule a retry. The task is already in
                // its final Interrupted state (the watchdog also recorded a
                // `HungDetected` event) — just balance the overlap counter
                // and return. Without this guard the `update_state(Failed)`
                // / `schedule_retry` calls below would clobber `Interrupted`.
                let already_interrupted = store
                    .load_task(&task_clone.id)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.state == TaskState::Interrupted)
                    .unwrap_or(false);
                if already_interrupted {
                    tracing::info!(
                        task_id = %task_clone.id,
                        "task already Interrupted by watchdog; skipping Failed/retry transition (no clobber)"
                    );
                    crate::utils::metrics::record_failure();
                    overlap.on_finish(&task_clone.id).await;
                    return;
                }
                let synthetic = crate::store::TaskResult::default();
                // acks_on_failure (C13): when false, dispatch errors are
                // retried indefinitely (ignoring retry_max). Same override
                // as the failure path below.
                let effective_retry_max = if task_clone.acks_on_failure {
                    task_clone.retry_max
                } else {
                    u32::MAX
                };
                let policy =
                    crate::retry::RetryPolicy::new(effective_retry_max, task_clone.retry_delay);
                if !policy.should_retry(&task_clone, &synthetic) {
                    if let Err(e) = store
                        .update_state(&task_clone.id, TaskState::Failed, None, Some(now_ts()))
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Failed failed");
                    }
                    if let Err(e) = store
                        .set_attempts_and_error(
                            &task_clone.id,
                            task_clone.attempts.saturating_add(1),
                            Some(format!("not retryable: {}", err_str)),
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "set_attempts_and_error failed");
                    }
                } else {
                    match crate::retry::schedule_retry(
                        &store,
                        &task_clone,
                        err_str,
                        effective_retry_max,
                    )
                    .await
                    {
                        Ok(true) => {
                            // Retry actually scheduled — bump retry counter.
                            crate::utils::metrics::record_retry();
                        }
                        Ok(false) => {
                            // Attempts exhausted — schedule_retry already
                            // marked the task as Failed. No retry counter.
                        }
                        Err(e) => {
                            tracing::warn!(task_id = %task_clone.id, error = %e, "schedule_retry failed");
                        }
                    }
                }
                crate::utils::metrics::record_failure();
                overlap.on_finish(&task_clone.id).await;
                return;
            }

            // Determine success/failure
            let success = match task_clone.task_type {
                crate::store::TaskType::Http => {
                    // 2xx = success
                    stored
                        .status_code
                        .map(|c| (200..300).contains(&c))
                        .unwrap_or(true)
                }
                crate::store::TaskType::Shell => stored.exit_code.map(|c| c == 0).unwrap_or(true),
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
                            if let Err(e) = store
                                .update_state(
                                    &task_clone.id,
                                    TaskState::Success,
                                    None,
                                    Some(finished),
                                )
                                .await
                            {
                                tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Success failed");
                            }
                            // Record a `Succeeded` event (A17) — terminal success.
                            if let Err(e) = store
                                .record_event(
                                    &task_clone.id,
                                    crate::store::EventType::Succeeded,
                                    None,
                                    finished as i64,
                                )
                                .await
                            {
                                tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Succeeded failed");
                            }
                        } else {
                            if let Err(e) = store
                                .update_state(
                                    &task_clone.id,
                                    TaskState::Pending,
                                    None,
                                    Some(finished),
                                )
                                .await
                            {
                                tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Pending failed");
                            }
                            // Record a `Succeeded` event (A17) — non-terminal
                            // success (周期任务将由 scan_once 重新触发)。
                            if let Err(e) = store
                                .record_event(
                                    &task_clone.id,
                                    crate::store::EventType::Succeeded,
                                    None,
                                    finished as i64,
                                )
                                .await
                            {
                                tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Succeeded failed");
                            }
                        }
                    }
                } else {
                    // 非周期任务（包括 run_at 一次性任务）: 立即进入 Success
                    // 终态，然后递增 execution_count 作为记账。原始逻辑保持
                    // 不变。
                    if let Err(e) = store
                        .update_state(&task_clone.id, TaskState::Success, None, Some(finished))
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Success failed");
                    }
                    if let Err(e) = store
                        .record_event(
                            &task_clone.id,
                            crate::store::EventType::Succeeded,
                            None,
                            finished as i64,
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Succeeded failed");
                    }
                    if let Ok(new_count) = store.increment_execution_count(&task_clone.id).await {
                        if task_clone.max_executions > 0 && new_count >= task_clone.max_executions {
                            if let Err(e) = store
                                .update_state(
                                    &task_clone.id,
                                    TaskState::Success,
                                    None,
                                    Some(crate::store::now_ts()),
                                )
                                .await
                            {
                                tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Success failed");
                            }
                        }
                    }
                }
                crate::utils::metrics::record_success();
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
                let policy =
                    crate::retry::RetryPolicy::new(effective_retry_max, task_clone.retry_delay);
                if !policy.should_retry(&task_clone, &stored) {
                    // Not retryable: fail permanently right now.
                    if let Err(e) = store
                        .update_state(&task_clone.id, TaskState::Failed, None, Some(now_ts()))
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "update_state to Failed failed");
                    }
                    if let Err(e) = store
                        .set_attempts_and_error(
                            &task_clone.id,
                            task_clone.attempts.saturating_add(1),
                            Some(format!("not retryable: {}", err_msg)),
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "set_attempts_and_error failed");
                    }
                    if let Err(e) = store
                        .record_event(
                            &task_clone.id,
                            crate::store::EventType::Failed,
                            Some(&format!(
                                "{{\"error\":{}}}",
                                serde_json::to_string(&err_msg).unwrap_or_default()
                            )),
                            now_ts() as i64,
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Failed failed");
                    }
                } else {
                    // Retryable: schedule retry (which itself may
                    // permanently fail if attempts are exhausted).
                    match crate::retry::schedule_retry(
                        &store,
                        &task_clone,
                        err_msg.clone(),
                        effective_retry_max,
                    )
                    .await
                    {
                        Ok(true) => {
                            // Retry actually scheduled — bump retry counter.
                            crate::utils::metrics::record_retry();
                        }
                        Ok(false) => {
                            // Attempts exhausted — schedule_retry already
                            // marked the task as Failed. No retry counter.
                        }
                        Err(e) => {
                            tracing::warn!(task_id = %task_clone.id, error = %e, "schedule_retry failed");
                        }
                    }
                    // Record a `Failed` event (A17) for the individual
                    // attempt; the retry will be processed separately.
                    if let Err(e) = store
                        .record_event(
                            &task_clone.id,
                            crate::store::EventType::Failed,
                            Some(&format!(
                                "{{\"error\":{}}}",
                                serde_json::to_string(&err_msg).unwrap_or_default()
                            )),
                            now_ts() as i64,
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task_clone.id, error = %e, "record_event Failed failed");
                    }
                }
                crate::utils::metrics::record_failure();
            }
            // Chain (C15) + group (C16) + worker_limits (C5 + C8) hooks.
            // Fire on terminal state transitions; the worker_limits counter
            // is incremented for every completion regardless of final state.
            let final_state = store
                .load_task(&task_clone.id)
                .await
                .ok()
                .flatten()
                .map(|t| t.state)
                .unwrap_or(task_clone.state);
            if final_state.is_terminal() {
                notify_chain_and_group(&queue_arc, &store, &task_clone, final_state).await;
            }
            // P0 fix: record task execution counter for worker_limits (C5+C8).
            // The actual limit check + graceful shutdown is done in run()
            // after each iteration — no std::process::exit here.
            if let Some(limits) = crate::utils::limits::worker_limits() {
                limits.record_task_execution();
            }
            overlap.on_finish(&task_clone.id).await;
            // P1 fix: when a rate-limited task reaches a terminal state,
            // release its rate-limit bucket so the per-task HashMap entry
            // does not leak forever. Without this, a long-lived daemon
            // processing many one-shot rate-limited tasks would accumulate
            // dead entries (String + Vec<u64> each) and eventually OOM.
            //
            // IMPORTANT: only clean up on terminal states. Periodic tasks
            // (cron/interval) that succeeded transition back to Pending to
            // be re-fired by scan_once; their rate-limit bucket MUST be
            // retained so the sliding window survives across fires.
            if task_clone.rate_limit_count > 0 && final_state.is_terminal() {
                queue_arc.rate_limiter.forget(&task_clone.id).await;
            }
        };

        // Pool mode selection: async (default) or thread.
        // XHJOB_POOL_MODE=thread → ThreadPool (1:1 OS thread, std::thread + block_on,
        //   bounded by thread count; recommended for CPU-bound tasks)
        // XHJOB_POOL_MODE=async (default) or legacy alias `coroutine`
        //   → async task pool (M:N tokio scheduling, max 1024; recommended for IO-bound)
        let pool_mode = std::env::var("XHJOB_POOL_MODE").unwrap_or_else(|_| "async".to_string());
        // P1 fix: wrap task_future with an in-flight counter so daemon
        // shutdown can actually wait for running tasks to drain instead
        // of sleeping a fixed 200ms. Increment before dispatch, decrement
        // in a finally-style guard (RAII) so panics / early returns still
        // decrement. The guard struct's Drop runs when the future completes.
        let in_flight_counter = Arc::clone(&self.in_flight);
        let counted_future = async move {
            in_flight_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // RAII guard: decrements on drop (success, error, panic, cancel).
            let _guard = InFlightGuard(Arc::clone(&in_flight_counter));
            task_future.await
        };
        if pool_mode == "thread" {
            crate::pool::thread_pool::global().submit(move || {
                if let Some(rt) = coroutine_pool::global_runtime() {
                    rt.block_on(counted_future);
                }
            });
        } else {
            // `async` (recommended) and `coroutine` (legacy alias) both route here.
            drop(coroutine_pool::global().spawn(counted_future));
        }

        Ok(())
    }

    /// Run the queue loop. Pulls pending tasks and processes them.
    /// Stops when `shutdown` becomes true.
    pub async fn run(
        self: Arc<Self>,
        shutdown: tokio::sync::watch::Receiver<bool>,
        shutdown_tx: tokio::sync::watch::Sender<bool>,
    ) {
        let mut shutdown_rx = shutdown;
        let mut retry_scan_ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        retry_scan_ticker.tick().await; // discard first immediate tick
                                        // Store shutdown_tx so record_worker_limits can trigger graceful
                                        // shutdown instead of std::process::exit (which skips Drop).
        let shutdown_tx = Arc::new(shutdown_tx);
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
            // P0 fix: check worker limits after each iteration. If a limit
            // is reached, trigger graceful shutdown via the watch channel
            // (NOT std::process::exit, which would skip Drop).
            // Note: record_task_execution() is called per-task in
            // task_future; here we only read the counters + check memory.
            let limits = match crate::utils::limits::worker_limits() {
                Some(l) => l,
                None => continue,
            };
            let tasks_reached = limits.max_tasks_per_child > 0
                && limits
                    .tasks_executed
                    .load(std::sync::atomic::Ordering::Relaxed)
                    >= limits.max_tasks_per_child;
            let mem_reached = limits.check_memory_limit();
            if !tasks_reached && !mem_reached {
                continue;
            }
            if tasks_reached {
                tracing::info!(
                    limit = limits.max_tasks_per_child,
                    executed = limits
                        .tasks_executed
                        .load(std::sync::atomic::Ordering::Relaxed),
                    "max_tasks_per_child reached, initiating graceful daemon shutdown"
                );
            }
            if mem_reached {
                tracing::info!(
                    limit = limits.max_memory_per_child,
                    "max_memory_per_child reached, initiating graceful daemon shutdown"
                );
            }
            let _ = shutdown_tx.send(true);
            break;
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
            if task.state != TaskState::Pending {
                continue;
            }
            if task.cron.is_some() {
                continue;
            }
            match task.next_fire {
                Some(t) if t <= now => {
                    // P0 fix: surface enqueue errors instead of `let _ =`.
                    if let Err(e) = self.enqueue(&task.id, task.priority).await {
                        tracing::warn!(task_id = %task.id, error = %e, "scan_retries enqueue failed");
                        continue; // keep next_fire so we retry next tick
                    }
                    // Clear next_fire so we don't re-enqueue the same retry multiple times.
                    if let Err(e) = self.store.update_next_fire(&task.id, None).await {
                        tracing::warn!(task_id = %task.id, error = %e, "scan_retries clear next_fire failed");
                    }
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
    if s.is_empty() || s == "null" {
        return None;
    }
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
///
/// Chord (C16+): if the task carries a `chord_id`, refresh the chord
/// state. When all header tasks have succeeded the chord dispatches the
/// callback (inserted into the store by `refresh_state`) and returns its
/// task id here for enqueuing. If any header failed, the chord flips to
/// `partial_failed` and the callback is not dispatched.
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
    // Chord (C16+): refresh chord state on every header-task terminal
    // transition (success or failure — a failure flips the chord to
    // partial_failed so the body is never dispatched).
    if let Some(ref chord_id) = task.chord_id {
        match crate::scheduler::chord::refresh_state(store, chord_id).await {
            Ok(result) => {
                if let Some(callback_id) = result.callback_task_id {
                    // The callback task was already inserted into the store
                    // by refresh_state; load it to recover its priority and
                    // enqueue it on the in-memory queue.
                    match store.load_task(&callback_id).await {
                        Ok(Some(t)) => {
                            if let Err(e) = queue.enqueue(&callback_id, t.priority).await {
                                tracing::warn!(
                                    error = %e, chord_id = %chord_id,
                                    callback_id = %callback_id,
                                    "chord callback enqueue failed"
                                );
                            }
                        }
                        Ok(None) => {
                            tracing::warn!(
                                chord_id = %chord_id,
                                callback_id = %callback_id,
                                "chord callback task not found after refresh_state"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e, chord_id = %chord_id,
                                callback_id = %callback_id,
                                "chord callback load failed"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, chord_id = %chord_id, "chord refresh_state failed");
            }
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
                            v.as_object_mut()
                                .unwrap()
                                .insert("xhjob_chain_id".to_string(), serde_json::json!(chain_id));
                            Some(v.to_string())
                        }
                        _ => Some(serde_json::json!({"xhjob_chain_id": chain_id}).to_string()),
                    }
                }
                _ => Some(serde_json::json!({"xhjob_chain_id": chain_id}).to_string()),
            };
            task.meta = meta_obj;
            // P0-17: propagate owner to chain step task so subsequent
            // ownership checks honor the chain creator's identity. Without
            // this, only the first step (set in handle_chain_op) would be
            // owned; subsequent steps dispatched from queue.rs would be
            // unowned and bypass the default-deny ownership_check.
            task.owner = std::env::var("XHJOB_OWNER").unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        ChainRecord, ChordRecord, EventType, GroupRecord, InMemoryStore, Task, TaskEvent,
        TaskResult, TaskSummary, TaskType, WorkerStats,
    };

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
            if t.state != TaskState::Running {
                break;
            }
            tries += 1;
            if tries > 500 {
                panic!(
                    "task still Running after ~5s of polling; state={:?}",
                    store
                        .load_task("t-cron-max-exec")
                        .await
                        .unwrap()
                        .unwrap()
                        .state
                );
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
            if t.state != TaskState::Running {
                break;
            }
            tries += 1;
            if tries > 500 {
                panic!("task still Running after ~5s of polling");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-ignore-result").await.unwrap().unwrap();
        // State machine still ran: task reached Success terminal.
        assert_eq!(
            final_task.state,
            TaskState::Success,
            "task should reach Success terminal even with ignore_result=true"
        );
        // But no result row was persisted.
        let result = store.load_result("t-ignore-result").await.unwrap();
        assert!(
            result.is_none(),
            "load_result should return None when ignore_result=true (no row saved)"
        );
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
            if t.state != TaskState::Running {
                break;
            }
            tries += 1;
            if tries > 500 {
                panic!("task still Running after ~5s of polling");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-save-result").await.unwrap().unwrap();
        assert_eq!(
            final_task.state,
            TaskState::Success,
            "task should reach Success terminal"
        );

        // Result row WAS persisted.
        let result = store.load_result("t-save-result").await.unwrap();
        assert!(
            result.is_some(),
            "load_result should return Some when ignore_result=false (default)"
        );
        let r = result.unwrap();
        assert_eq!(r.exit_code, Some(0), "exit_code should be 0 for 'echo hi'");
        assert!(
            r.stdout.as_deref().unwrap_or("").contains("hi"),
            "stdout should contain 'hi', got: {:?}",
            r.stdout
        );
    }

    /// InFlightGuard (P1 fix) must decrement the in-flight counter even when
    /// the task future is aborted mid-execution via `JoinHandle::abort()`.
    /// tokio's abort drops the future, which runs the guard's Drop impl,
    /// keeping `in_flight_count` balanced. This guards against a regression
    /// where the counter could leak (never reach 0) if the guard's lifetime
    /// were not tied to the future's drop — which would break
    /// `wait_for_idle` during graceful shutdown (it would wait the full
    /// drain deadline for a counter that never reaches 0).
    ///
    /// The test replicates the exact `counted_future` pattern from
    /// `process_one`: `fetch_add(1)` then wrap in `InFlightGuard` then await
    /// a long future. Aborting the spawned task must bring the counter back
    /// to 0 via the guard's Drop.
    #[tokio::test]
    async fn test_inflight_guard_balanced_on_abort() {
        let store = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(store, overlap));

        // Replicate the counted_future pattern: increment, guard, await.
        // `std::future::pending` never resolves, so the only way the counter
        // decrements is if the guard's Drop runs (triggered by abort).
        let counter = Arc::clone(&queue.in_flight);
        let handle = tokio::spawn(async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _guard = InFlightGuard(Arc::clone(&counter));
            std::future::pending::<()>().await;
        });

        // Wait for the spawned task to start and increment the counter.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            queue.in_flight_count(),
            1,
            "in_flight should be 1 after the task starts"
        );

        // Abort the task future. tokio drops the future, running
        // InFlightGuard::drop which fetch_sub(1)s the counter.
        handle.abort();
        // Give the runtime a moment to process the abort and run Drop.
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(
            queue.in_flight_count(),
            0,
            "in_flight must be 0 after abort (InFlightGuard Drop must run on abort)"
        );
    }

    /// Test-only store wrapper that fails the FIRST `update_state` call
    /// transitioning to `Running`, then delegates every other call (and all
    /// subsequent `update_state` calls) to an inner `InMemoryStore`. Used by
    /// `repro_overlap_counter_balanced_on_update_state_error` to simulate a
    /// transient `update_state` failure (SQLite WAL contention / disk full)
    /// without touching production code.
    struct FailingUpdateStateStore {
        inner: Arc<InMemoryStore>,
        /// When true, the next `update_state(_, Running, ..)` fails; the flag
        /// is consumed (set to false) on that first failure so later calls
        /// delegate normally.
        fail_next_running: Arc<std::sync::atomic::AtomicBool>,
    }

    impl FailingUpdateStateStore {
        fn new() -> Self {
            Self {
                inner: Arc::new(InMemoryStore::new()),
                fail_next_running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            }
        }
    }

    impl TaskStore for FailingUpdateStateStore {
        fn update_state(
            &self,
            id: &str,
            state: TaskState,
            started_at: Option<u64>,
            finished_at: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            // Fail exactly the first transition to Running, then delegate.
            if state == TaskState::Running
                && self
                    .fail_next_running
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Box::pin(async move {
                    Err(crate::errors::XhjobError::store(
                        "simulated transient update_state(Running) failure".to_string(),
                    ))
                });
            }
            self.inner.update_state(id, state, started_at, finished_at)
        }

        fn insert_task(
            &self,
            task: Task,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.insert_task(task)
        }

        fn update_worker_pid(
            &self,
            id: &str,
            worker_pid: Option<u32>,
            starttime: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_worker_pid(id, worker_pid, starttime)
        }

        fn save_result(
            &self,
            task_id: &str,
            result: TaskResult,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.save_result(task_id, result)
        }

        fn load_active_tasks(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>>
        {
            self.inner.load_active_tasks()
        }

        fn load_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>>
        {
            self.inner.load_task(id)
        }

        fn load_result(
            &self,
            id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>,
        > {
            self.inner.load_result(id)
        }

        fn count_running_instances(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
            self.inner.count_running_instances(id)
        }

        fn update_next_fire(
            &self,
            id: &str,
            next_fire: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_next_fire(id, next_fire)
        }

        fn set_attempts_and_error(
            &self,
            id: &str,
            attempts: u32,
            last_error: Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_attempts_and_error(id, attempts, last_error)
        }

        fn delete_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.delete_task(id)
        }

        fn increment_execution_count(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
            self.inner.increment_execution_count(id)
        }

        fn remove_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.remove_task(id)
        }

        fn set_paused(
            &self,
            id: &str,
            paused: bool,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_paused(id, paused)
        }

        fn cancel_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.cancel_task(id)
        }

        fn cleanup_expired_results(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_results()
        }

        fn list_tasks<'a>(
            &'a self,
            state_filter: Option<TaskState>,
            tag_filter: Option<&'a str>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>,
        > {
            self.inner.list_tasks(state_filter, tag_filter)
        }

        fn requeue_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.requeue_task(id)
        }

        fn reschedule_task(
            &self,
            id: &str,
            new_cron: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.reschedule_task(id, new_cron)
        }

        fn reset_running_to_pending(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.reset_running_to_pending()
        }

        fn mark_running_as_interrupted(
            &self,
            reason: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.mark_running_as_interrupted(reason)
        }

        fn record_event(
            &self,
            task_id: &str,
            event_type: EventType,
            payload: Option<&str>,
            ts: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.record_event(task_id, event_type, payload, ts)
        }

        fn list_events(
            &self,
            since_ts: i64,
            task_id_filter: Option<&str>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>>
        {
            self.inner.list_events(since_ts, task_id_filter)
        }

        fn cleanup_expired_events(
            &self,
            ttl_secs: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_events(ttl_secs)
        }

        fn create_chain(
            &self,
            chain_id: &str,
            tasks: &[serde_json::Value],
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_chain(chain_id, tasks, created_at)
        }

        fn get_chain(
            &self,
            chain_id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>,
        > {
            self.inner.get_chain(chain_id)
        }

        fn update_chain_step(
            &self,
            chain_id: &str,
            current_step: u32,
            state: &str,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .update_chain_step(chain_id, current_step, state, updated_at)
        }

        fn list_chains_by_state(
            &self,
            state: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>,
        > {
            self.inner.list_chains_by_state(state)
        }

        fn create_group(
            &self,
            group_id: &str,
            tasks: &[serde_json::Value],
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_group(group_id, tasks, created_at)
        }

        fn get_group(
            &self,
            group_id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>,
        > {
            self.inner.get_group(group_id)
        }

        fn update_group_state(
            &self,
            group_id: &str,
            state: &str,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_group_state(group_id, state, updated_at)
        }

        fn list_groups_by_state(
            &self,
            state: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>,
        > {
            self.inner.list_groups_by_state(state)
        }

        fn create_chord(
            &self,
            id: &str,
            header_task_ids: &[String],
            callback_json: &str,
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .create_chord(id, header_task_ids, callback_json, created_at)
        }

        fn get_chord(
            &self,
            id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<ChordRecord>>> + Send + '_>,
        > {
            self.inner.get_chord(id)
        }

        fn update_chord_state(
            &self,
            id: &str,
            state: &str,
            callback_task_id: Option<String>,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .update_chord_state(id, state, callback_task_id, updated_at)
        }

        fn update_progress(
            &self,
            id: &str,
            percent: u8,
            meta: Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_progress(id, percent, meta)
        }

        fn list_active_summary(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_active_summary()
        }

        fn list_registered_summary(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_registered_summary()
        }

        fn list_scheduled_summary(
            &self,
            now: u64,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_scheduled_summary(now)
        }

        fn worker_stats(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkerStats>> + Send + '_>>
        {
            self.inner.worker_stats()
        }

        fn modify_job(
            &self,
            id: &str,
            patch: &serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.modify_job(id, patch)
        }
    }

    /// Repro for the High-severity overlap-counter leak in `process_one`.
    ///
    /// Bug: `overlap.on_start(&task.id)` was called BEFORE
    /// `store.update_state(..., Running, ...).await?`. When `update_state`
    /// returned Err (transient SQLite WAL contention / disk full),
    /// `process_one` early-returned via `?`, but the overlap counter had
    /// already been incremented and `on_finish` (which only runs inside the
    /// later `task_future`) was never called. For a task with
    /// `max_instances=1` + `allow_overlap=false`, a single transient
    /// `update_state` failure permanently left the counter at 1, so
    /// `should_fire` returned false forever and the task could NEVER be
    /// dispatched again until daemon restart.
    ///
    /// Fix (Option A): `on_start` now runs AFTER `update_state(Running)`
    /// succeeds, so a failed `update_state` never touches the counter.
    ///
    /// This test simulates the transient failure with a `FailingUpdateStateStore`
    /// wrapper that fails the first `update_state(Running)` once, then verifies:
    ///   1. After the failed `process_one`, `should_fire` still returns true
    ///      (counter is 0, not leaked to 1) — the core regression assertion.
    ///   2. The task can still be dispatched on the next `process_one`
    ///      (fail-once flag consumed) and reaches Success terminal — proving
    ///      the task is not permanently stuck (the headline consequence).
    #[tokio::test]
    async fn repro_overlap_counter_balanced_on_update_state_error() {
        let store: Arc<dyn TaskStore> = Arc::new(FailingUpdateStateStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), Arc::clone(&overlap)));

        // max_instances=1 (default) + allow_overlap=false (default): at most 1
        // concurrent instance. A leaked counter of 1 would block all future fires.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-overlap-leak".to_string();
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts());
        store.insert_task(task.clone()).await.unwrap();

        // First process_one: update_state(Running) fails (transient) → Err,
        // propagated via `?`. With the bug this also already ran `on_start`,
        // leaking the counter; with the fix `on_start` never runs.
        queue.enqueue("t-overlap-leak", 0).await.unwrap();
        let res = Arc::clone(&queue).process_one().await;
        assert!(
            res.is_err(),
            "first process_one should propagate the simulated update_state error"
        );

        // CORE ASSERTION: the overlap counter must NOT have leaked.
        // The counter is private, so we observe it via `should_fire`, which
        // takes max(mem_count, store_count). With the bug, `on_start` ran
        // before the failed update_state, leaving mem_count=1 while the store
        // still shows the task Pending (store_count=0) → should_fire returns
        // false (1 >= limit=1). With the fix, on_start never ran, so the
        // counter is 0 and should_fire returns true — the task remains
        // dispatchable.
        let reloaded = store.load_task("t-overlap-leak").await.unwrap().unwrap();
        assert_eq!(
            reloaded.state,
            TaskState::Pending,
            "task must still be Pending after the failed update_state(Running)"
        );
        assert!(
            overlap.should_fire(&store, &reloaded).await.unwrap(),
            "should_fire must return true after the failed update_state — the \
             overlap counter must not leak (was the High-severity bug)"
        );

        // Second process_one: the fail-once flag is consumed, so
        // update_state(Running) succeeds, the task is dispatched, and it
        // should reach Success terminal — proving the task is not permanently
        // stuck after a transient update_state failure.
        queue.enqueue("t-overlap-leak", 0).await.unwrap();
        Arc::clone(&queue).process_one().await.unwrap();

        // Poll until the dispatch completes (state transitions away from
        // Running). Cap at ~5s to avoid hanging the test on a regression.
        let mut tries = 0;
        loop {
            let t = store.load_task("t-overlap-leak").await.unwrap().unwrap();
            if t.state != TaskState::Running {
                break;
            }
            tries += 1;
            if tries > 500 {
                panic!(
                    "task still Running after ~5s of polling; state={:?}",
                    store
                        .load_task("t-overlap-leak")
                        .await
                        .unwrap()
                        .unwrap()
                        .state
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let final_task = store.load_task("t-overlap-leak").await.unwrap().unwrap();
        assert_eq!(
            final_task.state,
            TaskState::Success,
            "task should reach Success terminal after the second (healthy) \
             dispatch — proving it is not permanently stuck by the earlier \
             transient update_state failure"
        );
    }

    /// Repro for the Medium-severity overlap-skip-drop bug in `process_one`.
    ///
    /// Bug: the overlap check `if !should_fire { return Ok(()); }` silently
    /// dropped a task from the in-memory pending queue (already removed by
    /// `drain_next`) without re-enqueueing it. For RECURRING tasks this was
    /// harmless — `scan_once` already advanced `next_fire` and re-fires next
    /// cycle. But for NON-RECURRING tasks:
    ///   - a `run_at` one-shot (whose `next_fire` is the `u64::MAX` sentinel
    ///     after firing), OR
    ///   - a RETRY-scheduled task (whose `next_fire` was cleared to `None` by
    ///     `scan_retries` right after enqueue)
    /// there was no trigger that would ever re-enqueue them. They became
    /// permanently stuck `Pending` with `next_fire=None`/`u64::MAX`, invisible
    /// to both `scan_retries` (needs `next_fire<=now`) and `scan_once` (only
    /// handles cron/interval/run_at; `run_at` with sentinel won't re-fire).
    ///
    /// Fix: when `should_fire` returns false, re-enqueue the task (after a
    /// short 200ms delay) instead of silently dropping it, so the task gets
    /// retried once the overlapping instance finishes and the counter
    /// decrements via `on_finish`.
    ///
    /// This test simulates the scenario with a plain one-shot task (no cron /
    /// interval / run_at) and an in-memory overlap counter manually
    /// incremented via `on_start` (simulating a Running instance). It verifies:
    ///   1. After the skip, the task is re-enqueued (pending queue size == 1),
    ///      NOT dropped.
    ///   2. After `on_finish` decrements the counter, the next `process_one`
    ///      actually dispatches the task (state → Running → Success), proving
    ///      the re-enqueued task is reachable. With the bug the second
    ///      `process_one` would have nothing to drain and the task would stay
    ///      Pending forever.
    #[tokio::test]
    async fn repro_overlap_skip_reenqueues_nonrecurring_task() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), Arc::clone(&overlap)));

        // Plain one-shot task: no cron / interval / run_at. max_instances=1
        // (default) + allow_overlap=false (default): at most 1 concurrent
        // instance. This is exactly the non-recurring shape that the bug
        // permanently lost.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-overlap-skip".to_string();
        task.state = TaskState::Pending;
        store.insert_task(task.clone()).await.unwrap();

        // Simulate a Running instance by manually incrementing the overlap
        // counter. Now `should_fire` returns false (counter 1 >= limit 1).
        overlap.on_start(&task.id).await;
        assert!(
            !overlap.should_fire(&store, &task).await.unwrap(),
            "sanity: with counter=1, should_fire must return false"
        );

        // Enqueue + process_one: drains the task, overlap skip fires.
        queue.enqueue(&task.id, 0).await.unwrap();
        Arc::clone(&queue).process_one().await.unwrap();

        // The task was drained; without the fix the pending queue is empty
        // here (task dropped). With the fix, a re-enqueue is spawned with a
        // 200ms delay. Sleep 300ms to let it fire.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // CORE ASSERTION 1: the task must be back in the pending queue
        // (re-enqueued, not dropped). With the bug this would be 0.
        let pending_len = queue.pending.lock().await.len();
        assert_eq!(
            pending_len, 1,
            "non-recurring task skipped by overlap must be re-enqueued, not dropped"
        );

        // Simulate the overlapping instance finishing: counter back to 0,
        // so should_fire now returns true.
        overlap.on_finish(&task.id).await;
        let reloaded = store.load_task(&task.id).await.unwrap().unwrap();
        assert!(
            overlap.should_fire(&store, &reloaded).await.unwrap(),
            "after on_finish, should_fire must return true again"
        );

        // CORE ASSERTION 2: the re-enqueued task is reachable — the second
        // process_one dispatches it (state → Running). With the bug, the
        // pending queue was empty so process_one would be a no-op and the
        // task would stay Pending forever.
        Arc::clone(&queue).process_one().await.unwrap();
        let after_dispatch = store.load_task(&task.id).await.unwrap().unwrap();
        assert_eq!(
            after_dispatch.state,
            TaskState::Running,
            "re-enqueued task must be dispatched by the second process_one (state Running)"
        );

        // Poll until the dispatch completes (state transitions away from
        // Running). Cap at ~5s to avoid hanging the test on a regression.
        let mut tries = 0;
        loop {
            let t = store.load_task(&task.id).await.unwrap().unwrap();
            if t.state != TaskState::Running {
                break;
            }
            tries += 1;
            if tries > 500 {
                panic!(
                    "task still Running after ~5s of polling; state={:?}",
                    store.load_task(&task.id).await.unwrap().unwrap().state
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let final_task = store.load_task(&task.id).await.unwrap().unwrap();
        assert_eq!(
            final_task.state,
            TaskState::Success,
            "task should reach Success terminal after the re-enqueued dispatch"
        );
    }
}

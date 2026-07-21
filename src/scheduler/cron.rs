//! Cron scheduler (reference: APScheduler CronTrigger).

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use cron::Schedule;
use chrono::TimeZone;
use chrono_tz::Tz;
use crate::errors::{Result, XhjobError};
use crate::store::{TaskState, TaskStore};

/// Timestamp (Unix seconds) of the last `cleanup_expired_results` invocation
/// in `scan_once`. Throttles result-TTL cleanup to at most once per 60s.
static LAST_CLEANUP_TS: AtomicU64 = AtomicU64::new(0);

/// A cron entry tracks one scheduled task.
#[derive(Debug, Clone)]
pub struct CronEntry {
    pub task_id: String,
    pub cron_expr: String,
    pub seconds: bool,
    pub next_fire: u64,
}

/// Validate that `tz_str` parses as a valid IANA timezone (e.g.
/// `Asia/Shanghai`, `America/New_York`). Returns `Ok(())` on success.
pub fn validate_timezone(tz_str: &str) -> Result<()> {
    tz_str.parse::<Tz>()
        .map(|_| ())
        .map_err(|_| XhjobError::Config(format!("invalid timezone: {}", tz_str)))
}

/// Compute the next fire time for a cron expression.
///
/// `cron` 0.12 requires 6 fields (sec min hour day month weekday). If the user
/// supplied 5 fields we prepend a `0` seconds field.
///
/// `timezone` controls how `from_ts` (a Unix timestamp) is interpreted when
/// matching the cron pattern:
/// - `None`: use the system local timezone (`chrono::Local`).
/// - `Some(tz_str)`: parse `tz_str` as an IANA timezone name via `chrono-tz`
///   (e.g. `Asia/Shanghai`). An unparseable string yields
///   `XhjobError::Config("invalid timezone: ...")`.
pub fn next_fire(
    cron_expr: &str,
    from_ts: u64,
    timezone: Option<&str>,
) -> Result<u64> {
    let normalized = if cron_expr.split_whitespace().count() >= 6 {
        cron_expr.to_string()
    } else {
        format!("0 {}", cron_expr)
    };

    let schedule = Schedule::from_str(&normalized)
        .map_err(|e| XhjobError::CronParse(format!("parse '{}': {}", cron_expr, e)))?;

    let next_ts: i64 = match timezone {
        Some(tz_str) => {
            let tz: Tz = tz_str.parse()
                .map_err(|_| XhjobError::Config(format!("invalid timezone: {}", tz_str)))?;
            let from_dt = tz.timestamp_opt(from_ts as i64, 0).single()
                .ok_or_else(|| XhjobError::CronParse(format!("invalid from_ts: {}", from_ts)))?;
            schedule.after(&from_dt).next()
                .ok_or_else(|| XhjobError::CronParse(format!("no future fire time for '{}'", cron_expr)))?
                .timestamp()
        }
        None => {
            let from_dt = chrono::Local.timestamp_opt(from_ts as i64, 0).single()
                .ok_or_else(|| XhjobError::CronParse(format!("invalid from_ts: {}", from_ts)))?;
            schedule.after(&from_dt).next()
                .ok_or_else(|| XhjobError::CronParse(format!("no future fire time for '{}'", cron_expr)))?
                .timestamp()
        }
    };

    Ok(next_ts as u64)
}

/// Default misfire grace window in seconds. Tasks that missed their fire time by
/// more than this many seconds are considered "misfired". With `coalesce=false`
/// such misfires are skipped (the trigger is dropped, next_fire rolls forward).
/// With `coalesce=true` (default) missed triggers are collapsed into one fire.
pub const MISFIRE_GRACE_TIME_SECS: u64 = 60;

/// Decide whether a due cron task (next_fire <= now) should be enqueued or skipped.
///
/// - `coalesce=true`: always fire (collapse N missed triggers into one).
/// - `coalesce=false`: skip if the gap (`now - next_fire`) exceeds the
///   per-job `misfire_grace_time` (or the global default
///   `MISFIRE_GRACE_TIME_SECS` when per-job override is 0).
fn should_fire_due(next_fire: u64, now: u64, coalesce: bool, per_job_grace: u64) -> bool {
    if coalesce {
        return true;
    }
    let grace = if per_job_grace > 0 { per_job_grace } else { MISFIRE_GRACE_TIME_SECS };
    now.saturating_sub(next_fire) <= grace
}

/// Cron scheduler: scans active cron tasks every second and triggers due ones.
pub struct CronScheduler {
    store: Arc<dyn TaskStore>,
}

impl CronScheduler {
    pub fn new(store: Arc<dyn TaskStore>) -> Self {
        Self { store }
    }

    /// Scan active cron tasks once and return the list of task IDs that should fire now.
    pub async fn scan_once(&self) -> Result<Vec<String>> {
        let now = now_ts();
        let active = self.store.load_active_tasks().await?;
        let mut due = Vec::new();
        for task in active {
            // Skip tasks that have reached their max executions limit
            if task.max_executions > 0 && task.execution_count >= task.max_executions {
                // Mark as Success terminal state if not already
                if task.state != TaskState::Success {
                    let _ = self.store.update_state(&task.id, TaskState::Success, None, Some(now_ts())).await;
                }
                continue;
            }
            // Task-level expires (C6): if a task is still Pending (not Running)
            // and expires > 0 and (created_at + expires) < now, transition to
            // Expired terminal state. Running tasks are NOT affected.
            // 该检查位于 start_date / interval / cron 触发逻辑之前，确保
            // 即使 next_fire > now（看似不应触发），只要 expires 窗口已过，
            // 任务也会被置为 Expired 终态，不会被误触发。
            // Reference: APScheduler expires.
            if task.expires > 0 && task.state == TaskState::Pending {
                let expiry_ts = task.created_at.saturating_add(task.expires);
                if now > expiry_ts {
                    let _ = self.store.update_state(
                        &task.id,
                        TaskState::Expired,
                        None,
                        Some(now_ts()),
                    ).await;
                    // 记录 Expired 事件（A17）—— 终态过期。
                    let _ = self.store.record_event(
                        &task.id,
                        crate::store::EventType::Expired,
                        None,
                        now as i64,
                    ).await;
                    continue;
                }
            }
            // Skip paused tasks (cron tick should not fire them).
            // Skip cancel_requested tasks (cancel was requested; do not re-trigger).
            if task.paused || task.cancel_requested {
                continue;
            }
            // Skip if start_date is set and now < start_date (cron tick before start).
            // Reference: APScheduler start_date.
            if let Some(start_ts) = task.start_date {
                if (now as i64) < start_ts {
                    continue;
                }
            }
            // If end_date is set and now > end_date, mark as Success terminal.
            // Reference: APScheduler end_date.
            if let Some(end_ts) = task.end_date {
                if (now as i64) > end_ts {
                    if task.state != TaskState::Success {
                        let _ = self.store.update_state(&task.id, TaskState::Success, None, Some(now_ts())).await;
                    }
                    continue;
                }
            }
            // Scheduling priority: runAt > cron > interval.
            //
            // DateTrigger (runAt): one-shot trigger at the given timestamp.
            // Fires once and immediately transitions to Success terminal state.
            // Reference: APScheduler DateTrigger.
            if let Some(run_at_ts) = task.run_at {
                let next = task.next_fire.unwrap_or(run_at_ts as u64);
                if next <= now {
                    due.push(task.id.clone());
                    // DateTrigger is one-shot: transition to Success terminal.
                    let _ = self.store.update_state(
                        &task.id,
                        TaskState::Success,
                        None,
                        Some(now_ts()),
                    ).await;
                }
                continue;
            }
            // IntervalTrigger (every): fires every `interval` seconds.
            // After firing, next_fire is advanced to now + interval (+ jitter).
            // 如果 next_fire 为 None（任务刚创建尚未触发过，例如被 daemon
            // 立即 enqueue 由 process_one 派发过一次但未推进 next_fire），
            // 使用 created_at + interval 作为首次触发时间。这样首次触发
            // 发生在创建后一个 interval，与 APScheduler IntervalTrigger 的
            // 语义一致，避免任务因 next_fire=None 永远无法被 scan_once
            // 重新触发。
            // Reference: APScheduler IntervalTrigger.
            if let Some(secs) = task.interval {
                if task.cron.is_none() {
                    let next = task.next_fire
                        .unwrap_or_else(|| task.created_at.saturating_add(secs));
                    if next <= now {
                        due.push(task.id.clone());
                        let mut new_next = now + secs;
                        if task.jitter > 0 {
                            new_next += rand_jitter(task.jitter);
                        }
                        let _ = self.store.update_next_fire(&task.id, Some(new_next)).await;
                    }
                    continue;
                }
            }
            if let Some(cron_expr) = &task.cron {
                let tz_ref = task.timezone.as_deref();
                // Check if next_fire is due
                let next = match task.next_fire {
                    Some(t) => t,
                    None => {
                        // Compute next fire if missing
                        match next_fire(cron_expr, now, tz_ref) {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!(
                                    task_id = %task.id,
                                    cron = %cron_expr,
                                    error = %e,
                                    "failed to compute next_fire during scan"
                                );
                                continue;
                            }
                        }
                    }
                };
                if next <= now {
                    let fire = should_fire_due(next, now, task.coalesce, task.misfire_grace_time);
                    if fire {
                        due.push(task.id.clone());
                    } else {
                        let grace = if task.misfire_grace_time > 0 { task.misfire_grace_time } else { MISFIRE_GRACE_TIME_SECS };
                        tracing::debug!(
                            task_id = %task.id,
                            gap_secs = now.saturating_sub(next),
                            grace = grace,
                            "MISFIRE_SKIP (coalesce=false)"
                        );
                        // Record a `Missed` event so listeners can observe the misfire.
                        let _ = self.store.record_event(
                            &task.id,
                            crate::store::EventType::Missed,
                            None,
                            now as i64,
                        ).await;
                    }
                    // Either way, roll next_fire forward to the next occurrence
                    // so we don't keep re-evaluating the stale fire time.
                    if let Ok(mut new_next) = next_fire(cron_expr, now + 1, tz_ref) {
                        // Apply jitter (A9): add a random offset in [0, jitter]
                        // to spread out cron triggers and avoid thundering-herd.
                        if task.jitter > 0 {
                            new_next += rand_jitter(task.jitter);
                        }
                        let _ = self.store.update_next_fire(&task.id, Some(new_next)).await;
                    } else {
                        tracing::warn!(
                            task_id = %task.id,
                            cron = %cron_expr,
                            "failed to roll next_fire forward during scan"
                        );
                    }
                }
            }
        }
        // Periodic cleanup of expired results (throttled to once per 60s).
        // Reference: Celery result_expires auto-cleanup.
        let last = LAST_CLEANUP_TS.load(Ordering::Relaxed);
        if now > last && now - last > 60 {
            let _ = self.store.cleanup_expired_results().await;
            LAST_CLEANUP_TS.store(now, Ordering::Relaxed);
        }
        Ok(due)
    }

    /// Run the scheduler loop. Calls `on_due(task_ids)` for each batch of due tasks.
    /// Stops when `shutdown` becomes true.
    pub async fn run<F>(&self, on_due: F, shutdown: tokio::sync::watch::Receiver<bool>)
    where
        F: Fn(Vec<String>) + Send + Sync + 'static,
    {
        let on_due = Arc::new(on_due);
        let mut shutdown_rx = shutdown;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    match self.scan_once().await {
                        Ok(ids) if !ids.is_empty() => {
                            let on_due = Arc::clone(&on_due);
                            on_due(ids);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("cron scan error: {}", e);
                        }
                    }
                }
                res = shutdown_rx.changed() => {
                    if res.is_err() || *shutdown_rx.borrow() {
                        tracing::info!("cron scheduler shutting down");
                        break;
                    }
                }
            }
        }
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Compute a random jitter offset in `[0, secs]` using `rand::thread_rng()`.
/// Used to spread out cron / interval task triggers and avoid thundering-herd
/// effects when many tasks share the same fire time. Returns 0 when `secs == 0`.
/// Reference: APScheduler jitter.
fn rand_jitter(secs: u64) -> u64 {
    if secs == 0 {
        return 0;
    }
    use rand::Rng;
    rand::thread_rng().gen_range(0..secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use crate::store::{InMemoryStore, Task, TaskType};

    #[test]
    fn coalesce_true_always_fires_even_when_far_behind() {
        // coalesce=true (default): collapse missed triggers into one fire.
        assert!(should_fire_due(0, 0, true, 0));
        assert!(should_fire_due(100, 150, true, 0));
        assert!(should_fire_due(100, 160, true, 0));
        assert!(should_fire_due(100, 10_000, true, 0));
        assert!(should_fire_due(0, 10_000, true, 0));
    }

    #[test]
    fn coalesce_false_fires_within_grace_window() {
        // gap exactly == grace (60s): still fire (boundary is "exceeds").
        assert!(should_fire_due(100, 150, false, 0)); // gap=50
        assert!(should_fire_due(100, 160, false, 0)); // gap=60 == grace
    }

    #[test]
    fn coalesce_false_skips_when_beyond_grace_window() {
        // gap > grace: skip the fire (misfire).
        assert!(!should_fire_due(100, 161, false, 0)); // gap=61 > 60
        assert!(!should_fire_due(0, 10_000, false, 0)); // huge gap
    }

    #[test]
    fn per_job_misfire_grace_time_overrides_global_default() {
        // Per-job grace = 5s: gap=10s > 5s -> misfire.
        assert!(!should_fire_due(100, 110, false, 5));
        // Per-job grace = 30s: gap=20s < 30s -> fire.
        assert!(should_fire_due(100, 120, false, 30));
        // Per-job grace = 0 falls back to global 60s.
        assert!(should_fire_due(100, 160, false, 0));
        assert!(!should_fire_due(100, 161, false, 0));
    }

    /// Build a cron Task with the given next_fire + coalesce, for scan_once tests.
    fn make_cron_task(id: &str, cron_expr: &str, next_fire: u64, coalesce: bool) -> Task {
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo test"}));
        task.id = id.to_string();
        task.cron = Some(cron_expr.to_string());
        task.next_fire = Some(next_fire);
        task.coalesce = coalesce;
        task
    }

    #[tokio::test]
    async fn scan_once_skips_misfired_coalesce_false_task() {
        // Task whose next_fire is far in the past and coalesce=false:
        // scan_once should NOT return it (it gets skipped as misfired),
        // and should roll next_fire forward.
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        // Cron every minute. Far-past next_fire (10 minutes ago).
        let now = now_ts();
        let stale = now.saturating_sub(600);
        let task = make_cron_task("t-misfire", "*/1 * * * *", stale, false);
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert!(due.is_empty(), "expected misfired task to be skipped, got {:?}", due);

        // next_fire should have been rolled forward past `now`.
        let updated = store.load_task("t-misfire").await.unwrap().unwrap();
        let new_next = updated.next_fire.expect("next_fire should be set");
        assert!(new_next > now, "next_fire should be rolled forward past now, got {}", new_next);
    }

    #[tokio::test]
    async fn scan_once_fires_coalesce_true_even_when_far_behind() {
        // Same setup but coalesce=true: should fire (collapse misses into one).
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        let stale = now.saturating_sub(600);
        let task = make_cron_task("t-coalesce", "*/1 * * * *", stale, true);
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert_eq!(due, vec!["t-coalesce".to_string()]);
    }

    #[tokio::test]
    async fn scan_once_fires_coalesce_false_within_grace() {
        // coalesce=false, but only 5s behind: should still fire.
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        let slightly_behind = now.saturating_sub(5);
        let task = make_cron_task("t-grace", "*/1 * * * *", slightly_behind, false);
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert_eq!(due, vec!["t-grace".to_string()]);
    }

    #[tokio::test]
    async fn scan_once_skips_paused_task() {
        // Paused tasks should NOT be fired by the cron tick.
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        let slightly_behind = now.saturating_sub(5);
        let mut task = make_cron_task("t-paused", "*/1 * * * *", slightly_behind, true);
        task.paused = true;
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert!(due.is_empty(), "paused task should be skipped, got {:?}", due);
    }

    #[tokio::test]
    async fn scan_once_skips_cancel_requested_task() {
        // Tasks with cancel_requested=true should NOT be re-triggered by cron.
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        let slightly_behind = now.saturating_sub(5);
        let mut task = make_cron_task("t-cancel-req", "*/1 * * * *", slightly_behind, true);
        task.cancel_requested = true;
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert!(due.is_empty(), "cancel_requested task should be skipped, got {:?}", due);
    }

    #[test]
    fn next_fire_uses_local_timezone() {
        // Sanity: next_fire for "0 9 * * *" from a given local timestamp
        // returns a later timestamp (not the same one). This exercises the
        // Local.timestamp_opt path.
        let now = now_ts();
        let next = next_fire("0 9 * * *", now, None).unwrap();
        assert!(next > now, "next_fire should be in the future: now={} next={}", now, next);
    }

    /// Compute the wall-clock hour:minute:second in `tz_str` for a Unix
    /// timestamp. Used by timezone tests to assert that next_fire actually
    /// lands on 09:00 in the configured timezone.
    fn hms_in_tz(ts: u64, tz_str: &str) -> (u32, u32, u32) {
        let tz: Tz = tz_str.parse().expect("valid tz");
        let dt = tz.timestamp_opt(ts as i64, 0).single().expect("valid ts");
        (dt.hour(), dt.minute(), dt.second())
    }

    #[test]
    fn next_fire_with_explicit_timezone() {
        // cron "0 9 * * *" with timezone Asia/Shanghai: next_fire should land
        // on 09:00:00 in Shanghai time, regardless of the system local zone.
        let now = now_ts();
        let next = next_fire("0 9 * * *", now, Some("Asia/Shanghai"))
            .expect("Asia/Shanghai is a valid tz");
        let (h, m, s) = hms_in_tz(next, "Asia/Shanghai");
        assert_eq!((h, m, s), (9, 0, 0),
            "next_fire in Asia/Shanghai should be 09:00:00, got {:02}:{:02}:{:02}", h, m, s);
        assert!(next > now, "next_fire should be in the future: now={} next={}", now, next);
    }

    #[test]
    fn next_fire_timezone_diff() {
        // Same cron expression "0 9 * * *", two different timezones.
        // The two next_fire timestamps should differ by ~12 or ~13 hours
        // (Shanghai is UTC+8, New York is UTC-5 or UTC-4 with DST).
        let now = now_ts();
        let next_shanghai = next_fire("0 9 * * *", now, Some("Asia/Shanghai"))
            .expect("Asia/Shanghai is a valid tz");
        let next_new_york = next_fire("0 9 * * *", now, Some("America/New_York"))
            .expect("America/New_York is a valid tz");

        // Sanity: both should land on 09:00:00 in their respective zones.
        assert_eq!(hms_in_tz(next_shanghai, "Asia/Shanghai"), (9, 0, 0));
        assert_eq!(hms_in_tz(next_new_york, "America/New_York"), (9, 0, 0));

        // Shanghai is UTC+8, New York is UTC-5 (STD) or UTC-4 (DST).
        // Difference is therefore 12 or 13 hours (in seconds).
        let diff_secs = (next_shanghai as i64 - next_new_york as i64).unsigned_abs();
        let twelve_hours = 12 * 3600u64;
        let thirteen_hours = 13 * 3600u64;
        assert!(
            diff_secs == twelve_hours || diff_secs == thirteen_hours,
            "expected 12h or 13h difference between Shanghai and New York next_fire, got {} secs ({}h)",
            diff_secs, diff_secs / 3600
        );
    }

    #[test]
    fn next_fire_invalid_timezone_returns_error() {
        // An unparseable IANA timezone string should yield a Config error,
        // not a panic and not a silent fallback to Local.
        let now = now_ts();
        let res = next_fire("0 9 * * *", now, Some("Invalid/Zone"));
        match res {
            Err(XhjobError::Config(msg)) => {
                assert!(msg.contains("invalid timezone"), "unexpected message: {}", msg);
                assert!(msg.contains("Invalid/Zone"), "message should mention the bad zone: {}", msg);
            }
            other => panic!("expected Err(Config(...)), got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn next_fire_none_timezone_uses_local() {
        // timezone=None should preserve the legacy Local-based behavior
        // (and not panic). Just ensure it returns a future timestamp.
        let now = now_ts();
        let next = next_fire("0 9 * * *", now, None).expect("None tz should not error");
        assert!(next > now, "next_fire should be in the future: now={} next={}", now, next);
    }

    #[test]
    fn validate_timezone_accepts_known_zones() {
        assert!(validate_timezone("Asia/Shanghai").is_ok());
        assert!(validate_timezone("America/New_York").is_ok());
        assert!(validate_timezone("UTC").is_ok());
        assert!(validate_timezone("Europe/London").is_ok());
    }

    #[test]
    fn validate_timezone_rejects_unknown_zones() {
        assert!(validate_timezone("Invalid/Zone").is_err());
        assert!(validate_timezone("NotAZone").is_err());
        assert!(validate_timezone("").is_err());
    }

    /// IntervalTrigger (every): after a fire, scan_once advances next_fire
    /// to `now + interval`. Subsequent scans do NOT re-fire until the next
    /// interval elapses.
    /// Reference: APScheduler IntervalTrigger.
    #[tokio::test]
    async fn test_interval_trigger_next_fire_advances() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        // Interval task: every 30 seconds. next_fire in the past so the first scan fires.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo i"}));
        task.id = "t-interval".to_string();
        task.interval = Some(30);
        task.next_fire = Some(now.saturating_sub(5)); // due now
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert_eq!(due, vec!["t-interval".to_string()]);

        // After firing, next_fire should advance to roughly now+30.
        let updated = store.load_task("t-interval").await.unwrap().unwrap();
        let new_next = updated.next_fire.expect("next_fire should be set");
        assert!(new_next >= now + 30,
            "next_fire should advance to at least now+30, got {} (now+30={})",
            new_next, now + 30);
        assert!(new_next <= now + 35,
            "next_fire should be near now+30, got {} (now+35={})",
            new_next, now + 35);

        // A second scan immediately after should NOT re-fire (next_fire is in the future).
        let due2 = sched.scan_once().await.unwrap();
        assert!(due2.is_empty(), "expected no re-fire after advancing next_fire, got {:?}", due2);
    }

    /// DateTrigger (runAt): fires once at the given timestamp and immediately
    /// transitions to Success terminal state. Subsequent scans skip it.
    /// Reference: APScheduler DateTrigger.
    #[tokio::test]
    async fn test_run_at_one_shot_terminal() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        // runAt in the past: should fire immediately and go Success terminal.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task.id = "t-runat".to_string();
        task.run_at = Some((now - 5) as i64);
        task.next_fire = Some(now - 5); // due now
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert_eq!(due, vec!["t-runat".to_string()]);

        // After firing, state should be Success (terminal).
        let updated = store.load_task("t-runat").await.unwrap().unwrap();
        assert_eq!(updated.state, TaskState::Success,
            "runAt task should immediately transition to Success after firing");
        assert!(updated.state.is_terminal());

        // A second scan should NOT re-fire (terminal tasks are filtered out
        // by load_active_tasks).
        let due2 = sched.scan_once().await.unwrap();
        assert!(due2.is_empty(), "expected no re-fire for terminal runAt task, got {:?}", due2);
    }

    /// Jitter (A9): when set on an interval task, scan_once advances
    /// next_fire to `now + interval + [0, jitter]`. Verify the new next_fire
    /// falls within `[now + interval, now + interval + jitter]`.
    /// Reference: APScheduler jitter.
    #[tokio::test]
    async fn test_jitter_adds_random_offset_within_range() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        // Interval task every 10s with jitter=5. next_fire in the past so the
        // first scan fires.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo j"}));
        task.id = "t-jitter".to_string();
        task.interval = Some(10);
        task.jitter = 5;
        task.next_fire = Some(now.saturating_sub(5));
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert_eq!(due, vec!["t-jitter".to_string()]);

        let updated = store.load_task("t-jitter").await.unwrap().unwrap();
        let new_next = updated.next_fire.expect("next_fire should be set");
        // Lower bound: now + interval (jitter >= 0).
        // Upper bound: now + interval + jitter (jitter <= jitter).
        let lower = now + 10;
        let upper = now + 10 + 5;
        assert!(
            new_next >= lower && new_next <= upper,
            "next_fire with jitter should be in [{}, {}], got {}",
            lower, upper, new_next
        );
    }

    /// Task expires (C6): when `expires > 0` and a Pending task has been
    /// waiting longer than `expires` seconds (measured from `created_at`),
    /// scan_once transitions it to `Expired` terminal state. Running tasks
    /// are NOT affected.
    /// Reference: APScheduler expires.
    #[tokio::test]
    async fn test_expires_marks_pending_task_expired() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        // Pending task with expires=5, created 100s ago (well past expiry).
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo e"}));
        task.id = "t-expires".to_string();
        task.expires = 5;
        task.created_at = now.saturating_sub(100);
        task.state = TaskState::Pending;
        // next_fire in the future so the only path to terminal is via expires.
        task.next_fire = Some(now + 60);
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        // Should NOT fire (expired instead).
        assert!(due.is_empty(), "expired task should not fire, got {:?}", due);

        let updated = store.load_task("t-expires").await.unwrap().unwrap();
        assert_eq!(updated.state, TaskState::Expired,
            "Pending task past expires should transition to Expired");
        assert!(updated.state.is_terminal(),
            "Expired should be a terminal state");
        assert!(updated.finished_at.is_some(),
            "Expired task should have finished_at set");

        // A second scan should NOT re-process the task (terminal tasks are
        // filtered out by load_active_tasks).
        let due2 = sched.scan_once().await.unwrap();
        assert!(due2.is_empty(), "expired terminal task should not be re-scanned, got {:?}", due2);
    }

    /// Task expires (C6): Running tasks are NOT affected by expires — only
    /// Pending tasks transition to Expired. This guards against accidentally
    /// killing in-flight executions.
    /// Reference: APScheduler expires.
    #[tokio::test]
    async fn test_expires_does_not_affect_running_task() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let now = now_ts();
        // Running task with expires=5, created 100s ago. Should remain Running.
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task.id = "t-expires-running".to_string();
        task.expires = 5;
        task.created_at = now.saturating_sub(100);
        task.state = TaskState::Running;
        task.next_fire = Some(now + 60);
        store.insert_task(task).await.unwrap();

        let sched = CronScheduler::new(Arc::clone(&store));
        let due = sched.scan_once().await.unwrap();
        assert!(due.is_empty(), "running task should not fire, got {:?}", due);

        let updated = store.load_task("t-expires-running").await.unwrap().unwrap();
        assert_eq!(updated.state, TaskState::Running,
            "Running tasks should NOT be expired; only Pending tasks are affected");
    }
}

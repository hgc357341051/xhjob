//! Cron scheduler (reference: APScheduler CronTrigger).

use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use cron::Schedule;
<<<<<<< Updated upstream
use chrono::{TimeZone, Local};
=======
use chrono::TimeZone;
use chrono_tz::Tz;
>>>>>>> Stashed changes
use crate::errors::{Result, XhjobError};
use crate::store::TaskStore;

/// A cron entry tracks one scheduled task.
#[derive(Debug, Clone)]
pub struct CronEntry {
    pub task_id: String,
    pub cron_expr: String,
    pub seconds: bool,
    pub next_fire: u64,
}

<<<<<<< Updated upstream
=======
/// Validate that `tz_str` parses as a valid IANA timezone (e.g.
/// `Asia/Shanghai`, `America/New_York`). Returns `Ok(())` on success.
pub fn validate_timezone(tz_str: &str) -> Result<()> {
    tz_str.parse::<Tz>()
        .map(|_| ())
        .map_err(|_| XhjobError::Config(format!("invalid timezone: {}", tz_str)))
}

>>>>>>> Stashed changes
/// Compute the next fire time for a cron expression.
///
/// `cron` 0.12 requires 6 fields (sec min hour day month weekday). If the user
/// supplied 5 fields we prepend a `0` seconds field.
<<<<<<< Updated upstream
pub fn next_fire(cron_expr: &str, seconds: bool, from_ts: u64) -> Result<u64> {
=======
///
/// `timezone` controls how `from_ts` (a Unix timestamp) is interpreted when
/// matching the cron pattern:
/// - `None`: use the system local timezone (`chrono::Local`).
/// - `Some(tz_str)`: parse `tz_str` as an IANA timezone name via `chrono-tz`
///   (e.g. `Asia/Shanghai`). An unparseable string yields
///   `XhjobError::Config("invalid timezone: ...")`.
pub fn next_fire(
    cron_expr: &str,
    seconds: bool,
    from_ts: u64,
    timezone: Option<&str>,
) -> Result<u64> {
>>>>>>> Stashed changes
    let _ = seconds;
    let normalized = if cron_expr.split_whitespace().count() >= 6 {
        cron_expr.to_string()
    } else {
        format!("0 {}", cron_expr)
    };

    let schedule = Schedule::from_str(&normalized)
        .map_err(|e| XhjobError::CronParse(format!("parse '{}': {}", cron_expr, e)))?;

<<<<<<< Updated upstream
    let from_dt = Local.timestamp_opt(from_ts as i64, 0).single()
        .ok_or_else(|| XhjobError::CronParse(format!("invalid from_ts: {}", from_ts)))?;

    let next = schedule.after(&from_dt).next()
        .ok_or_else(|| XhjobError::CronParse(format!("no future fire time for '{}'", cron_expr)))?;

    Ok(next.timestamp() as u64)
=======
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
>>>>>>> Stashed changes
}

/// Default misfire grace window in seconds. Tasks that missed their fire time by
/// more than this many seconds are considered "misfired". With `coalesce=false`
/// such misfires are skipped (the trigger is dropped, next_fire rolls forward).
/// With `coalesce=true` (default) missed triggers are collapsed into one fire.
pub const MISFIRE_GRACE_TIME_SECS: u64 = 60;

/// Decide whether a due cron task (next_fire <= now) should be enqueued or skipped.
///
/// - `coalesce=true`: always fire (collapse N missed triggers into one).
/// - `coalesce=false`: skip if the gap (`now - next_fire`) exceeds
///   `MISFIRE_GRACE_TIME_SECS`.
fn should_fire_due(next_fire: u64, now: u64, coalesce: bool) -> bool {
    if coalesce {
        return true;
    }
    now.saturating_sub(next_fire) <= MISFIRE_GRACE_TIME_SECS
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
            if let Some(cron_expr) = &task.cron {
<<<<<<< Updated upstream
=======
                let tz_ref = task.timezone.as_deref();
>>>>>>> Stashed changes
                // Check if next_fire is due
                let next = match task.next_fire {
                    Some(t) => t,
                    None => {
                        // Compute next fire if missing
<<<<<<< Updated upstream
                        match next_fire(cron_expr, false, now) {
                            Ok(t) => t,
                            Err(_) => continue,
=======
                        match next_fire(cron_expr, false, now, tz_ref) {
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
>>>>>>> Stashed changes
                        }
                    }
                };
                if next <= now {
                    let fire = should_fire_due(next, now, task.coalesce);
                    if fire {
                        due.push(task.id.clone());
                    } else {
                        tracing::debug!(
                            task_id = %task.id,
                            gap_secs = now.saturating_sub(next),
                            grace = MISFIRE_GRACE_TIME_SECS,
                            "MISFIRE_SKIP (coalesce=false)"
                        );
                    }
                    // Either way, roll next_fire forward to the next occurrence
                    // so we don't keep re-evaluating the stale fire time.
<<<<<<< Updated upstream
                    if let Ok(new_next) = next_fire(cron_expr, false, now + 1) {
                        let _ = self.store.update_next_fire(&task.id, Some(new_next)).await;
=======
                    if let Ok(new_next) = next_fire(cron_expr, false, now + 1, tz_ref) {
                        let _ = self.store.update_next_fire(&task.id, Some(new_next)).await;
                    } else {
                        tracing::warn!(
                            task_id = %task.id,
                            cron = %cron_expr,
                            "failed to roll next_fire forward during scan"
                        );
>>>>>>> Stashed changes
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
<<<<<<< Updated upstream
=======
    use chrono::Timelike;
>>>>>>> Stashed changes
    use crate::store::{InMemoryStore, Task, TaskType};

    #[test]
    fn coalesce_true_always_fires_even_when_far_behind() {
        // coalesce=true (default): collapse missed triggers into one fire.
        assert!(should_fire_due(0, 0, true));
        assert!(should_fire_due(100, 150, true));
        assert!(should_fire_due(100, 160, true));
        assert!(should_fire_due(100, 10_000, true));
        assert!(should_fire_due(0, 10_000, true));
    }

    #[test]
    fn coalesce_false_fires_within_grace_window() {
        // gap exactly == grace (60s): still fire (boundary is "exceeds").
        assert!(should_fire_due(100, 150, false)); // gap=50
        assert!(should_fire_due(100, 160, false)); // gap=60 == grace
    }

    #[test]
    fn coalesce_false_skips_when_beyond_grace_window() {
        // gap > grace: skip the fire (misfire).
        assert!(!should_fire_due(100, 161, false)); // gap=61 > 60
        assert!(!should_fire_due(0, 10_000, false)); // huge gap
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

    #[test]
    fn next_fire_uses_local_timezone() {
        // Sanity: next_fire for "0 9 * * *" from a given local timestamp
        // returns a later timestamp (not the same one). This exercises the
        // Local.timestamp_opt path.
        let now = now_ts();
<<<<<<< Updated upstream
        let next = next_fire("0 9 * * *", false, now).unwrap();
        assert!(next > now, "next_fire should be in the future: now={} next={}", now, next);
    }
=======
        let next = next_fire("0 9 * * *", false, now, None).unwrap();
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
        let next = next_fire("0 9 * * *", false, now, Some("Asia/Shanghai"))
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
        let next_shanghai = next_fire("0 9 * * *", false, now, Some("Asia/Shanghai"))
            .expect("Asia/Shanghai is a valid tz");
        let next_new_york = next_fire("0 9 * * *", false, now, Some("America/New_York"))
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
        let res = next_fire("0 9 * * *", false, now, Some("Invalid/Zone"));
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
        let next = next_fire("0 9 * * *", false, now, None).expect("None tz should not error");
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
>>>>>>> Stashed changes
}

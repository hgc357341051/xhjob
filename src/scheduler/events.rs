//! Task lifecycle event log helpers (A17).
//!
//! Reference: APScheduler `add_listener` + `EVENT_JOB_*` constants.
//!
//! The daemon records `TaskEvent` records at each state transition via the
//! `TaskStore::record_event` trait method. `xhjob_events($since_ts, $task_id)`
//! returns the recent events for client-side inspection / dashboards.
//!
//! Event TTL: a periodic cleanup job (driven by `scan_once`, throttled to
//! 60s) drops events older than `EVENT_TTL_SECS` (default 24h).

use crate::store::{EventType, TaskStore};
use crate::store::now_ts;

/// Default event retention: 24 hours. Older events are auto-cleaned.
pub const EVENT_TTL_SECS: u64 = 24 * 3600;

/// Record a `started` / `succeeded` / `failed` / etc. event for `task_id`.
/// Payload is an optional JSON string with extra context (e.g. error message).
pub async fn record(
    store: &std::sync::Arc<dyn TaskStore>,
    task_id: &str,
    event_type: EventType,
    payload: Option<&str>,
) -> crate::errors::Result<()> {
    store.record_event(task_id, event_type, payload, now_ts() as i64).await
}

/// Delete events older than `EVENT_TTL_SECS`. Returns the number deleted.
pub async fn cleanup_expired(
    store: &std::sync::Arc<dyn TaskStore>,
) -> crate::errors::Result<u64> {
    store.cleanup_expired_events(EVENT_TTL_SECS).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, EventType};

    #[tokio::test]
    async fn test_record_and_list_events() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        record(&store, "t1", EventType::Started, None).await.unwrap();
        record(&store, "t1", EventType::Succeeded, Some(r#"{"exit":0}"#)).await.unwrap();
        record(&store, "t2", EventType::Started, None).await.unwrap();
        // List all events since 0.
        let all = store.list_events(0, None).await.unwrap();
        assert_eq!(all.len(), 3);
        // Filter by task_id.
        let t1 = store.list_events(0, Some("t1")).await.unwrap();
        assert_eq!(t1.len(), 2);
        assert!(t1.iter().all(|e| e.task_id == "t1"));
        // Verify the payload roundtrips.
        let succeeded = t1.iter().find(|e| e.event_type == EventType::Succeeded).unwrap();
        assert_eq!(succeeded.payload.as_deref(), Some(r#"{"exit":0}"#));
    }

    #[tokio::test]
    async fn test_cleanup_expired_events() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        // Insert an event with an old ts.
        let old_ts = now_ts() as i64 - 10_000;
        store.record_event("t-old", EventType::Started, None, old_ts).await.unwrap();
        // Insert a fresh event.
        store.record_event("t-fresh", EventType::Started, None, now_ts() as i64).await.unwrap();
        // Cleanup with EVENT_TTL_SECS = 24h. The old event (10_000s ago) is < 24h,
        // so it should NOT be cleaned up yet.
        let deleted = cleanup_expired(&store).await.unwrap();
        assert_eq!(deleted, 0, "10_000s < 24h TTL -> not deleted");
        // Cleanup with a tiny TTL should drop the old one.
        let deleted = store.cleanup_expired_events(60).await.unwrap();
        assert_eq!(deleted, 1, "old event (>60s) should be deleted");
        // Fresh event still there.
        let remaining = store.list_events(0, None).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].task_id, "t-fresh");
    }
}

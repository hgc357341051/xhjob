//! Minimal internal metrics counters (P0-11).
//!
//! Process-global `AtomicU64` counters for basic daemon observability.
//! No external crates (prometheus, etc.) are used — the snapshot is
//! exposed via the `stats` IPC op and is intended for lightweight
//! health checks / dashboards.

use std::sync::atomic::{AtomicU64, Ordering};

static DISPATCH_COUNT: AtomicU64 = AtomicU64::new(0);
static SUCCESS_COUNT: AtomicU64 = AtomicU64::new(0);
static FAILURE_COUNT: AtomicU64 = AtomicU64::new(0);
static RETRY_COUNT: AtomicU64 = AtomicU64::new(0);
static CANCEL_COUNT: AtomicU64 = AtomicU64::new(0);
static IPC_REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn record_dispatch() {
    DISPATCH_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_success() {
    SUCCESS_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_failure() {
    FAILURE_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_retry() {
    RETRY_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_cancel() {
    CANCEL_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn record_ipc_request() {
    IPC_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "dispatch": DISPATCH_COUNT.load(Ordering::Relaxed),
        "success": SUCCESS_COUNT.load(Ordering::Relaxed),
        "failure": FAILURE_COUNT.load(Ordering::Relaxed),
        "retry": RETRY_COUNT.load(Ordering::Relaxed),
        "cancel": CANCEL_COUNT.load(Ordering::Relaxed),
        "ipc_request": IPC_REQUEST_COUNT.load(Ordering::Relaxed),
    })
}

//! Daemon main entry point: wires together store, IPC, cron scheduler, task queue.

use std::sync::Arc;
use tokio::sync::watch;
use crate::errors::{Result, XhjobError};
use crate::ipc::{self, Request, Response, bind_listener, read_frame, write_frame};
use crate::pool::coroutine_pool;
use crate::store::{self, TaskStore, TaskState};
use crate::scheduler::{CronScheduler, TaskQueue, OverlapController};
use crate::task::TaskBuilder;
use crate::outcome;
use crate::utils::limits::{init_worker_limits, worker_limits};

/// Generate a new random id (UUID v4 style). Uses the `uuid` crate if
/// available; falls back to a timestamp+random composite.
fn new_id() -> String {
    // Use process id + nanos + counter for uniqueness without pulling in
    // another crate. This is good enough for chain/group ids (which are
    // user-inspectable labels, not security-sensitive).
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:016x}{:08x}", nanos, n as u32)
}

/// The daemon entry point. Called by daemon::spawn_daemon(daemon_main) after fork.
pub fn daemon_main() {
    // P0-13: Load the config file (if any) into the process environment
    // BEFORE the tracing subscriber is initialized, so RUST_LOG from the
    // config file is honored. Also before any other env::var reads so all
    // existing std::env::var("XHJOB_") calls pick up config-file values.
    // If the config file doesn't exist (default path /etc/xhjob/config),
    // this is a no-op — fully backward compatible with env-var-only config.
    crate::config::load_config_file();

    // Initialize the global tokio runtime.
    let rt = coroutine_pool::init_global_runtime();

    // Initialize the tracing subscriber BEFORE entering the runtime so that
    // every `tracing::info!` / `tracing::error!` / `tracing::warn!` call in
    // the daemon (and in the runtime's own internals) actually emits output.
    // Without this the global default subscriber is `None` and ALL tracing
    // macros become no-ops — meaning the 77+ `tracing::*` calls sprinkled
    // across daemon_main / queue / cron / ipc would silently drop every
    // log line, making production debugging impossible.
    //
    // `try_init()` is used (not `init()`) because ext-php-rs / other PHP
    // extension init paths may already have installed a subscriber during
    // MINIT; we tolerate that instead of panicking.
    //
    // Env filter: `RUST_LOG=xhjob=info` style. Default level is `info` for
    // the xhjob crate and `warn` for everything else so the daemon log is
    // not flooded with hyper / reqwest noise.
    //
    // P0-15: Log rotation. Instead of writing to stderr (which daemon/unix.rs
    // redirects to a plain append-mode file that grows forever), configure
    // the tracing subscriber to write to a `tracing_appender` rolling daily
    // file. This creates files like `xhjob.default.log.2026-07-22` and
    // rotates once per day, preventing unbounded log growth. The stderr
    // redirect in daemon/unix.rs is kept for panic/crash output only.
    let log_dir = crate::service::current_data_dir()
        .unwrap_or_else(|| "/var/log/xhjob".to_string());
    let _ = std::fs::create_dir_all(&log_dir);
    let service_name = crate::service::current();
    let file_appender = tracing_appender::rolling::daily(
        &log_dir,
        format!("xhjob.{}.log", service_name),
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // Keep the WorkerGuard alive for the daemon's lifetime. If the guard is
    // dropped, the background writer thread shuts down and all subsequent
    // log lines are silently dropped. `std::mem::forget` prevents Drop from
    // running, keeping the worker alive until the process exits.
    std::mem::forget(_guard);
    let _ = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("xhjob=info,warn")),
        )
        .try_init();

    rt.block_on(async {
        if let Err(e) = run_daemon().await {
            tracing::error!("daemon exited with error: {}", e);
            // Even on error, we must cleanup PID file
            #[cfg(unix)]
            crate::daemon::unix::daemon_stopping();
            #[cfg(windows)]
            crate::daemon::windows::daemon_stopping();
            // P0 fix: do NOT call std::process::exit here — it skips Drop
            // for the tokio runtime / IPC listener / in-flight tasks,
            // leaking resources and potentially leaving the socket file on
            // disk. Instead, let the runtime unwind naturally so Drop runs.
            // The daemon process is the top-level binary; returning from
            // block_on exits the process after the runtime is dropped.
            return;
        }
    });
}

async fn run_daemon() -> Result<()> {
    // Announce daemon startup (write PID file already done by spawn_via_double_fork on Unix;
    // on Windows we need to write it here).
    #[cfg(unix)]
    { crate::daemon::unix::daemon_started()?; }
    #[cfg(windows)]
    { crate::daemon::windows::daemon_started()?; }

    let service_name = crate::service::current();
    tracing::info!(
        service = %service_name,
        pid = std::process::id(),
        "xhjob daemon starting"
    );

    // Choose store.
    //
    // When the `persist` cargo feature is enabled, the daemon defaults to
    // SQLite-backed storage so that per-task `persist=true` works out of the
    // box (TaskBuilder.persist field is honored without extra env config).
    // Set XHJOB_PERSIST=0 to explicitly disable (fall back to InMemoryStore).
    //
    // When the feature is NOT compiled in, the binary has no SqliteStore at
    // all, so InMemoryStore is always used regardless of the env var.
    #[cfg(feature = "persist")]
    let use_persist = std::env::var("XHJOB_PERSIST")
        .map(|v| v != "0" && v != "false")
        .unwrap_or(true);
    #[cfg(not(feature = "persist"))]
    let use_persist = std::env::var("XHJOB_PERSIST")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);
    let store: Arc<dyn TaskStore> = if use_persist {
        #[cfg(feature = "persist")]
        {
            let data_dir = crate::service::current_data_dir();
            let path = crate::store::db_path_for(&service_name, data_dir.as_deref());
            // Ensure the data directory exists (user-specified data_dir may not exist yet)
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            tracing::info!(db_path = %path, "opening SQLite store");
            Arc::new(crate::store::SqliteStore::open(&path)?)
        }
        #[cfg(not(feature = "persist"))]
        {
            tracing::warn!("XHJOB_PERSIST=1 but `persist` feature not enabled; falling back to InMemoryStore");
            Arc::new(store::InMemoryStore::new())
        }
    } else {
        Arc::new(store::InMemoryStore::new())
    };

    // On startup, recover active tasks (for persist mode). In-memory store starts empty.
    if use_persist {
        let active = store.load_active_tasks().await?;
        tracing::info!("recovered {} active tasks from store", active.len());
        // acksLate (C10) crash recovery: reset Running tasks with
        // `acks_late=true` to Pending and re-trigger them immediately on the
        // next scan. Must run BEFORE the cron scheduler + task queue are
        // spawned so the reset tasks are visible to the scheduler.
        //
        // Celery semantics: tasks marked `acks_late=true` are re-queued on
        // worker crash; tasks with `acks_late=false` (default) are
        // "acked early" and left in the Running state on restart (must be
        // manually requeued via `xhjob_requeue`). This is a behavior change
        // from the previous loop which unconditionally reset ALL Running
        // tasks — but it matches the Celery `acks_late` contract that this
        // feature introduces.
        // Reference: Celery acks_late.
        let reset = store.reset_running_to_pending().await
            .map_err(|e| {
                tracing::warn!(error = %e, "reset_running_to_pending failed on startup");
                e
            })
            .unwrap_or(0);
        if reset > 0 {
            tracing::info!(
                reset_count = reset,
                "acksLate crash recovery: reset Running tasks with acks_late=true to Pending"
            );
        }
    }

    // Build components
    let overlap = Arc::new(OverlapController::new());
    let queue = Arc::new(TaskQueue::new(Arc::clone(&store), Arc::clone(&overlap)));
    let cron = Arc::new(CronScheduler::new(Arc::clone(&store)));

    // Install worker_limits singleton (C5 + C8). Done before the queue
    // starts so queue.rs can poll the counters after each task completion.
    let _worker_limits = init_worker_limits();

    // Shutdown signal
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Install signal handler (Unix)
    #[cfg(unix)]
    {
        let mut sig_rx = install_unix_signal_handler();
        let shutdown_tx2 = shutdown_tx.clone();
        tokio::spawn(async move {
            loop {
                match sig_rx.recv().await {
                    Some(sig) => {
                        tracing::info!(signal = ?sig, "received signal, shutting down");
                        let _ = shutdown_tx2.send(true);
                        break;
                    }
                    None => break,
                }
            }
        });
    }
    #[cfg(unix)]
    {
        // P0-14: SIGHUP handler for runtime config reload.
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sighup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to install SIGHUP handler");
                    return;
                }
            };
            loop {
                sighup.recv().await;
                tracing::info!("SIGHUP received, reloading config file");
                crate::config::load_config_file();
                // Note: most config values are read at startup and cached.
                // Values that are read per-request (env::var in hot paths) will
                // be picked up automatically. Values read once at startup
                // (e.g. store path) require a daemon restart to take effect.
                tracing::info!("config file reloaded; per-request settings updated");
            }
        });
    }
    #[cfg(windows)]
    {
        // Windows: rely on CTRL_BREAK_EVENT sent by stop()
        let shutdown_tx2 = shutdown_tx.clone();
        tokio::spawn(async move {
            // simple: wait for ctrl_c via tokio
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx2.send(true);
        });
    }

    // Start cron scheduler
    let cron_shutdown_rx = shutdown_rx.clone();
    let queue_for_cron = Arc::clone(&queue);
    let store_for_cron = Arc::clone(&store);
    tokio::spawn(async move {
        cron.run(move |task_ids: Vec<String>| {
            // For each due task, enqueue it
            let queue = Arc::clone(&queue_for_cron);
            let store = Arc::clone(&store_for_cron);
            tokio::spawn(async move {
                for task_id in task_ids {
                    // Look up priority from store
                    let priority = store.load_task(&task_id).await
                        .ok().flatten()
                        .map(|t| t.priority)
                        .unwrap_or(0);
                    if let Err(e) = queue.enqueue(&task_id, priority).await {
                        tracing::warn!(task_id = %task_id, error = %e, "cron callback enqueue failed");
                    }
                }
            });
        }, cron_shutdown_rx).await;
    });

    // Start task queue
    let queue_shutdown_rx = shutdown_rx.clone();
    let queue_shutdown_tx = shutdown_tx.clone();
    let queue_clone = Arc::clone(&queue);
    tokio::spawn(async move {
        queue_clone.run(queue_shutdown_rx, queue_shutdown_tx).await;
    });

    // IPC server loop
    let listener = bind_listener().await?;
    tracing::info!("IPC listener bound");

    let store_ipc = Arc::clone(&store);
    let queue_ipc = Arc::clone(&queue);
    // P0 fix: limit the number of concurrent IPC connections to prevent fd
    // exhaustion / memory amplification from a connection storm. Default
    // 256; tunable via XHJOB_MAX_CONNECTIONS.
    let max_conn = std::env::var("XHJOB_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(256);
    let conn_semaphore = Arc::new(tokio::sync::Semaphore::new(max_conn));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                let stream = match accept_result {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        continue;
                    }
                };
                let store = Arc::clone(&store_ipc);
                let queue = Arc::clone(&queue_ipc);
                let sem = Arc::clone(&conn_semaphore);
                tokio::spawn(async move {
                    // Acquire a permit before handling the connection. If
                    // the connection limit is reached, this waits until a
                    // slot frees up (back-pressure) instead of spawning
                    // unbounded tasks.
                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => {
                            tracing::warn!("connection semaphore closed");
                            return;
                        }
                    };
                    // P0-3: bound the per-connection handler with a timeout so
                    // that a half-closed / malicious / OOM-killed PHP-FPM
                    // worker cannot leave a daemon-side tokio task forever
                    // blocked in read_frame/write_frame, accumulating socket
                    // fds until daemon becomes silently unavailable (ulimit).
                    // 15s is generous enough for any legitimate store op
                    // (including list_events / inspect) while still catching
                    // stuck connections.
                    let conn_timeout = std::time::Duration::from_secs(15);
                    match tokio::time::timeout(
                        conn_timeout,
                        handle_connection(stream, store, queue),
                    ).await {
                        Ok(Ok(())) => {},
                        Ok(Err(e)) => {
                            tracing::debug!(error = %e, "connection handler exited");
                        }
                        Err(_elapsed) => {
                            tracing::warn!(
                                elapsed_secs = conn_timeout.as_secs(),
                                "connection handler timed out, dropping connection"
                            );
                        }
                    }
                });
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("daemon shutdown signal received");
                    break;
                }
            }
        }
    }

    // Cleanup
    let _ = shutdown_tx.send(true);
    // P1 fix: actually drain in-flight tasks instead of a fixed 200ms sleep.
    // The queue tracks in-flight task futures via an AtomicU64 counter
    // (incremented on dispatch, decremented via RAII guard on completion).
    // We wait up to XHJOB_SHUTDOWN_DRAIN_SECS (default 30s) for that counter
    // to reach 0; if tasks are still running at the deadline we log how many
    // were abandoned (they'll be killed when the runtime drops) and proceed.
    // This prevents long-running tasks from being silently truncated by the
    // old fixed 200ms sleep while still bounding shutdown latency.
    let drain_secs = std::env::var("XHJOB_SHUTDOWN_DRAIN_SECS")
        .ok().and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(30);
    let remaining = queue.wait_for_idle(std::time::Duration::from_secs(drain_secs)).await;
    if remaining > 0 {
        tracing::warn!(
            remaining = remaining,
            drain_secs = drain_secs,
            "shutdown drain deadline reached; abandoning still-running tasks"
        );
    } else {
        tracing::info!("all in-flight tasks drained during shutdown");
    }
    #[cfg(unix)]
    crate::daemon::unix::daemon_stopping();
    #[cfg(windows)]
    crate::daemon::windows::daemon_stopping();
    tracing::info!("daemon exited");
    Ok(())
}

async fn handle_connection(
    mut stream: Box<dyn ipc::IpcStream>,
    store: Arc<dyn TaskStore>,
    queue: Arc<TaskQueue>,
) -> Result<()> {
    let req: Request = read_frame(&mut stream).await?;
    // P0 fix: enter an info_span! keyed by op + trace_id so every downstream
    // tracing! line in the handler (and the queue / store / executor it calls)
    // automatically carries the trace_id for cross-process request correlation.
    // Previously we only logged one debug! line with the trace_id at the entry,
    // so any error/warn emitted by a deeper frame lost the correlation.
    let trace_id = req.trace_id.clone().unwrap_or_else(|| "-".to_string());
    let span = tracing::info_span!(
        "ipc",
        op = %req.op,
        id = req.id,
        trace_id = %trace_id,
    );
    let _guard = span.enter();
    tracing::debug!("request");
    crate::utils::metrics::record_ipc_request();
    let resp = match req.op.as_str() {
        "dispatch" => handle_dispatch(&store, &queue, req.payload).await,
        "state" => handle_state_op(&store, req.payload).await,
        "result" => handle_result_op(&store, req.payload).await,
        "remove" => handle_remove_op(&store, req.payload).await,
        "pause" => handle_pause_op(&store, req.payload, true).await,
        "resume" => handle_pause_op(&store, req.payload, false).await,
        "cancel" => handle_cancel_op(&store, &queue, req.payload).await,
        "list" => handle_list_op(&store, req.payload).await,
        "requeue" => handle_requeue_op(&store, req.payload).await,
        "reschedule" => handle_reschedule_op(&store, req.payload).await,
        "get" => handle_get_op(&store, req.payload).await,
        "events" => handle_events_op(&store, req.payload).await,
        "chain" => handle_chain_op(&store, &queue, req.payload).await,
        "chain_state" => handle_chain_state_op(&store, req.payload).await,
        "group" => handle_group_op(&store, &queue, req.payload).await,
        "group_state" => handle_group_state_op(&store, req.payload).await,
        "chord" => handle_chord_op(&store, &queue, req.payload).await,
        "chord_state" => handle_chord_state_op(&store, req.payload).await,
        "report_progress" => handle_report_progress_op(&store, req.payload).await,
        "pull_events" => handle_pull_events_op(&store, req.payload).await,
        "inspect" => handle_inspect_op(&store, req.payload).await,
        "stats" => handle_stats_op(req.payload).await,
        "ping" => Ok(Response::success(req.id, serde_json::json!({"pong": true}))),
        other => Ok(Response::error(req.id, format!("unknown op: {}", other))),
    };
    let resp = match resp {
        Ok(r) => r,
        Err(e) => Response::error(req.id, format!("{}", e)),
    };
    write_frame(&mut stream, &resp).await?;
    Ok(())
}

/// BUG fix (TOCTOU): per-task-id dispatch lock.
///
/// Without this, two concurrent PHP-FPM workers dispatching the same explicit
/// `id` with `replace_existing=false` both see `load_task → None`, both proceed
/// to `insert_task`, and the second silently overwrites the first via
/// `INSERT OR REPLACE` — losing the first task without reporting the conflict.
///
/// We shard a `Mutex<()>` per task id (same pattern as the chord/chain locks
/// in scheduler/) and acquire it around the check-then-insert critical section
/// in `handle_dispatch`. Different ids are not serialized against each other.
static DISPATCH_LOCKS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = std::sync::OnceLock::new();
fn dispatch_locks() -> &'static std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    DISPATCH_LOCKS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}
async fn get_dispatch_lock(task_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let arc = {
        let mut map = dispatch_locks().lock().unwrap();
        map.entry(task_id.to_string()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone()
    };
    arc
}

/// P0-17 fix: default-deny ownership check.
///
/// Returns `Ok(())` if the caller may access `task`, otherwise returns
/// `Ok(Response::error(...))` wrapped in `Err`-free form via the
/// `OwnershipResult` alias. The rule is intentionally strict:
///
/// - If the task has no owner (empty string, e.g. legacy rows written
///   before P0-17) → any caller may access (backward compatible).
/// - If the task has an owner AND the caller's `XHJOB_OWNER` env var is
///   empty → DENY (default-deny: an unauthenticated caller cannot touch
///   owned tasks). This closes the bypass where an attacker simply
///   unsets `XHJOB_OWNER` to access another tenant's tasks.
/// - If both are non-empty and differ → DENY.
/// - If both are non-empty and equal → allow.
///
/// The previous "soft" check (`!task.owner.is_empty() && !owner.is_empty()
/// && task.owner != owner`) let an empty caller owner bypass all checks;
/// this helper makes the empty-caller case deny when the task is owned.
fn ownership_check(task: &crate::store::Task) -> std::result::Result<(), Response> {
    if task.owner.is_empty() {
        // Legacy / unowned task — backward compat: any caller may access.
        return Ok(());
    }
    let caller = std::env::var("XHJOB_OWNER").unwrap_or_default();
    if caller.is_empty() {
        // Default-deny: caller did not identify itself but task is owned.
        return Err(Response::error(0, "ownership: caller has no XHJOB_OWNER but task is owned"));
    }
    if task.owner != caller {
        return Err(Response::error(0, "ownership: task belongs to a different owner"));
    }
    Ok(())
}

/// P0-17 fix: filter a list of TaskSummary by caller ownership.
///
/// Returns a new Vec containing only the summaries the caller may see.
/// The rule mirrors `ownership_check`:
/// - Unowned summaries (legacy rows, owner == "") are always visible
///   (backward compat — single-tenant deployments see everything).
/// - Owned summaries are visible only to a caller whose XHJOB_OWNER
///   matches. If the caller did not set XHJOB_OWNER (empty), all owned
///   summaries are hidden (default-deny).
fn filter_summaries_by_owner(summaries: Vec<crate::store::TaskSummary>) -> Vec<crate::store::TaskSummary> {
    let caller = std::env::var("XHJOB_OWNER").unwrap_or_default();
    summaries.into_iter().filter(|s| {
        // Unowned = visible to anyone (legacy compat).
        if s.owner.is_empty() {
            return true;
        }
        // Owned = visible only to matching caller. Empty caller hides it.
        !caller.is_empty() && s.owner == caller
    }).collect()
}

/// P0-17 fix: filter a list of TaskEvent by the owner of the task each
/// event belongs to. Events don't carry an owner field directly, so we
/// look up each distinct task_id's owner and apply the same default-deny
/// rule as `filter_summaries_by_owner`. Events for unowned tasks (legacy)
/// stay visible; events for owned tasks are visible only to the matching
/// caller. Failed lookups (task already deleted) are treated as unowned
/// (visible) so historical event streams remain queryable.
async fn filter_events_by_owner(
    store: &Arc<dyn TaskStore>,
    events: Vec<crate::store::TaskEvent>,
) -> Vec<crate::store::TaskEvent> {
    let caller = std::env::var("XHJOB_OWNER").unwrap_or_default();
    // Build a distinct set of task_ids and look up each owner once.
    let mut owner_cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for e in &events {
        if owner_cache.contains_key(&e.task_id) {
            continue;
        }
        let owner = match store.load_task(&e.task_id).await {
            Ok(Some(t)) => t.owner,
            _ => String::new(), // deleted / not found → treat as unowned
        };
        owner_cache.insert(e.task_id.clone(), owner);
    }
    events.into_iter().filter(|e| {
        let task_owner = owner_cache.get(&e.task_id).map(|s| s.as_str()).unwrap_or("");
        if task_owner.is_empty() {
            return true; // legacy / unowned — visible to anyone
        }
        !caller.is_empty() && task_owner == caller
    }).collect()
}

async fn handle_dispatch(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    crate::utils::metrics::record_dispatch();
    // payload is a serialized TaskBuilder JSON
    let builder_json = if payload.is_string() {
        payload.as_str().unwrap_or("{}").to_string()
    } else {
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    };
    let builder = TaskBuilder::from_json(&builder_json)?;
    let mut task = builder.build()?;
    // P0-17: set task owner from env for multi-tenant isolation.
    let owner = std::env::var("XHJOB_OWNER").unwrap_or_default();
    task.owner = owner;
    let task_id = task.id.clone();
    let priority = task.priority;
    // BUG fix (TOCTOU): acquire a per-task-id lock around the check-then-insert
    // critical section so two concurrent dispatches of the same explicit `id`
    // with `replace_existing=false` can't both observe "not exists" and then
    // both insert (second silently overwriting the first via INSERT OR REPLACE).
    // Different task ids are NOT serialized against each other — only same-id
    // dispatches are ordered. Lock is released after `insert_task` returns.
    // Bind the Arc to a stable local so the MutexGuard outlives the temporary.
    let _dispatch_arc = get_dispatch_lock(&task_id).await;
    let _dispatch_guard = _dispatch_arc.lock().await;
    // replace_existing (A14): when true and the user supplied an explicit
    // `id`, drop any pre-existing task with the same id before inserting.
    // Mirrors APScheduler's `replace_existing=True` semantics.
    //
    // When false (default): detect id conflict and return an error rather
    // than silently overwriting the existing task. The underlying stores
    // (SqliteStore `INSERT OR REPLACE`, InMemoryStore `HashMap::insert`)
    // would silently overwrite, so the conflict check must happen here at
    // the daemon layer. Reference: APScheduler replace_existing=False raises
    // ConflictingIdError.
    if task.replace_existing {
        if let Err(e) = store.delete_task(&task.id).await {
            tracing::warn!(task_id = %task.id, error = %e, "replace_existing: delete prior task failed");
        }
    } else if let Ok(Some(_existing)) = store.load_task(&task.id).await {
        return Ok(Response::error(0, format!(
            "task id '{}' already exists; use replace_existing=true to overwrite",
            task.id
        )));
    }
    // DateTrigger / countdown delay: when a task is a pure one-shot DateTrigger
    // (run_at set, no cron, no interval) and next_fire lies in the future,
    // do NOT enqueue immediately — let scan_once pick it up at next_fire time.
    // This matches APScheduler DateTrigger and Celery countdown/eta semantics.
    // cron/interval tasks are always enqueued immediately: process_one's
    // next_fire check will skip premature execution and re-enqueue via the
    // deferred-start_date path. Immediate (one-shot, no trigger) tasks have
    // next_fire=None and are enqueued right away.
    let now = store::now_ts();
    let is_pure_date_trigger = task.run_at.is_some()
        && task.cron.is_none()
        && task.interval.is_none();
    let should_defer = is_pure_date_trigger
        && task.next_fire.map(|nf| nf > now).unwrap_or(false);
    store.insert_task(task).await?;
    // Release the dispatch lock before enqueueing — enqueue is idempotent and
    // doesn't need to be inside the critical section, so we don't serialize
    // queue activity across same-id dispatches any longer than necessary.
    drop(_dispatch_guard);
    if !should_defer {
        queue.enqueue(&task_id, priority).await?;
    }
    Ok(Response::success(0, serde_json::json!({"task_id": task_id})))
}

async fn handle_state_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing task_id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    let info = outcome::handle_state(store, task_id).await?;
    let data = serde_json::to_value(&info)
        .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
    Ok(Response::success(0, data))
}

async fn handle_result_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing task_id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    let result = outcome::handle_result(store, task_id).await?;
    let data = serde_json::to_value(&result)
        .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
    Ok(Response::success(0, data))
}

/// Handler for `remove` op: remove a task definition from the store.
/// Reference: APScheduler remove_job.
async fn handle_remove_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    store.remove_task(task_id).await?;
    Ok(Response::success(0, serde_json::json!({"removed": true})))
}

/// Handler for `pause` / `resume` op: set paused flag.
/// `paused=true` for pause, `paused=false` for resume.
/// Reference: APScheduler pause_job / resume_job.
async fn handle_pause_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
    paused: bool,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    store.set_paused(task_id, paused).await?;
    Ok(Response::success(0, serde_json::json!({"paused": paused})))
}

/// Handler for `cancel` op: cancel a task.
/// Pending → Cancelled terminal; Running → cancel_requested=true (no retry, no cron re-trigger).
/// 对 Running 任务，同时通过 `queue.signal_cancel` 通知 executor 立即终止子进程，
/// 避免任务继续运行到自然结束。
/// Reference: Celery revoke (terminate=true).
async fn handle_cancel_op(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    store.cancel_task(task_id).await?;
    // 通知正在执行的 executor 终止子进程。
    // 对 Pending 任务（已被 cancel_task 转 Cancelled 终态）此调用返回 false，无副作用。
    // signal_cancel 返回 bool（是否找到并通知了正在执行的任务），不返回 Result。
    let signaled = queue.signal_cancel(task_id).await;
    if !signaled {
        tracing::debug!(task_id = %task_id, "signal_cancel: no in-flight executor found (task may already be terminal)");
    }
    Ok(Response::success(0, serde_json::json!({"cancelled": true})))
}

/// Handler for `list` op: list all tasks, optionally filtered by state.
/// Reference: APScheduler get_jobs.
async fn handle_list_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let state_filter: Option<String> = payload.get("state_filter")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // 兼容历史的大写输入与新的小写标准（serde 风格，如 "success"/"pending"）。
    let state_filter = match state_filter.as_deref() {
        Some(s) => {
            match s.to_ascii_lowercase().as_str() {
                "pending" => Some(TaskState::Pending),
                "running" => Some(TaskState::Running),
                "interrupted" => Some(TaskState::Interrupted),
                "success" => Some(TaskState::Success),
                "failed" => Some(TaskState::Failed),
                "cancelled" => Some(TaskState::Cancelled),
                "expired" => Some(TaskState::Expired),
                other => return Err(XhjobError::InvalidTask(format!("invalid state_filter: {}", other))),
            }
        }
        None => None,
    };
    let tag_filter: Option<String> = payload.get("tag_filter")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let summaries = store.list_tasks(state_filter, tag_filter.as_deref()).await?;
    // P0-17: filter out tasks owned by other tenants before returning.
    let summaries = filter_summaries_by_owner(summaries);
    let arr: Vec<serde_json::Value> = summaries.iter()
        .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null))
        .collect();
    Ok(Response::success(0, serde_json::json!({"tasks": arr})))
}

/// Handler for `requeue` op: re-queue a terminal task (Cancelled / Failed /
/// Expired) back to Pending so it can be triggered again. Returns
/// `{"requeued": bool}`.
/// Reference: Celery requeue.
async fn handle_requeue_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    let requeued = store.requeue_task(task_id).await?;
    Ok(Response::success(0, serde_json::json!({"requeued": requeued})))
}

/// Handler for `reschedule` op: online modify a cron task's cron expression.
/// Returns `{"rescheduled": bool}`. On invalid cron, returns an error
/// response with message starting with "cron parse: invalid cron".
/// Reference: APScheduler reschedule_job.
async fn handle_reschedule_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    let new_cron = payload.get("cron")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing cron".to_string()))?;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(task_id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    let rescheduled = store.reschedule_task(task_id, new_cron).await?;
    Ok(Response::success(0, serde_json::json!({"rescheduled": rescheduled})))
}

/// Handler for `get` op: fetch a single task definition by id. Returns the
/// full Task JSON (all persisted fields including config, state, and
/// execution metadata). Distinguishes from `state` (which returns the
/// trimmed `StateInfo`) by including all configuration fields such as
/// `retry_max` / `timeout` / `priority` / `allow_overlap` / `max_instances` /
/// `coalesce` / `cron` / `interval` / `run_at` / etc.
///
/// Response shape:
/// - Found: `{"ok": true, "data": <task_json>}`
/// - Not found: `{"ok": false, "error": "not found"}`
///
/// Reference: APScheduler get_job.
async fn handle_get_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    match store.load_task(task_id).await? {
        Some(task) => {
            // P0-17: ownership check — only the task's owner can access it.
            if let Err(resp) = ownership_check(&task) {
                return Ok(resp);
            }
            let data = serde_json::to_value(&task)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
            Ok(Response::success(0, serde_json::json!({"ok": true, "data": data})))
        }
        None => Ok(Response::success(0, serde_json::json!({"ok": false, "error": "not found"}))),
    }
}

/// Handler for `events` op: list task events since `since_ts` (Unix seconds),
/// optionally filtered by `task_id` (A17).
/// Reference: APScheduler EVENT_JOB_*.
async fn handle_events_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let since_ts = payload.get("since_ts")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let task_id_filter: Option<String> = payload.get("task_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(tid) = task_id_filter.as_deref() {
        if let Some(task) = store.load_task(tid).await? {
            if let Err(resp) = ownership_check(&task) {
                return Ok(resp);
            }
        }
    }
    let events = store.list_events(since_ts, task_id_filter.as_deref()).await?;
    // P0-17: when no task_id filter is set, filter events by owner so a
    // tenant cannot see another tenant's task events. When a task_id
    // filter IS set, the per-task ownership check above already enforced
    // access, so we skip the cross-event filter (avoids redundant lookups).
    let events = if task_id_filter.is_some() {
        events
    } else {
        filter_events_by_owner(store, events).await
    };
    let arr: Vec<serde_json::Value> = events.iter()
        .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
        .collect();
    Ok(Response::success(0, serde_json::json!({"events": arr})))
}

/// Handler for `chain` op (C15): create a chain of tasks. Persists a chain
/// record and dispatches the first step. Each step's task stores its
/// `chain_id` in `meta` so the queue can advance the chain after each step
/// completes.
/// Reference: Celery `chain(t1, t2, t3)`.
async fn handle_chain_op(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    let tasks = payload.get("tasks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| XhjobError::ipc("missing or invalid tasks array".to_string()))?;
    if tasks.is_empty() {
        return Err(XhjobError::ipc("chain requires at least one task".to_string()));
    }
    let chain_id = new_id();
    let now = store::now_ts() as i64;
    // Persist the chain record (preserves the user-supplied task configs
    // untouched so chain.rs::advance can build the next step's TaskBuilder
    // from the original config).
    store.create_chain(&chain_id, tasks, now).await?;
    // Dispatch the first step. chain::advance returns the next task config
    // to run and bumps `current_step`. We set the task's `meta` to encode
    // the chain_id so queue.rs can call advance() on the next success.
    let next = crate::scheduler::chain::advance(store, &chain_id).await?
        .ok_or_else(|| XhjobError::store(format!("chain {} produced no first step", chain_id)))?;
    let builder = TaskBuilder::from_json(&next.to_string())?;
    let mut task = builder.build()?;
    // P0-17: propagate owner to chain step task so subsequent ownership
    // checks on this task (state/result/cancel/requeue) honor the chain
    // creator's identity. Without this, chain tasks are unowned and the
    // default-deny ownership_check cannot protect them.
    task.owner = std::env::var("XHJOB_OWNER").unwrap_or_default();
    // Tag the task with the chain_id so queue.rs can advance the chain on
    // success / mark_failed on failure.
    let meta_obj = match task.meta.take() {
        Some(s) if !s.is_empty() && s != "null" => {
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(mut v) if v.is_object() => {
                    v.as_object_mut().unwrap().insert("xhjob_chain_id".to_string(), serde_json::json!(chain_id));
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
    store.insert_task(task).await?;
    queue.enqueue(&task_id, priority).await?;
    Ok(Response::success(0, serde_json::json!({"chain_id": chain_id})))
}

/// Handler for `chain_state` op (C15): inspect a chain record by id.
/// Returns `{"ok": true, "data": <chain_record>}` or `{"ok": false, "error": "not found"}`.
/// Reference: Celery chain inspection.
async fn handle_chain_state_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let chain_id = payload.get("chain_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing chain_id".to_string()))?;
    match store.get_chain(chain_id).await? {
        Some(record) => {
            let data = serde_json::to_value(&record)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
            Ok(Response::success(0, serde_json::json!({"ok": true, "data": data})))
        }
        None => Ok(Response::success(0, serde_json::json!({"ok": false, "error": "not found"}))),
    }
}

/// Handler for `group` op (C16): create a group of tasks. Persists a group
/// record and dispatches all tasks concurrently. Each task stores its
/// `group_id` in `meta` so the queue can refresh the group state on
/// completion.
/// Reference: Celery `group(t1, t2, t3)`.
async fn handle_group_op(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    let tasks = payload.get("tasks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| XhjobError::ipc("missing or invalid tasks array".to_string()))?;
    if tasks.is_empty() {
        return Err(XhjobError::ipc("group requires at least one task".to_string()));
    }
    let group_id = new_id();
    let now = store::now_ts() as i64;
    // Build + persist each task and record its id in the group's task list.
    let mut persisted: Vec<serde_json::Value> = Vec::with_capacity(tasks.len());
    for cfg in tasks {
        let builder = TaskBuilder::from_json(&cfg.to_string())?;
        let mut task = builder.build()?;
        // P0-17: propagate owner to each group member task so subsequent
        // ownership checks honor the group creator's identity.
        task.owner = std::env::var("XHJOB_OWNER").unwrap_or_default();
        // Tag the task with the group_id so queue.rs can refresh the group
        // state on completion.
        let meta_obj = match task.meta.take() {
            Some(s) if !s.is_empty() && s != "null" => {
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(mut v) if v.is_object() => {
                        v.as_object_mut().unwrap().insert("xhjob_group_id".to_string(), serde_json::json!(group_id));
                        Some(v.to_string())
                    }
                    _ => Some(serde_json::json!({"xhjob_group_id": group_id}).to_string()),
                }
            }
            _ => Some(serde_json::json!({"xhjob_group_id": group_id}).to_string()),
        };
        task.meta = meta_obj;
        let task_id = task.id.clone();
        let priority = task.priority;
        store.insert_task(task).await?;
        queue.enqueue(&task_id, priority).await?;
        // Record the dispatched task's id in the group record so
        // group::summarize can inspect each task's live state.
        persisted.push(serde_json::json!({ "id": task_id }));
    }
    store.create_group(&group_id, &persisted, now).await?;
    Ok(Response::success(0, serde_json::json!({"group_id": group_id})))
}

/// Handler for `group_state` op (C16): inspect a group record by id plus a
/// live summary computed from each member task's current state.
/// Returns `{"ok": true, "data": <group_record>, "summary": {total, succeeded, failed, pending}}`
/// or `{"ok": false, "error": "not found"}`.
/// Reference: Celery group inspection.
async fn handle_group_state_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let group_id = payload.get("group_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing group_id".to_string()))?;
    match store.get_group(group_id).await? {
        Some(record) => {
            let (total, succeeded, failed, pending) =
                crate::scheduler::group::summarize(store, group_id).await?;
            let mut data = serde_json::to_value(&record)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("summary".to_string(), serde_json::json!({
                    "total": total,
                    "succeeded": succeeded,
                    "failed": failed,
                    "pending": pending,
                }));
            }
            Ok(Response::success(0, serde_json::json!({"ok": true, "data": data})))
        }
        None => Ok(Response::success(0, serde_json::json!({"ok": false, "error": "not found"}))),
    }
}

/// Handler for `chord` op (C16+): create a chord (header + callback).
///
/// Payload:
///   { "header": [<TaskBuilder JSON>, ...], "callback": <TaskBuilder JSON> }
///
/// Dispatches all header tasks concurrently (each tagged with the chord_id
/// on its `chord_id` field so queue.rs can refresh the chord state on
/// completion), then persists a ChordRecord carrying the serialized
/// callback TaskBuilder. When all header tasks succeed, the chord's
/// `refresh_state` builds the callback task (with `meta` set to the
/// aggregated header results) and returns its id for the queue to enqueue.
///
/// Reference: Celery `chord(header, body)`.
async fn handle_chord_op(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    let header_arr = payload.get("header")
        .and_then(|v| v.as_array())
        .ok_or_else(|| XhjobError::ipc("missing or invalid header array".to_string()))?;
    if header_arr.is_empty() {
        return Err(XhjobError::ipc("chord requires at least one header task".to_string()));
    }
    let callback_json = payload.get("callback")
        .ok_or_else(|| XhjobError::ipc("missing callback".to_string()))?;
    // Validate the callback config parses as a TaskBuilder up front so we
    // can fail the IPC call before dispatching any header tasks.
    let callback_str = serde_json::to_string(callback_json)
        .map_err(|e| XhjobError::ipc(format!("serialize callback: {}", e)))?;
    if let Err(e) = TaskBuilder::from_json(&callback_str) {
        return Err(XhjobError::ipc(format!("invalid callback builder: {}", e)));
    }

    let chord_id = new_id();
    let now = store::now_ts() as i64;
    let mut header_ids: Vec<String> = Vec::with_capacity(header_arr.len());
    for cfg in header_arr {
        let builder = TaskBuilder::from_json(&cfg.to_string())?;
        let mut task = builder.build()?;
        // P0-17: propagate owner to each chord header task so subsequent
        // ownership checks honor the chord creator's identity.
        task.owner = std::env::var("XHJOB_OWNER").unwrap_or_default();
        // Tag the task with the chord_id so queue.rs can refresh the chord
        // state on completion. Uses the dedicated `chord_id` field (no
        // need to mash it into `meta`).
        task.chord_id = Some(chord_id.clone());
        let task_id = task.id.clone();
        let priority = task.priority;
        store.insert_task(task).await?;
        queue.enqueue(&task_id, priority).await?;
        header_ids.push(task_id);
    }
    store.create_chord(&chord_id, &header_ids, &callback_str, now).await?;
    Ok(Response::success(0, serde_json::json!({"chord_id": chord_id})))
}

/// Handler for `chord_state` op (C16+): inspect a chord record by id.
/// Returns `{"ok": true, "data": <chord_record>}` or
/// `{"ok": false, "error": "not found"}`.
/// Reference: Celery chord inspection.
async fn handle_chord_state_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let chord_id = payload.get("chord_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing chord_id".to_string()))?;
    match store.get_chord(chord_id).await? {
        Some(record) => {
            let data = serde_json::to_value(&record)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?;
            Ok(Response::success(0, serde_json::json!({"ok": true, "data": data})))
        }
        None => Ok(Response::success(0, serde_json::json!({"ok": false, "error": "not found"}))),
    }
}

/// Handler for `report_progress` op: update a task's progress percent and
/// optional meta JSON. Returns `{"updated": true}` on success.
/// Reference: Celery update_state(state='PROGRESS', meta=...).
async fn handle_report_progress_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::ipc("missing id".to_string()))?;
    // BUG fix: validate BEFORE casting to u8. The previous `as u8` cast
    // happened before the range check, so values like 256 truncated to 0
    // (bypassing the `> 100` guard) and 356 truncated to 100 (passing the
    // guard as a perfect 100%). Any value of the form `256*k + 0..100`
    // silently bypassed validation. Parse as u64, range-check, then narrow.
    let percent_u64 = payload.get("percent")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if percent_u64 > 100 {
        return Ok(Response::error(0, "percent must be 0-100".to_string()));
    }
    let percent = percent_u64 as u8;
    // P0-17: ownership check — only the task's owner can access it.
    if let Some(task) = store.load_task(id).await? {
        if let Err(resp) = ownership_check(&task) {
            return Ok(resp);
        }
    }
    let meta = payload.get("meta")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    store.update_progress(id, percent, meta).await?;
    Ok(Response::success(0, serde_json::json!({"updated": true})))
}

/// Handler for `pull_events` op: list task events since `since_ts` (Unix
/// seconds), optionally filtered by `event_type` (A17). Returns
/// `{"events": [...]}`.
/// Reference: APScheduler EVENT_JOB_*.
async fn handle_pull_events_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let since_ts = payload.get("since_ts")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let event_type_filter: Option<String> = payload.get("event_type")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let events = store.list_events(since_ts, None).await?;
    // P0-17: filter out events whose task is owned by another tenant.
    let events = filter_events_by_owner(store, events).await;
    let arr: Vec<serde_json::Value> = events.iter()
        .filter(|e| match &event_type_filter {
            Some(f) => e.event_type.as_str() == f.as_str(),
            None => true,
        })
        .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
        .collect();
    Ok(Response::success(0, serde_json::json!({"events": arr})))
}

/// Handler for `inspect` op: aggregate query for daemon state.
/// `mode` selects the query:
///   - `"active"`     — currently running tasks (TaskSummary list)
///   - `"registered"` — cron / interval tasks (TaskSummary list)
///   - `"scheduled"`  — tasks with a future `next_fire` (TaskSummary list)
///   - `"stats"`      — aggregate WorkerStats (default)
/// Returns `{"data": <array_or_object>}`.
/// Reference: Celery inspect active / registered / scheduled / stats.
async fn handle_inspect_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let mode = payload.get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("stats");
    let data = match mode {
        "active" => {
            let tasks = store.list_active_summary().await?;
            // P0-17: filter out tasks owned by other tenants.
            let tasks = filter_summaries_by_owner(tasks);
            serde_json::to_value(&tasks)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?
        }
        "registered" => {
            let tasks = store.list_registered_summary().await?;
            // P0-17: filter out tasks owned by other tenants.
            let tasks = filter_summaries_by_owner(tasks);
            serde_json::to_value(&tasks)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?
        }
        "scheduled" => {
            let now = crate::store::now_ts();
            let tasks = store.list_scheduled_summary(now).await?;
            // P0-17: filter out tasks owned by other tenants.
            let tasks = filter_summaries_by_owner(tasks);
            serde_json::to_value(&tasks)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?
        }
        _ => {
            let stats = store.worker_stats().await?;
            serde_json::to_value(&stats)
                .map_err(|e| XhjobError::ipc(format!("serialize: {}", e)))?
        }
    };
    Ok(Response::success(0, serde_json::json!({"data": data})))
}

/// Handler for `stats` op: snapshot daemon worker stats (C5 + C8).
/// Returns the cumulative task execution count, configured limits, and
/// current process RSS in bytes.
/// Reference: Celery worker stats.
async fn handle_stats_op(_payload: serde_json::Value) -> Result<Response> {
    let mut data = match worker_limits() {
        Some(l) => l.snapshot(),
        None => serde_json::json!({
            "tasks_executed": 0,
            "max_tasks_per_child": 0,
            "max_memory_per_child": 0,
            "current_rss_bytes": 0,
            "note": "worker_limits not initialized",
        }),
    };
    // Merge in the internal metrics counters (P0-11).
    if let Some(obj) = data.as_object_mut() {
        if let Some(metrics) = crate::utils::metrics::snapshot().as_object() {
            for (k, v) in metrics {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Response::success(0, data))
}

#[cfg(unix)]
fn install_unix_signal_handler() -> tokio::sync::mpsc::Receiver<i32> {
    use tokio::signal::unix::{signal, SignalKind};
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT");
        loop {
            tokio::select! {
                _ = sigterm.recv() => { let _ = tx.send(15).await; break; }
                _ = sigint.recv() => { let _ = tx.send(2).await; break; }
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Task, TaskType, TaskSummary, TaskEvent, EventType};

    /// Serialize tests that mutate the `XHJOB_OWNER` env var. `std::env::set_var`
    /// / `remove_var` are process-global and NOT thread-safe — when these
    /// owner-filter tests run in parallel, one test's `remove_var` can fire
    /// between another test's `set_var` and the filter call, producing flaky
    /// failures. This static mutex serializes them so only one owner-test
    /// touches the env at a time. Acquire it as the FIRST line of each test.
    static OWNER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Helper: build a minimal Task with the given owner.
    fn make_task(id: &str, owner: &str) -> Task {
        Task {
            id: id.to_string(),
            task_type: TaskType::Shell,
            payload: serde_json::json!({"cmd": "echo hi"}),
            cron: None,
            retry_max: 0,
            retry_delay: 0,
            timeout: 10,
            priority: 0,
            allow_overlap: false,
            max_instances: 1,
            coalesce: false,
            persist: true,
            proxy: None,
            encoding: None,
            timezone: None,
            state: crate::store::TaskState::Pending,
            attempts: 0,
            next_fire: None,
            created_at: 0,
            started_at: None,
            finished_at: None,
            last_error: None,
            max_executions: 0,
            execution_count: 0,
            paused: false,
            cancel_requested: false,
            start_date: None,
            end_date: None,
            result_ttl: 0,
            meta: None,
            interval: None,
            run_at: None,
            jitter: 0,
            expires: 0,
            retry_backoff: false,
            ignore_result: false,
            acks_late: false,
            soft_timeout: None,
            misfire_grace_time: 0,
            replace_existing: false,
            tags: Vec::new(),
            rate_limit_count: 0,
            rate_limit_window: 0,
            acks_on_failure: true,
            idempotent: false,
            progress: None,
            progress_meta: None,
            owner: owner.to_string(),
            chord_id: None,
        }
    }

    /// ownership_check: empty task owner → allow (legacy backward compat).
    #[test]
    fn test_ownership_check_empty_task_owner_allows_any_caller() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::remove_var("XHJOB_OWNER");
        let task = make_task("t1", "");
        assert!(ownership_check(&task).is_ok());
    }

    /// ownership_check: owned task + empty caller → DENY (default-deny).
    #[test]
    fn test_ownership_check_owned_task_empty_caller_denies() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::remove_var("XHJOB_OWNER");
        let task = make_task("t1", "tenant-a");
        assert!(ownership_check(&task).is_err());
    }

    /// ownership_check: owned task + matching caller → allow.
    #[test]
    fn test_ownership_check_matching_owner_allows() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::set_var("XHJOB_OWNER", "tenant-a");
        let task = make_task("t1", "tenant-a");
        assert!(ownership_check(&task).is_ok());
        std::env::remove_var("XHJOB_OWNER");
    }

    /// ownership_check: owned task + mismatched caller → DENY.
    #[test]
    fn test_ownership_check_mismatched_owner_denies() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::set_var("XHJOB_OWNER", "tenant-a");
        let task = make_task("t1", "tenant-b");
        assert!(ownership_check(&task).is_err());
        std::env::remove_var("XHJOB_OWNER");
    }

    /// filter_summaries_by_owner: empty caller sees only unowned tasks.
    #[test]
    fn test_filter_summaries_by_owner_empty_caller_sees_only_unowned() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::remove_var("XHJOB_OWNER");
        let summaries = vec![
            TaskSummary::from(&make_task("t1", "")),       // unowned → visible
            TaskSummary::from(&make_task("t2", "tenant-a")), // owned → hidden
            TaskSummary::from(&make_task("t3", "tenant-b")), // owned → hidden
        ];
        let filtered = filter_summaries_by_owner(summaries);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "t1");
    }

    /// filter_summaries_by_owner: caller sees own + unowned, not others'.
    #[test]
    fn test_filter_summaries_by_owner_caller_sees_own_and_unowned() {
        let _g = OWNER_ENV_LOCK.lock().unwrap();
        std::env::set_var("XHJOB_OWNER", "tenant-a");
        let summaries = vec![
            TaskSummary::from(&make_task("t1", "")),
            TaskSummary::from(&make_task("t2", "tenant-a")),
            TaskSummary::from(&make_task("t3", "tenant-b")),
        ];
        let filtered = filter_summaries_by_owner(summaries);
        assert_eq!(filtered.len(), 2);
        let ids: Vec<_> = filtered.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"t1"));
        assert!(ids.contains(&"t2"));
        assert!(!ids.contains(&"t3"));
        std::env::remove_var("XHJOB_OWNER");
    }

    /// filter_events_by_owner: async filter — caller sees only own + unowned events.
    #[tokio::test]
    async fn test_filter_events_by_owner_async() {
        std::env::set_var("XHJOB_OWNER", "tenant-a");
        let store: Arc<dyn TaskStore> = Arc::new(crate::store::InMemoryStore::new());
        // Seed tasks with different owners.
        store.insert_task(make_task("owned-a", "tenant-a")).await.unwrap();
        store.insert_task(make_task("owned-b", "tenant-b")).await.unwrap();
        store.insert_task(make_task("unowned", "")).await.unwrap();
        // Build events referencing each task.
        let now = crate::store::now_ts() as i64;
        let events = vec![
            TaskEvent { task_id: "owned-a".to_string(), event_type: EventType::Succeeded, payload: None, ts: now },
            TaskEvent { task_id: "owned-b".to_string(), event_type: EventType::Succeeded, payload: None, ts: now },
            TaskEvent { task_id: "unowned".to_string(), event_type: EventType::Succeeded, payload: None, ts: now },
        ];
        let filtered = filter_events_by_owner(&store, events).await;
        assert_eq!(filtered.len(), 2);
        let task_ids: Vec<_> = filtered.iter().map(|e| e.task_id.as_str()).collect();
        assert!(task_ids.contains(&"owned-a"));
        assert!(task_ids.contains(&"unowned"));
        assert!(!task_ids.contains(&"owned-b"));
        std::env::remove_var("XHJOB_OWNER");
    }
}

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

/// The daemon entry point. Called by daemon::spawn_daemon(daemon_main) after fork.
pub fn daemon_main() {
    // Initialize the global tokio runtime.
    let rt = coroutine_pool::init_global_runtime();
    rt.block_on(async {
        if let Err(e) = run_daemon().await {
            tracing::error!("daemon exited with error: {}", e);
            // Even on error, we must cleanup PID file
            #[cfg(unix)]
            crate::daemon::unix::daemon_stopping();
            #[cfg(windows)]
            crate::daemon::windows::daemon_stopping();
            std::process::exit(1);
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

    // Choose store
    let use_persist = std::env::var("XHJOB_PERSIST").map(|v| v == "1" || v == "true").unwrap_or(false);
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
                    let _ = queue.enqueue(&task_id, priority).await;
                }
            });
        }, cron_shutdown_rx).await;
    });

    // Start task queue
    let queue_shutdown_rx = shutdown_rx.clone();
    let queue_clone = Arc::clone(&queue);
    tokio::spawn(async move {
        queue_clone.run(queue_shutdown_rx).await;
    });

    // IPC server loop
    let listener = bind_listener().await?;
    tracing::info!("IPC listener bound");

    let store_ipc = Arc::clone(&store);
    let queue_ipc = Arc::clone(&queue);

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
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, store, queue).await {
                        tracing::debug!(error = %e, "connection handler exited");
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
    // give workers a moment to drain
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
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
    tracing::debug!(op = %req.op, id = req.id, "request");
    let resp = match req.op.as_str() {
        "dispatch" => handle_dispatch(&store, &queue, req.payload).await,
        "state" => handle_state_op(&store, req.payload).await,
        "result" => handle_result_op(&store, req.payload).await,
        "remove" => handle_remove_op(&store, req.payload).await,
        "pause" => handle_pause_op(&store, req.payload, true).await,
        "resume" => handle_pause_op(&store, req.payload, false).await,
        "cancel" => handle_cancel_op(&store, req.payload).await,
        "list" => handle_list_op(&store, req.payload).await,
        "requeue" => handle_requeue_op(&store, req.payload).await,
        "reschedule" => handle_reschedule_op(&store, req.payload).await,
        "get" => handle_get_op(&store, req.payload).await,
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

async fn handle_dispatch(
    store: &Arc<dyn TaskStore>,
    queue: &Arc<TaskQueue>,
    payload: serde_json::Value,
) -> Result<Response> {
    // payload is a serialized TaskBuilder JSON
    let builder_json = if payload.is_string() {
        payload.as_str().unwrap_or("{}").to_string()
    } else {
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    };
    let builder = TaskBuilder::from_json(&builder_json)?;
    let task = builder.build()?;
    let task_id = task.id.clone();
    let priority = task.priority;
    store.insert_task(task).await?;
    queue.enqueue(&task_id, priority).await?;
    Ok(Response::success(0, serde_json::json!({"task_id": task_id})))
}

async fn handle_state_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::Ipc("missing task_id".to_string()))?;
    let info = outcome::handle_state(store, task_id).await?;
    let data = serde_json::to_value(&info)
        .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
    Ok(Response::success(0, data))
}

async fn handle_result_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("task_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::Ipc("missing task_id".to_string()))?;
    let result = outcome::handle_result(store, task_id).await?;
    let data = serde_json::to_value(&result)
        .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
    store.set_paused(task_id, paused).await?;
    Ok(Response::success(0, serde_json::json!({"paused": paused})))
}

/// Handler for `cancel` op: cancel a task.
/// Pending → Cancelled terminal; Running → cancel_requested=true (no retry, no cron re-trigger).
/// Reference: Celery revoke.
async fn handle_cancel_op(
    store: &Arc<dyn TaskStore>,
    payload: serde_json::Value,
) -> Result<Response> {
    let task_id = payload.get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
    store.cancel_task(task_id).await?;
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
    let state_filter = match state_filter.as_deref() {
        Some("PENDING") => Some(TaskState::Pending),
        Some("RUNNING") => Some(TaskState::Running),
        Some("INTERRUPTED") => Some(TaskState::Interrupted),
        Some("SUCCESS") => Some(TaskState::Success),
        Some("FAILED") => Some(TaskState::Failed),
        Some("CANCELLED") => Some(TaskState::Cancelled),
        Some("EXPIRED") => Some(TaskState::Expired),
        Some(other) => return Err(XhjobError::InvalidTask(format!("invalid state_filter: {}", other))),
        None => None,
    };
    let summaries = store.list_tasks(state_filter).await?;
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
    let new_cron = payload.get("cron")
        .and_then(|v| v.as_str())
        .ok_or_else(|| XhjobError::Ipc("missing cron".to_string()))?;
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
    match store.load_task(task_id).await? {
        Some(task) => {
            let data = serde_json::to_value(&task)
                .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
            Ok(Response::success(0, serde_json::json!({"ok": true, "data": data})))
        }
        None => Ok(Response::success(0, serde_json::json!({"ok": false, "error": "not found"}))),
    }
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

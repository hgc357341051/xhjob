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
        let _ = store.delete_task(&task.id).await;
    } else if let Ok(Some(_existing)) = store.load_task(&task.id).await {
        return Ok(Response::error(0, format!(
            "task id '{}' already exists; use replace_existing=true to overwrite",
            task.id
        )));
    }
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
        .ok_or_else(|| XhjobError::Ipc("missing id".to_string()))?;
    store.cancel_task(task_id).await?;
    // 通知正在执行的 executor 终止子进程。
    // 对 Pending 任务（已被 cancel_task 转 Cancelled 终态）此调用返回 false，无副作用。
    let _ = queue.signal_cancel(task_id).await;
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
    let events = store.list_events(since_ts, task_id_filter.as_deref()).await?;
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
        .ok_or_else(|| XhjobError::Ipc("missing or invalid tasks array".to_string()))?;
    if tasks.is_empty() {
        return Err(XhjobError::Ipc("chain requires at least one task".to_string()));
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
        .ok_or_else(|| XhjobError::Store(format!("chain {} produced no first step", chain_id)))?;
    let builder = TaskBuilder::from_json(&next.to_string())?;
    let mut task = builder.build()?;
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
        .ok_or_else(|| XhjobError::Ipc("missing chain_id".to_string()))?;
    match store.get_chain(chain_id).await? {
        Some(record) => {
            let data = serde_json::to_value(&record)
                .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
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
        .ok_or_else(|| XhjobError::Ipc("missing or invalid tasks array".to_string()))?;
    if tasks.is_empty() {
        return Err(XhjobError::Ipc("group requires at least one task".to_string()));
    }
    let group_id = new_id();
    let now = store::now_ts() as i64;
    // Build + persist each task and record its id in the group's task list.
    let mut persisted: Vec<serde_json::Value> = Vec::with_capacity(tasks.len());
    for cfg in tasks {
        let builder = TaskBuilder::from_json(&cfg.to_string())?;
        let mut task = builder.build()?;
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
        .ok_or_else(|| XhjobError::Ipc("missing group_id".to_string()))?;
    match store.get_group(group_id).await? {
        Some(record) => {
            let (total, succeeded, failed, pending) =
                crate::scheduler::group::summarize(store, group_id).await?;
            let mut data = serde_json::to_value(&record)
                .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
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

/// Handler for `stats` op: snapshot daemon worker stats (C5 + C8).
/// Returns the cumulative task execution count, configured limits, and
/// current process RSS in bytes.
/// Reference: Celery worker stats.
async fn handle_stats_op(_payload: serde_json::Value) -> Result<Response> {
    let data = match worker_limits() {
        Some(l) => l.snapshot(),
        None => serde_json::json!({
            "tasks_executed": 0,
            "max_tasks_per_child": 0,
            "max_memory_per_child": 0,
            "current_rss_bytes": 0,
            "note": "worker_limits not initialized",
        }),
    };
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

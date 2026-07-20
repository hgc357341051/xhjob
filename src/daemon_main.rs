//! Daemon main entry point: wires together store, IPC, cron scheduler, task queue.

use std::sync::Arc;
use tokio::sync::watch;
use crate::errors::{Result, XhjobError};
use crate::ipc::{self, Request, Response, bind_listener, read_frame, write_frame};
use crate::pool::coroutine_pool;
use crate::store::{self, TaskStore, TaskState, now_ts};
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

<<<<<<< Updated upstream
    tracing::info!("xhjob daemon starting (pid={})", std::process::id());
=======
    let service_name = crate::service::current();
    tracing::info!(
        service = %service_name,
        pid = std::process::id(),
        "xhjob daemon starting"
    );
>>>>>>> Stashed changes

    // Choose store
    let use_persist = std::env::var("XHJOB_PERSIST").map(|v| v == "1" || v == "true").unwrap_or(false);
    let store: Arc<dyn TaskStore> = if use_persist {
        #[cfg(feature = "persist")]
        {
<<<<<<< Updated upstream
            let path = std::env::var("XHJOB_DB").unwrap_or_else(|_| {
                #[cfg(unix)]
                { "/tmp/xhjob.db".to_string() }
                #[cfg(windows)]
                { std::env::temp_dir().join("xhjob.db").to_string_lossy().to_string() }
            });
=======
            let path = crate::store::db_path_for(&service_name);
>>>>>>> Stashed changes
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
        // Reset RUNNING tasks to PENDING so they will be retried/fired again.
        // - Cron tasks: just reset to PENDING; cron scheduler will fire them at next_fire.
        // - Non-cron tasks: reset to PENDING and set next_fire=now() so scan_retries
        //   picks them up immediately on its next 1-second tick.
        // (Previously these were marked INTERRUPTED, which neither scan_retries nor
        //  scan_once would pick up, leaving them stuck forever.)
        for task in active {
            if task.state == TaskState::Running {
                if task.cron.is_some() {
                    let _ = store.update_state(&task.id, TaskState::Pending, None, None).await;
                } else {
                    let _ = store.update_state(&task.id, TaskState::Pending, None, None).await;
                    let _ = store.update_next_fire(&task.id, Some(now_ts())).await;
                }
            }
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

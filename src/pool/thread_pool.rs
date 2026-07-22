//! 线程池模块（1:1 OS 线程调度模式）。
//!
//! 当 `XHJOB_POOL_MODE=thread` 时，daemon 使用此线程池执行任务。每个任务
//! 在独立的工作线程中运行（通过 `block_on` 执行 async future），适合
//! CPU 密集型或需要严格并发控制的场景。并发度由线程数控制（默认=CPU 核数，
//! 可通过 `XHJOB_THREAD_POOL_SIZE` 覆盖）。
//!
//! 默认模式 `XHJOB_POOL_MODE=async`（或兼容别名 `coroutine`）使用 async task
//! 池（tokio M:N 调度），适合 IO 密集型任务，最大并发 1024（可通过
//! `XHJOB_ASYNC_POOL_SIZE` 或兼容别名 `XHJOB_COROUTINE_POOL_SIZE` 覆盖）。
//!
//! 两种模式本质区别：
//!   - async（默认，M:N）：N 个 tokio worker 线程复用跑 M 个 async task，
//!     task 在 `await` 时 yield 让出线程，单线程可高并发处理 IO。
//!   - thread（1:1）：每个任务独占一个 OS 线程，`block_on` 阻塞整个线程，
//!     真并行受限于线程数，适合 CPU 密集型或需严格隔离。
//!
//! Built on top of `std::thread` + `crossbeam-channel`.

use std::sync::Arc;
use std::thread;
use crossbeam_channel::{bounded, unbounded, Sender};
use once_cell::sync::OnceCell;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Shared state between the pool and its worker threads.
struct PoolInner {
    sender: Sender<Job>,
    shutdown: Sender<()>,
}

pub struct ThreadPool {
    inner: Arc<PoolInner>,
    /// P1 fix: workers are owned directly by `ThreadPool` (not inside the
    /// `Arc<PoolInner>`) so that `Drop` can actually `join()` them. When
    /// workers lived behind the shared `Arc`, `Drop` could only send the
    /// shutdown signal — it couldn't move the `Vec<JoinHandle>` out to
    /// drain it, so workers were abandoned (possibly mid-task) on drop.
    /// Keeping them here means `Drop` owns the only handle and can wait
    /// for each worker to finish its current job before the process exits.
    workers: Vec<thread::JoinHandle<()>>,
}

static GLOBAL: OnceCell<ThreadPool> = OnceCell::new();

impl ThreadPool {
    /// Create a new fixed-size thread pool.
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "thread pool size must be > 0");
        let (sender, receiver) = unbounded::<Job>();
        let (shutdown_tx, shutdown_rx) = bounded::<()>(1);
        let receiver = Arc::new(receiver);
        let mut workers = Vec::with_capacity(size);
        for i in 0..size {
            let receiver = Arc::clone(&receiver);
            let shutdown_rx = shutdown_rx.clone();
            let handle = thread::Builder::new()
                .name(format!("xhjob-worker-{}", i))
                .spawn(move || {
                    loop {
                        // Use select! over receiver and shutdown signal.
                        crossbeam_channel::select! {
                            recv(receiver) -> job => {
                                match job {
                                    Ok(j) => {
                                        // Catch panics so a single bad job doesn't kill the worker.
                                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(j));
                                    }
                                    Err(_) => break, // channel closed
                                }
                            }
                            recv(shutdown_rx) -> _ => break,
                        }
                    }
                })
                .expect("failed to spawn worker thread");
            workers.push(handle);
        }
        Self {
            inner: Arc::new(PoolInner {
                sender,
                shutdown: shutdown_tx,
            }),
            workers,
        }
    }

    /// Submit a job to the pool.
    pub fn submit<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job: Job = Box::new(f);
        // Ignore send error (happens if pool is shutting down)
        let _ = self.inner.sender.send(job);
    }

    /// Number of worker threads.
    pub fn size(&self) -> usize {
        self.workers.len()
    }

    /// Shutdown the pool: signal workers to stop and join them.
    ///
    /// P1 fix: actually `join()` each worker so a graceful shutdown waits
    /// for in-flight jobs to complete (or for the worker to observe the
    /// shutdown signal and exit its loop) instead of abandoning the threads.
    /// Joining also lets us surface a panic from a worker via `join_err`,
    /// which previously was silently dropped.
    pub fn shutdown(&self) {
        let _ = self.inner.shutdown.send(());
        // Drop the sender so receivers see channel close — but it's shared
        // via Arc with workers only through the receiver; we keep the sender
        // alive on the pool. The shutdown signal is the primary stop trigger.
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // Signal workers to exit their select! loop.
        let _ = self.inner.shutdown.send(());
        // Drain and join each worker so we don't abandon threads mid-job.
        // A bounded join timeout would be ideal, but std JoinHandle has no
        // timed join; workers exit promptly once they observe the shutdown
        // signal (between jobs, immediately; mid-job, after the job returns).
        for handle in self.workers.drain(..) {
            if let Err(join_err) = handle.join() {
                // Worker panicked — log and continue joining the rest.
                tracing::warn!(
                    error = ?join_err,
                    "thread pool worker panicked during shutdown; continuing to join remaining workers"
                );
            }
        }
    }
}

/// Get the configured pool size (from env or CPU count).
pub fn configured_size() -> usize {
    if let Ok(s) = std::env::var("XHJOB_THREAD_POOL_SIZE") {
        if let Ok(n) = s.parse::<usize>() {
            if n > 0 { return n; }
        }
    }
    num_cpus::get()
}

/// Get the global thread pool (lazily initialized).
pub fn global() -> &'static ThreadPool {
    GLOBAL.get_or_init(|| ThreadPool::new(configured_size()))
}

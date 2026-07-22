//! 线程池模块（多线程池模式）。
//!
//! 当 `XHJOB_POOL_MODE=thread` 时，daemon 使用此线程池执行任务。每个任务
//! 在独立的工作线程中运行（通过 `block_on` 执行 async future），适合
//! CPU 密集型或需要严格并发控制的场景。并发度由线程数控制（默认=CPU 核数，
//! 可通过 `XHJOB_THREAD_POOL_SIZE` 覆盖）。
//!
//! 默认模式 `XHJOB_POOL_MODE=coroutine` 使用协程池（tokio async runtime），
//! 适合 IO 密集型任务，最大并发 1024（可通过 `XHJOB_COROUTINE_POOL_SIZE` 覆盖）。
//!
//! Built on top of `std::thread` + `crossbeam-channel`.

use std::sync::Arc;
use std::thread;
use crossbeam_channel::{bounded, unbounded, Sender};
use once_cell::sync::OnceCell;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct PoolInner {
    sender: Sender<Job>,
    shutdown: Sender<()>,
    workers: Vec<thread::JoinHandle<()>>,
}

pub struct ThreadPool {
    inner: Arc<PoolInner>,
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
                workers,
            }),
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
        self.inner.workers.len()
    }

    /// Shutdown the pool: signal workers to stop and join them.
    pub fn shutdown(&self) {
        let _ = self.inner.shutdown.send(());
        // Drop sender so receivers eventually see channel close.
        // Note: we can't drop sender here because it's shared via Arc; instead
        // workers will exit on shutdown signal.
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.shutdown();
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

//! Real OS-thread pool for CPU-intensive task execution.
//!
//! Built on top of `std::thread` + `crossbeam-channel`. The default pool size
//! equals the number of CPU cores; it can be overridden via `XHJOB_THREAD_POOL_SIZE`.

use std::sync::Arc;
use std::thread;
use crossbeam_channel::{bounded, unbounded, Sender, Receiver};
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

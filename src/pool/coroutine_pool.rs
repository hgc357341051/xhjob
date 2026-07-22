//! Async task pool backed by tokio (M:N scheduling).
//!
//! Spawns async tasks (Futures) onto the global tokio runtime, gated by a
//! `Semaphore` to enforce a max concurrency. The default max concurrency is
//! 1024; it can be overridden via `XHJOB_ASYNC_POOL_SIZE` (recommended) or
//! `XHJOB_COROUTINE_POOL_SIZE` (legacy alias, kept for backward compatibility).
//!
//! NOTE on naming: Rust has no language-level "coroutine" — only `async/await`
//! + `Future` compiled to state machines, polled by a runtime (tokio). The
//! file/struct name `coroutine_pool` is kept for path stability, but the
//! accurate term is "async task pool": M async tasks multiplexed onto N tokio
//! worker threads (N = num_cpus) via cooperative `await` yield points.

use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use once_cell::sync::OnceCell;

pub struct CoroutinePool {
    semaphore: Arc<Semaphore>,
    max_concurrency: usize,
}

static GLOBAL: OnceCell<CoroutinePool> = OnceCell::new();
static GLOBAL_RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();

impl CoroutinePool {
    /// Create a new coroutine pool with the given max concurrency.
    pub fn new(max_concurrency: usize) -> Self {
        assert!(max_concurrency > 0, "max concurrency must be > 0");
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            max_concurrency,
        }
    }

    /// Max concurrency limit.
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// Spawn a future onto the pool. The future will wait for a permit
    /// before executing if all concurrency slots are taken.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let sem = Arc::clone(&self.semaphore);
        tokio::spawn(async move {
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(_) => return, // semaphore closed
            };
            future.await;
        })
    }
}

/// Get the configured max concurrency.
///
/// Reads `XHJOB_ASYNC_POOL_SIZE` (recommended) first, then falls back to
/// `XHJOB_COROUTINE_POOL_SIZE` (legacy alias). Defaults to 1024.
pub fn configured_max() -> usize {
    for var in ["XHJOB_ASYNC_POOL_SIZE", "XHJOB_COROUTINE_POOL_SIZE"] {
        if let Ok(s) = std::env::var(var) {
            if let Ok(n) = s.parse::<usize>() {
                if n > 0 { return n; }
            }
        }
    }
    1024
}

/// Get the global coroutine pool (must be called from within a tokio runtime).
pub fn global() -> &'static CoroutinePool {
    GLOBAL.get_or_init(|| CoroutinePool::new(configured_max()))
}

/// Initialize the global tokio runtime (multi-thread, worker_threads = num_cpus).
/// Should be called once at daemon startup.
pub fn init_global_runtime() -> &'static tokio::runtime::Runtime {
    GLOBAL_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(num_cpus::get())
            .enable_all()
            .thread_name("xhjob-tokio")
            .build()
            .expect("failed to build tokio runtime")
    })
}

/// Get the global runtime if initialized.
pub fn global_runtime() -> Option<&'static tokio::runtime::Runtime> {
    GLOBAL_RT.get()
}

/// Run a future to completion on the global runtime (blocking call).
/// Used by daemon main to enter the async context.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let rt = init_global_runtime();
    rt.block_on(future)
}

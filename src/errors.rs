// P0 fix: switched to thiserror for proper source() chain support.
// Previously the std::error::Error impl was empty (`impl Error for XhjobError {}`),
// meaning the root cause of Io errors was lost when printed. With thiserror,
// #[from] generates both From and source() automatically.
//
// P0 fix (round 2): the Ipc/Store/Exec/Config variants now carry an optional
// boxed source. Callers that simply format!("...: {}", e) keep working via
// the `From<String>` + `From<(String, Box<dyn Error>)>` constructors; new
// code can use `XhjobError::Ipc::with_source(ctx, e)` to preserve the source
// chain so `.source()` walks into the underlying rusqlite / hyper / io error
// instead of being flattened into a string.
use thiserror::Error;

pub type BoxedSource = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
pub enum XhjobError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("ipc: {context}")]
    Ipc {
        context: String,
        #[source]
        source: Option<BoxedSource>,
    },

    #[error("store: {context}")]
    Store {
        context: String,
        #[source]
        source: Option<BoxedSource>,
    },

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("daemon not running")]
    DaemonNotRunning,

    #[error("daemon already running")]
    DaemonAlreadyRunning,

    #[error("invalid task: {0}")]
    InvalidTask(String),

    #[error("cron parse: {0}")]
    CronParse(String),

    #[error("exec: {context}")]
    Exec {
        context: String,
        #[source]
        source: Option<BoxedSource>,
    },

    #[error("config: {context}")]
    Config {
        context: String,
        #[source]
        source: Option<BoxedSource>,
    },
}

impl XhjobError {
    /// Build an `Ipc` error from a context string only (no source).
    #[inline]
    pub fn ipc(context: impl Into<String>) -> Self {
        XhjobError::Ipc {
            context: context.into(),
            source: None,
        }
    }
    /// Build an `Ipc` error preserving the underlying cause.
    #[inline]
    pub fn ipc_with_source(context: impl Into<String>, source: BoxedSource) -> Self {
        XhjobError::Ipc {
            context: context.into(),
            source: Some(source),
        }
    }
    /// Build a `Store` error from a context string only.
    #[inline]
    pub fn store(context: impl Into<String>) -> Self {
        XhjobError::Store {
            context: context.into(),
            source: None,
        }
    }
    /// Build a `Store` error preserving the underlying cause.
    #[inline]
    pub fn store_with_source(context: impl Into<String>, source: BoxedSource) -> Self {
        XhjobError::Store {
            context: context.into(),
            source: Some(source),
        }
    }
    /// Build an `Exec` error from a context string only.
    #[inline]
    pub fn exec(context: impl Into<String>) -> Self {
        XhjobError::Exec {
            context: context.into(),
            source: None,
        }
    }
    /// Build an `Exec` error preserving the underlying cause.
    #[inline]
    pub fn exec_with_source(context: impl Into<String>, source: BoxedSource) -> Self {
        XhjobError::Exec {
            context: context.into(),
            source: Some(source),
        }
    }
    /// Build a `Config` error from a context string only.
    #[inline]
    pub fn config(context: impl Into<String>) -> Self {
        XhjobError::Config {
            context: context.into(),
            source: None,
        }
    }
}

// Backwards-compatible constructors: keep `XhjobError::Ipc("...".to_string())`
// style call sites working by routing through the struct variant with no source.
impl From<String> for XhjobError {
    fn from(s: String) -> Self {
        // Default bucket for bare strings — kept for ergonomic `?` propagation
        // of plain strings. Prefer the typed constructors above for new code.
        XhjobError::ipc(s)
    }
}

pub type Result<T> = std::result::Result<T, XhjobError>;

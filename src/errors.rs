// P0 fix: switched to thiserror for proper source() chain support.
// Previously the std::error::Error impl was empty (`impl Error for XhjobError {}`),
// meaning the root cause of Io errors was lost when printed. With thiserror,
// #[from] generates both From and source() automatically.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XhjobError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("ipc: {0}")]
    Ipc(String),

    #[error("store: {0}")]
    Store(String),

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

    #[error("exec: {0}")]
    Exec(String),

    #[error("config: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, XhjobError>;

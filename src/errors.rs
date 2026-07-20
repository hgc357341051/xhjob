use std::fmt;
use std::io;

#[derive(Debug)]
pub enum XhjobError {
    Io(io::Error),
    Ipc(String),
    Store(String),
    TaskNotFound(String),
    DaemonNotRunning,
    DaemonAlreadyRunning,
    InvalidTask(String),
    CronParse(String),
    Exec(String),
<<<<<<< Updated upstream
=======
    Config(String),
>>>>>>> Stashed changes
}

impl fmt::Display for XhjobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XhjobError::Io(e) => write!(f, "io: {}", e),
            XhjobError::Ipc(s) => write!(f, "ipc: {}", s),
            XhjobError::Store(s) => write!(f, "store: {}", s),
            XhjobError::TaskNotFound(id) => write!(f, "task not found: {}", id),
            XhjobError::DaemonNotRunning => write!(f, "daemon not running"),
            XhjobError::DaemonAlreadyRunning => write!(f, "daemon already running"),
            XhjobError::InvalidTask(s) => write!(f, "invalid task: {}", s),
            XhjobError::CronParse(s) => write!(f, "cron parse: {}", s),
            XhjobError::Exec(s) => write!(f, "exec: {}", s),
<<<<<<< Updated upstream
=======
            XhjobError::Config(s) => write!(f, "config: {}", s),
>>>>>>> Stashed changes
        }
    }
}

impl std::error::Error for XhjobError {}

impl From<io::Error> for XhjobError {
    fn from(e: io::Error) -> Self { XhjobError::Io(e) }
}

pub type Result<T> = std::result::Result<T, XhjobError>;

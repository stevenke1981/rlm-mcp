use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("cbm client error: {0}")]
    Cbm(String),

    #[error("{0}")]
    Other(String),
}

//! Error types.

use thiserror::Error;

pub type MemioResult<T> = Result<T, MemioError>;

#[derive(Debug, Error)]
pub enum MemioError {
    #[error("Arena allocation failed: requested {requested} bytes, available {available}")]
    ArenaFull { requested: usize, available: usize },

    #[error("Alignment error: expected {expected}, got {actual}")]
    Alignment { expected: usize, actual: usize },

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Platform not supported")]
    PlatformNotSupported,

    #[error("Invalid capacity")]
    InvalidCapacity,

    #[error("Create failed: {0}")]
    CreateFailed(String),

    #[error("Open failed: {0}")]
    OpenFailed(String),

    #[error("Memory mapping failed")]
    MmapFailed,

    #[error("Data ({data_len} bytes) exceeds capacity ({capacity} bytes)")]
    DataTooLarge { data_len: usize, capacity: usize },

    #[error("Invalid header")]
    InvalidHeader,

    #[error("Region not found: {0}")]
    NotFound(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for MemioError {
    fn from(e: std::io::Error) -> Self {
        MemioError::Io(e.to_string())
    }
}

impl<T> From<std::sync::PoisonError<T>> for MemioError {
    fn from(e: std::sync::PoisonError<T>) -> Self {
        MemioError::LockPoisoned(e.to_string())
    }
}

impl MemioError {
    pub fn lock_poisoned(msg: impl Into<String>) -> Self {
        MemioError::LockPoisoned(msg.into())
    }
    
    pub fn lock_failed() -> Self {
        MemioError::Io("Failed to acquire lock".into())
    }
}

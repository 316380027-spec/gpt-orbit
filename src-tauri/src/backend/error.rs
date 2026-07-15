use thiserror::Error;

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Clone, Debug, Error)]
pub enum BackendError {
    #[error("Codex authentication required")]
    AuthenticationRequired,
    #[error("invalid app-server message: {0}")]
    InvalidMessage(String),
    #[error("app-server RPC disconnected")]
    RpcDisconnected,
    #[error("app-server RPC request timed out")]
    RequestTimeout,
    #[error("app-server RPC error: {0}")]
    RpcError(String),
}

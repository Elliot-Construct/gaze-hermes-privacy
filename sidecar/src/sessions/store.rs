//! Typed errors for session snapshot persistence.

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("encrypt failed")]
    Encrypt,
    #[error("decrypt failed")]
    Decrypt,
    #[error("integrity check failed")]
    Integrity,
    #[error("recovery blocked: snapshot present but unusable")]
    RecoveryBlocked,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session error: {0}")]
    Session(String),
}

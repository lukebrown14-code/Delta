//! Errors shared by the services crate.

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("sqlite error: {0}")]
    Db(#[from] delta_core::db::DbError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{message}")]
    Invalid { message: String },
    #[error("plugin {plugin}: {message}")]
    Plugin { plugin: String, message: String },
}

impl ServiceError {
    /// `ServiceError::Invalid` shorthand.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }
}

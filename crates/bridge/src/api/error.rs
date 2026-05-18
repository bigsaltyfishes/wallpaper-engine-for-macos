#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgeErrorKind {
    Config,
    Library,
    Project,
    Engine,
    Display,
    Io,
    InvalidInput,
    Startup,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{message}")]
    Error {
        kind: BridgeErrorKind,
        message: String,
    },
}

impl BridgeError {
    #[must_use]
    pub fn kind(&self) -> BridgeErrorKind {
        match self {
            Self::Error { kind, .. } => *kind,
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Error { message, .. } => message,
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::Error {
            kind: BridgeErrorKind::InvalidInput,
            message: message.into(),
        }
    }

    pub fn engine(message: impl Into<String>) -> Self {
        Self::Error {
            kind: BridgeErrorKind::Engine,
            message: message.into(),
        }
    }
}

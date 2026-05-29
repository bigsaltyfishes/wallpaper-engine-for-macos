//! Shader pipeline errors and diagnostics.

mod diagnostic;
mod report;
mod span;

use thiserror::Error;

pub use self::{diagnostic::ShaderDiagnostic, span::SourceSpan};
use crate::model::IncludePath;

/// Result type used by the shader crate.
pub type ShaderResult<T> = Result<T, ShaderError>;

/// Error type for shader model, source, compilation, and reflection failures.
#[derive(Debug, Error)]
pub enum ShaderError {
    /// The requested shader program or typed model value is invalid.
    #[error("invalid shader request: {message}")]
    InvalidRequest {
        /// Validation message.
        message: String,
    },

    /// A shader include was not found by the source provider.
    #[error("shader include not found: {path}")]
    IncludeNotFound {
        /// Missing include path.
        path: IncludePath,
    },

    /// A shader include was found but could not be read.
    #[error("shader source read failed for {path}: {message}")]
    SourceRead {
        /// Include path that failed to read.
        path: IncludePath,
        /// Provider-specific read failure message.
        message: String,
    },

    /// Shader include bytes were not valid UTF-8.
    #[error("shader source utf-8 invalid for {path}")]
    InvalidSourceUtf8 {
        /// Include path with invalid UTF-8.
        path: IncludePath,
    },

    /// Shader parsing failed.
    #[error("shader parse failed")]
    Parse {
        /// Parse diagnostics.
        diagnostics: Box<[ShaderDiagnostic]>,
    },

    /// Shader legalization failed.
    #[error("shader legalization failed")]
    Legalize {
        /// Legalization diagnostics.
        diagnostics: Box<[ShaderDiagnostic]>,
    },

    /// Shader compilation failed.
    #[error("naga compilation failed")]
    Compile {
        /// Compilation diagnostics.
        diagnostics: Box<[ShaderDiagnostic]>,
    },

    /// Shader reflection failed.
    #[error("shader reflection failed: {message}")]
    Reflection {
        /// Reflection message.
        message: String,
    },

    /// Shader bridge conversion failed.
    #[cfg(feature = "ffi")]
    #[error("shader bridge failed: {message}")]
    Bridge {
        /// Bridge conversion message.
        message: String,
    },
}

impl ShaderError {
    /// Creates an invalid request error.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into(),
        }
    }

    /// Creates a source provider read failure.
    #[must_use]
    pub fn source_read(path: IncludePath, message: impl Into<String>) -> Self {
        Self::SourceRead {
            path,
            message: message.into(),
        }
    }

    /// Creates an invalid UTF-8 source error.
    #[must_use]
    pub const fn invalid_source_utf8(path: IncludePath) -> Self {
        Self::InvalidSourceUtf8 { path }
    }

    /// Creates a bridge conversion error.
    #[cfg(feature = "ffi")]
    #[must_use]
    pub fn bridge(message: impl Into<String>) -> Self {
        Self::Bridge {
            message: message.into(),
        }
    }

    /// Renders diagnostic-bearing errors with `miette`.
    #[must_use]
    pub fn to_miette_report(&self) -> String {
        let diagnostics = match self {
            Self::Parse { diagnostics }
            | Self::Legalize { diagnostics }
            | Self::Compile { diagnostics } => diagnostics.as_ref(),
            Self::InvalidRequest { .. }
            | Self::IncludeNotFound { .. }
            | Self::SourceRead { .. }
            | Self::InvalidSourceUtf8 { .. }
            | Self::Reflection { .. } => return self.to_string(),
            #[cfg(feature = "ffi")]
            Self::Bridge { .. } => return self.to_string(),
        };

        if diagnostics.is_empty() {
            return self.to_string();
        }

        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.to_miette_report(None))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

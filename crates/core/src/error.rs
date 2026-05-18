use std::{
    error::Error,
    fmt::{Display, Formatter},
};

/// Error type returned by wallpaper-core APIs.
///
/// Variants are intentionally broad because the crate crosses several
/// subsystems: platform windows/audio, Open Wallpaper Engine renderer calls,
/// media decoding, JSON parsing, and filesystem-backed cache management.
#[derive(Debug)]
pub enum EngineError {
    /// Caller supplied invalid data or an unsupported value.
    InvalidInput(String),
    /// Platform API or system integration failed.
    Platform(String),
    /// Renderer setup, cache preparation, or rendering backend failed.
    Render(String),
    /// An unwind crossed the Open Wallpaper Engine FFI boundary.
    Crash(String),
    /// The requested operation is not implemented on this target OS.
    UnsupportedPlatform(&'static str),
}

impl Display for EngineError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::Platform(message)
            | Self::Render(message)
            | Self::Crash(message) => write!(f, "{message}"),
            Self::UnsupportedPlatform(platform) => {
                write!(f, "unsupported platform: {platform}")
            }
        }
    }
}

impl Error for EngineError {}

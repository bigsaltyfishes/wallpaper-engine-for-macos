//! Source span validation and serialization.

use super::{ShaderError, ShaderResult};

/// Byte span in a shader source buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SourceSpan {
    /// Inclusive byte offset where the span begins.
    start: usize,
    /// Exclusive byte offset where the span ends.
    end: usize,
}

impl SourceSpan {
    /// Creates a validated source span.
    ///
    /// # Errors
    ///
    /// Returns an error when `end` is before `start`.
    pub fn new(start: usize, end: usize) -> ShaderResult<Self> {
        if end < start {
            return Err(ShaderError::invalid_request(
                "source span end is before start",
            ));
        }

        Ok(Self { start, end })
    }

    /// Returns the inclusive start byte offset.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Returns the exclusive end byte offset.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for SourceSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(rename = "SourceSpan")]
        struct SpanDto {
            start: usize,
            end: usize,
        }

        let dto = SpanDto::deserialize(deserializer)?;
        Self::new(dto.start, dto.end).map_err(serde::de::Error::custom)
    }
}

//! Shader source provider traits and in-memory implementation.

use std::collections::BTreeMap;

use crate::{IncludePath, ShaderError, ShaderResult};

/// Provider used by the pipeline to read shader include sources.
pub trait ShaderSourceProvider {
    /// Reads a shader include path as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`ShaderError::IncludeNotFound`] when the include path is not
    /// available, [`ShaderError::SourceRead`] when the provider cannot read
    /// the source, or [`ShaderError::InvalidSourceUtf8`] when available bytes
    /// are not valid UTF-8.
    fn read_to_string(&self, path: &IncludePath) -> ShaderResult<String>;
}

/// In-memory shader source provider for tests and small static source sets.
#[derive(Clone, Debug, Default)]
pub struct InMemoryShaderSourceProvider {
    /// Include path to UTF-8 source text map.
    sources: BTreeMap<IncludePath, String>,
}

impl InMemoryShaderSourceProvider {
    /// Creates an empty in-memory source provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
        }
    }

    /// Adds or replaces an include source.
    #[must_use]
    pub fn with_source(mut self, path: IncludePath, source: impl Into<String>) -> Self {
        let _old = self.sources.insert(path, source.into());
        self
    }

    /// Adds or replaces an include source by mutable reference.
    pub fn insert(&mut self, path: IncludePath, source: impl Into<String>) {
        let _old = self.sources.insert(path, source.into());
    }

    /// Returns the number of include sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Returns whether no include sources are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

impl ShaderSourceProvider for InMemoryShaderSourceProvider {
    fn read_to_string(&self, path: &IncludePath) -> ShaderResult<String> {
        let Some(source) = self.sources.get(path) else {
            return Err(ShaderError::IncludeNotFound { path: path.clone() });
        };

        Ok(source.clone())
    }
}

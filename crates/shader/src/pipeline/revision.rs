/// Stable identity for compiler options that affect generated SPIR-V.
pub(super) const COMPILER_OPTIONS_CACHE_SALT: &str =
    "naga-29.0.3-spv-no-coordinate-space-adjustment";

/// Typed pipeline revision included in cache key construction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ShaderPipelineRevision(u64);

impl ShaderPipelineRevision {
    /// Current default pipeline revision.
    pub const CURRENT: Self = Self(3);

    /// Creates a typed pipeline revision.
    #[must_use]
    pub const fn new(revision: u64) -> Self {
        Self(revision)
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

//! Typed source text used by syntax parsing.

use crate::SourceSpan;

/// Typed view of shader source text used during syntax parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShaderSourceText<'src> {
    /// Borrowed shader source text.
    source: &'src str,
}

impl<'src> ShaderSourceText<'src> {
    /// Creates a typed shader source view.
    #[must_use]
    pub const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Returns the original shader source text.
    #[must_use]
    pub const fn as_str(self) -> &'src str {
        self.source
    }

    /// Borrows the source text covered by `span`.
    #[must_use]
    pub fn slice(self, span: SourceSpan) -> &'src str {
        debug_assert!(
            self.source.get(span.start()..span.end()).is_some(),
            "source span must reference this shader source"
        );
        self.source
            .get(span.start()..span.end())
            .map_or("", |slice| slice)
    }

    /// Returns whether the gap between two spans stays on the same source
    /// line.
    #[must_use]
    pub fn is_same_line_gap(self, before: SourceSpan, after: SourceSpan) -> bool {
        if after.start() < before.end() {
            return false;
        }

        self.source
            .get(before.end()..after.start())
            .is_some_and(|between| !between.bytes().any(|byte| matches!(byte, b'\n' | b'\r')))
    }
}

/// Shared behavior for syntax values that expose typed source text.
pub trait SourceTextView<'src> {
    /// Returns the typed shader source view.
    fn source_text(&self) -> ShaderSourceText<'src>;
}

/// Shared behavior for syntax values that expose a source span.
pub trait SpannedSyntax {
    /// Returns the syntax value's source span.
    fn span(&self) -> SourceSpan;
}

impl<'src> SourceTextView<'src> for ShaderSourceText<'src> {
    fn source_text(&self) -> ShaderSourceText<'src> {
        *self
    }
}

impl Default for SourceSpan {
    fn default() -> Self {
        Self::new(0, 0).expect("zero-length source span is valid")
    }
}

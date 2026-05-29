//! Declarator and type-name facts.

use crate::{SourceSpan, tokenizer::TokenCursor};

/// Token range for one declarator initializer.
#[derive(Clone, Copy)]
pub(crate) struct DeclaratorInitializer {
    /// First non-comment initializer token.
    pub start: usize,
    /// Last non-comment initializer token.
    pub end: usize,
    /// Source span covering the initializer expression.
    pub span: SourceSpan,
}

impl DeclaratorInitializer {
    /// Creates an initializer range from inclusive token bounds.
    #[must_use]
    pub(crate) fn from_inclusive_tokens(
        tokens: TokenCursor<'_>,
        start: usize,
        end: usize,
    ) -> Option<Self> {
        let span = SourceSpan::new(tokens[start].span().start(), tokens[end].span().end()).ok()?;
        Some(Self { start, end, span })
    }

    /// Returns the first initializer token.
    #[must_use]
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    /// Returns the last initializer token.
    #[must_use]
    pub(crate) const fn end(self) -> usize {
        self.end
    }

    /// Returns the initializer source span.
    #[must_use]
    pub(crate) const fn span(self) -> SourceSpan {
        self.span
    }
}

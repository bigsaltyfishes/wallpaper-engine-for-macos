//! Expression replacement templates rendered from settled child fixups.

pub mod analysis;

use super::fixups::{Fixup, FixupReplacement};
use crate::{ShaderResult, SourceSpan, syntax::ShaderSourceText};

/// Replacement text assembled from literal text and rendered source spans.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExpressionReplacement {
    /// Ordered replacement parts.
    parts: Vec<ExpressionPart>,
    /// Whether generated text changed expression semantics.
    changed: bool,
}

impl ExpressionReplacement {
    /// Creates an empty expression replacement.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            parts: Vec::new(),
            changed: false,
        }
    }

    /// Creates an expression replacement from generated text.
    #[must_use]
    pub(crate) fn changed_text(text: impl Into<String>) -> Self {
        Self::new().with_text(text).mark_changed()
    }

    /// Marks this replacement as semantically changed.
    #[must_use]
    pub(crate) const fn mark_changed(mut self) -> Self {
        self.changed = true;
        self
    }

    /// Appends literal replacement text.
    #[must_use]
    pub(crate) fn with_text(mut self, text: impl Into<String>) -> Self {
        self.push_text(text);
        self
    }

    /// Appends a source span that should be rendered with child fixups applied.
    #[must_use]
    pub(crate) fn with_source(mut self, span: SourceSpan) -> Self {
        self.push_source(span);
        self
    }

    /// Appends another expression replacement.
    #[must_use]
    pub(crate) fn with_replacement(mut self, replacement: Self) -> Self {
        self.changed |= replacement.changed;
        self.parts.extend(replacement.parts);
        self
    }

    /// Appends literal replacement text.
    pub(crate) fn push_text(&mut self, text: impl Into<String>) {
        self.parts.push(ExpressionPart::Text(text.into()));
    }

    /// Appends an original source span.
    pub(crate) fn push_source(&mut self, span: SourceSpan) {
        self.parts.push(ExpressionPart::Source(span));
    }

    /// Appends another expression replacement.
    pub(crate) fn push_replacement(&mut self, replacement: Self) {
        self.changed |= replacement.changed;
        self.parts.extend(replacement.parts);
    }

    /// Returns whether generated text changed expression semantics.
    #[must_use]
    pub(crate) const fn is_changed(&self) -> bool {
        self.changed
    }
}

/// One component in an expression replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ExpressionPart {
    /// Literal replacement text.
    Text(String),
    /// Source span rendered through child fixups.
    Source(SourceSpan),
}

/// Renders source spans while applying already-collected nested fixups.
pub(super) struct ExpressionRenderer<'fixups, 'src> {
    /// Original shader source.
    pub source: ShaderSourceText<'src>,
    /// Ordered fixups available to child expressions.
    pub fixups: &'fixups [Fixup],
}

impl ExpressionRenderer<'_, '_> {
    /// Renders an expression replacement, excluding the replacement currently
    /// being resolved so it cannot recursively consume itself.
    pub(super) fn render_replacement(
        &self,
        replacement: &ExpressionReplacement,
        excluded: usize,
    ) -> ShaderResult<String> {
        let mut output = String::new();
        for part in &replacement.parts {
            match part {
                ExpressionPart::Text(text) => output.push_str(text),
                ExpressionPart::Source(span) => {
                    output.push_str(&self.render_span(*span, Some(excluded))?);
                }
            }
        }
        Ok(output)
    }

    /// Renders one source span with top-level child fixups applied.
    fn render_span(&self, span: SourceSpan, excluded: Option<usize>) -> ShaderResult<String> {
        let mut output = String::new();
        let mut copied = span.start();
        for (index, fixup) in self.fixups.iter().enumerate() {
            if excluded == Some(index) || !span.contains(fixup.span()) {
                continue;
            }
            if fixup.span().start() < copied {
                continue;
            }

            output.push_str(
                self.source
                    .slice(SourceSpan::new(copied, fixup.span().start())?),
            );
            output.push_str(&self.render_fixup(index)?);
            copied = fixup.span().end();
        }
        output.push_str(self.source.slice(SourceSpan::new(copied, span.end())?));
        Ok(output)
    }

    /// Renders a child fixup replacement.
    fn render_fixup(&self, index: usize) -> ShaderResult<String> {
        let fixup = &self.fixups[index];
        match fixup.replacement() {
            FixupReplacement::Text(text) => Ok(text.clone()),
            FixupReplacement::Expression(replacement) => {
                self.render_replacement(replacement, index)
            }
        }
    }
}

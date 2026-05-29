//! Ordered source fixups for one codegen pass.

use super::{declarations::DeclarationPlan, expressions::ExpressionReplacement};
use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, SourceSpan, syntax::ShaderModule,
    tokenizer::TypedToken,
};

/// Collection of non-overlapping source edits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FixupSet {
    /// Pending source edits.
    items: Vec<Fixup>,
}

impl FixupSet {
    /// Adds a source edit, dropping duplicate or covered edits.
    pub(crate) fn push(&mut self, fixup: Fixup) {
        if self.items.iter().any(|existing| existing == &fixup) {
            return;
        }

        if fixup.span().empty() {
            self.items.push(fixup);
            return;
        }

        if self.items.iter().any(|existing| {
            existing.span().contains(fixup.span())
                && !existing.replacement().renders_children()
                && !(existing.span() == fixup.span() && fixup.replacement().renders_children())
        }) {
            return;
        }

        self.items.retain(|existing| {
            if existing.span().empty() || !fixup.span().contains(existing.span()) {
                return true;
            }
            if fixup.replacement().renders_children() {
                return true;
            }
            fixup.span() == existing.span() && existing.replacement().renders_children()
        });
        self.items.push(fixup);
    }

    /// Inserts local-copy declarations at the beginning of `main`.
    pub(crate) fn insert_main_prelude(
        &mut self,
        module: &ShaderModule<'_>,
        declarations: &DeclarationPlan<'_>,
    ) -> ShaderResult<()> {
        let mut prelude = String::new();
        for input in declarations.main_prelude_interfaces() {
            input.emit_local_copy(&mut prelude)?;
        }
        if prelude.is_empty() {
            return Ok(());
        }

        let tokens = module.token_stream();
        let Some(offset) = module.function_calls().find_map(|call| {
            if call.name() != "main" {
                return None;
            }
            let body_open = tokens.next_non_comment(call.close_index + 1)?;
            matches!(tokens.as_slice()[body_open].kind(), TypedToken::LeftBrace)
                .then_some(tokens.as_slice()[body_open].span().end())
        }) else {
            return Ok(());
        };
        self.push(Fixup::insert(SourceSpan::new(offset, offset)?, prelude));
        Ok(())
    }

    /// Sorts fixups and verifies that replacements do not overlap.
    pub(crate) fn ordered(&mut self) -> ShaderResult<&[Fixup]> {
        self.items.sort_by(|left, right| {
            left.span()
                .start()
                .cmp(&right.span().start())
                .then(left.span().end().cmp(&right.span().end()))
        });
        for (index, fixup) in self.items.iter().enumerate() {
            if fixup.span().empty() || Self::is_expression_child(&self.items, index) {
                continue;
            }
            if self
                .items
                .iter()
                .enumerate()
                .take(index)
                .any(|(previous_index, previous)| {
                    !(previous.span().empty()
                        || Self::is_expression_child(&self.items, previous_index)
                        || (fixup.replacement().renders_children()
                            && fixup.span() != previous.span()
                            && fixup.span().contains(previous.span()))
                        || previous.span().end() <= fixup.span().start())
                })
            {
                return Err(ShaderError::Codegen {
                    diagnostics: Box::new([
                        ShaderDiagnostic::new("codegen fixups overlap").with_span(fixup.span())
                    ]),
                });
            }
        }
        Ok(&self.items)
    }

    /// Returns whether the fixup is consumed by an enclosing expression
    /// replacement instead of final top-level emission.
    #[must_use]
    pub(crate) fn is_expression_child(items: &[Fixup], index: usize) -> bool {
        let child = &items[index];
        items.iter().enumerate().any(|(parent_index, parent)| {
            parent_index != index
                && !parent.span().empty()
                && parent.replacement().renders_children()
                && (parent.span() != child.span() || !child.replacement().renders_children())
                && parent.span().contains(child.span())
        })
    }
}

/// Single source edit represented as span replacement text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Fixup {
    /// Source range replaced by this fixup.
    span: SourceSpan,
    /// Replacement text, empty for removals.
    replacement: FixupReplacement,
}

impl Fixup {
    /// Creates an insertion fixup at a zero-length span.
    #[must_use]
    pub(crate) fn insert(span: SourceSpan, replacement: String) -> Self {
        Self {
            span,
            replacement: FixupReplacement::Text(replacement),
        }
    }

    /// Creates a replacement fixup.
    pub(crate) fn replace(span: SourceSpan, replacement: impl Into<FixupReplacement>) -> Self {
        Self {
            span,
            replacement: replacement.into(),
        }
    }

    /// Returns the source range affected by this fixup.
    #[must_use]
    pub(crate) const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns replacement text for this fixup.
    #[must_use]
    pub(crate) const fn replacement(&self) -> &FixupReplacement {
        &self.replacement
    }
}

/// Replacement payload for one source fixup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FixupReplacement {
    /// Fully resolved text replacement.
    Text(String),
    /// Expression template resolved after child fixups have settled.
    Expression(ExpressionReplacement),
}

impl FixupReplacement {
    /// Returns whether this replacement renders child fixups from covered
    /// source spans.
    const fn renders_children(&self) -> bool {
        matches!(self, Self::Expression(_))
    }
}

impl From<String> for FixupReplacement {
    fn from(replacement: String) -> Self {
        Self::Text(replacement)
    }
}

impl From<&str> for FixupReplacement {
    fn from(replacement: &str) -> Self {
        Self::Text(replacement.to_owned())
    }
}

impl From<ExpressionReplacement> for FixupReplacement {
    fn from(replacement: ExpressionReplacement) -> Self {
        Self::Expression(replacement)
    }
}

impl SourceSpan {
    /// Returns whether `self` fully covers `other`.
    #[must_use]
    pub(crate) const fn contains(self, other: SourceSpan) -> bool {
        self.start() <= other.start() && self.end() >= other.end()
    }

    /// Returns whether the span is a zero-length insertion point.
    #[must_use]
    pub(crate) const fn empty(self) -> bool {
        self.start() == self.end()
    }
}

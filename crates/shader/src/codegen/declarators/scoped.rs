//! Scoped local declaration facts.

use smol_str::SmolStr;

use super::types::DeclaratorInitializer;
use crate::{
    SourceSpan,
    tokenizer::{DeclarationFact, KeywordType, TokenCursor, TokenIndexRange, TypedToken},
};

/// Simple local declaration.
#[derive(Clone)]
pub(crate) struct LocalDeclaration {
    /// Declared name token text.
    pub name: SmolStr,
    /// Declared type token text.
    pub ty: SmolStr,
    /// Declared type token index within the scanned token slice.
    pub type_index: usize,
    /// Declared name token index within the scanned token slice.
    pub name_index: usize,
    /// First token after the declaration statement.
    pub tail_start: usize,
    /// First token after this declarator's initializer.
    pub declarator_end: usize,
    /// First token outside the declaration's lexical scope.
    pub scope_end: usize,
    /// Declared name token span.
    pub name_span: SourceSpan,
    /// Declared type token span.
    pub type_span: SourceSpan,
    /// Initializer range derived from tokenizer facts.
    pub initializer: Option<TokenIndexRange>,
}

impl LocalDeclaration {
    /// Returns the local name.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the local type spelling.
    #[must_use]
    pub(crate) fn ty(&self) -> &str {
        self.ty.as_str()
    }

    /// Returns the local type token index.
    #[must_use]
    pub(crate) const fn type_index(&self) -> usize {
        self.type_index
    }

    /// Returns the local name token index.
    #[must_use]
    pub(crate) const fn name_index(&self) -> usize {
        self.name_index
    }

    /// Returns the first token after the declaration statement.
    #[must_use]
    pub(crate) const fn tail_start(&self) -> usize {
        self.tail_start
    }

    /// Returns the first token after this declarator's initializer.
    #[must_use]
    pub(crate) const fn declarator_end(&self) -> usize {
        self.declarator_end
    }

    /// Returns first token outside this local declaration's scope.
    #[must_use]
    pub(crate) const fn scope_end(&self) -> usize {
        self.scope_end
    }

    /// Returns the declaration name span.
    #[must_use]
    pub(crate) const fn name_span(&self) -> SourceSpan {
        self.name_span
    }

    /// Returns the declaration type span.
    #[must_use]
    pub(crate) const fn type_span(&self) -> SourceSpan {
        self.type_span
    }

    /// Returns this declarator's initializer range.
    #[must_use]
    pub(crate) fn initializer(&self, tokens: TokenCursor<'_>) -> Option<DeclaratorInitializer> {
        let range = self.initializer?;
        let end = range.last()?;
        DeclaratorInitializer::from_inclusive_tokens(tokens, range.start(), end)
    }

    /// Returns the fixed-array suffix after this declarator name, when present.
    #[must_use]
    pub(crate) fn array_suffix(&self, tokens: TokenCursor<'_>) -> Option<SourceSpan> {
        let open = tokens.next_non_comment(self.name_index + 1)?;
        if !tokens[open].kind().is_left_square() {
            return None;
        }
        let close = tokens.matching_right_square(open)?;
        SourceSpan::new(tokens[open].span().start(), tokens[close].span().end()).ok()
    }

    /// Returns the comma or semicolon token after this declarator.
    #[must_use]
    pub(crate) fn initializer_separator(&self, tokens: TokenCursor<'_>) -> Option<usize> {
        let search = tokens;
        let separator = search.previous_non_comment(self.declarator_end)?;
        matches!(
            tokens[separator].kind(),
            TypedToken::Comma | TypedToken::Semicolon
        )
        .then_some(separator)
    }

    /// Returns the source span covering this full declaration statement.
    #[must_use]
    pub(crate) fn statement_span(&self, tokens: TokenCursor<'_>) -> Option<SourceSpan> {
        tokens.range_span(self.type_index, self.tail_start)
    }
}

impl LocalDeclaration {
    /// Creates a local declaration from a tokenizer declaration fact.
    #[must_use]
    pub(crate) fn from_declaration_fact(
        tokens: TokenCursor<'_>,
        fact: &DeclarationFact,
    ) -> Option<Self> {
        let scope_end = LocalDeclarationScope {
            declaration_start: fact.statement().start(),
            declaration_tail_start: fact.statement().end(),
        }
        .end(tokens);
        Some(Self {
            name: fact.name().clone(),
            ty: fact.ty().clone(),
            type_index: fact.type_index(),
            name_index: fact.name_index(),
            tail_start: fact.statement().end(),
            declarator_end: fact.declarator_end(),
            scope_end,
            name_span: tokens.get(fact.name_index())?.span(),
            type_span: tokens.get(fact.type_index())?.span(),
            initializer: fact.initializer(),
        })
    }
}

/// Scope extent for one local declaration.
#[derive(Clone, Copy)]
struct LocalDeclarationScope {
    /// Declaration start token.
    declaration_start: usize,
    /// First token after the declaration statement.
    declaration_tail_start: usize,
}

impl LocalDeclarationScope {
    /// Returns the first token outside the declaring block.
    fn end(self, tokens: TokenCursor<'_>) -> usize {
        if let Some(end) = self.for_loop_scope_end(tokens) {
            return end;
        }
        let search = tokens;
        let mut depth = 0usize;
        for index in (0..self.declaration_start).rev() {
            match tokens[index].kind() {
                TypedToken::RightBrace => depth += 1,
                TypedToken::LeftBrace => {
                    if depth == 0 {
                        return search.scope_end_after(index);
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        tokens.len()
    }

    /// Returns the end of an enclosing `for (...)` statement when the
    /// declaration appears in the initializer section.
    fn for_loop_scope_end(self, tokens: TokenCursor<'_>) -> Option<usize> {
        let open = self.enclosing_for_header_open(tokens)?;
        let search = tokens;
        let close = search.matching_right_paren(open)?;
        if self.declaration_tail_start > close {
            return None;
        }
        let Some(after_header) = search.next_non_comment(close + 1) else {
            return Some(tokens.len());
        };
        if matches!(tokens[after_header].kind(), TypedToken::LeftBrace) {
            return Some(search.scope_end_after(after_header));
        }
        search.controlled_statement_end_after(after_header)
    }

    /// Returns the opening parenthesis of a `for` header containing this
    /// declaration.
    fn enclosing_for_header_open(self, tokens: TokenCursor<'_>) -> Option<usize> {
        let mut depth = 0usize;
        for index in (0..self.declaration_start).rev() {
            match tokens[index].kind() {
                kind if kind.is_right_paren() => depth += 1,
                kind if kind.is_left_paren() && depth == 0 => {
                    let previous = tokens.previous_non_comment(index)?;
                    return tokens[previous]
                        .kind()
                        .is_keyword(KeywordType::For)
                        .then_some(index);
                }
                kind if kind.is_left_paren() => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }
}

//! Scoped local declaration scanning.

use super::{
    super::TokenSearch,
    ScopedDeclarationFact, ScopedDeclarationTypeMode,
    types::{DeclarationCandidate, DeclaratorInitializer, LocalTypeName},
};
use crate::{
    SourceSpan,
    lexer::{Token, TokenKind},
};

/// Simple local declaration.
#[derive(Clone, Copy)]
pub struct LocalDeclaration<'src> {
    /// Declared name token text.
    pub(super) name: &'src str,
    /// Declared type token text.
    pub(super) ty: &'src str,
    /// Declared type token index within the scanned token slice.
    pub(super) type_index: usize,
    /// Declared name token index within the scanned token slice.
    pub(super) name_index: usize,
    /// First token after the declaration statement.
    pub(super) tail_start: usize,
    /// First token after this declarator's initializer.
    pub(super) declarator_end: usize,
    /// First token outside the declaration's lexical scope.
    pub(super) scope_end: usize,
    /// Declared name token span.
    pub(super) name_span: SourceSpan,
    /// Declared type token span.
    pub(super) type_span: SourceSpan,
}

/// Candidate local declaration start token.
#[derive(Clone, Copy)]
pub struct LocalDeclarationStart<'tokens, 'src> {
    /// Tokens being scanned.
    pub(crate) tokens: &'tokens [Token<'src>],
    /// Candidate type token index.
    pub(crate) start: usize,
}

impl<'src> LocalDeclaration<'src> {
    /// Returns the local name.
    pub(crate) const fn name(self) -> &'src str {
        self.name
    }

    /// Returns the local type spelling.
    pub(crate) const fn ty(self) -> &'src str {
        self.ty
    }

    /// Returns the local type token index.
    pub(crate) const fn type_index(self) -> usize {
        self.type_index
    }

    /// Returns the local name token index.
    pub(crate) const fn name_index(self) -> usize {
        self.name_index
    }

    /// Returns the first token after the declaration statement.
    pub(crate) const fn tail_start(self) -> usize {
        self.tail_start
    }

    /// Returns the first token after this declarator's initializer.
    pub(crate) const fn declarator_end(self) -> usize {
        self.declarator_end
    }

    /// Returns first token outside this local declaration's scope.
    pub(crate) const fn scope_end(self) -> usize {
        self.scope_end
    }

    /// Returns the declaration name span.
    pub(crate) const fn name_span(self) -> SourceSpan {
        self.name_span
    }

    /// Returns the declaration type span.
    pub(crate) const fn type_span(self) -> SourceSpan {
        self.type_span
    }

    /// Returns this declarator's initializer range.
    pub(crate) fn initializer(self, tokens: &[Token<'_>]) -> Option<DeclaratorInitializer> {
        let separator = self.initializer_separator(tokens)?;
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut equals = None;
        for (index, token) in tokens
            .iter()
            .enumerate()
            .take(separator)
            .skip(self.name_index + 1)
        {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Punctuation('=')
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    equals = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let equals = equals?;
        let search = TokenSearch::new(tokens);
        let start = search.next_non_comment(equals + 1)?;
        let end = search.previous_non_comment(separator)?;
        (start <= end)
            .then(|| {
                SourceSpan::new(tokens[start].span.start(), tokens[end].span.end())
                    .ok()
                    .map(|span| DeclaratorInitializer { start, end, span })
            })
            .flatten()
    }

    /// Returns the comma or semicolon token after this declarator.
    pub(crate) fn initializer_separator(self, tokens: &[Token<'_>]) -> Option<usize> {
        let search = TokenSearch::new(tokens);
        let separator = search.previous_non_comment(self.declarator_end)?;
        matches!(
            tokens[separator].kind,
            TokenKind::Comma | TokenKind::Semicolon
        )
        .then_some(separator)
    }
}

impl<'src> TryFrom<LocalDeclarationStart<'_, 'src>> for LocalDeclaration<'src> {
    type Error = ();

    fn try_from(start: LocalDeclarationStart<'_, 'src>) -> Result<Self, Self::Error> {
        let tokens = start.tokens;
        if tokens
            .get(start.start)
            .is_none_or(|token| token.kind.is_comment())
        {
            return Err(());
        }
        let search = TokenSearch::new(tokens);
        let mut index = start.start;
        while tokens[index].kind.is_declaration_modifier() {
            index = search.next_non_comment(index + 1).ok_or(())?;
        }

        let Some(TokenKind::Identifier(ty)) = tokens.get(index).map(|token| token.kind) else {
            return Err(());
        };
        if !LocalTypeName::from(ty).is_local() {
            return Err(());
        }
        let candidate = DeclarationCandidate {
            tokens,
            start: start.start,
        };
        if candidate.is_function_parameter() {
            return Err(());
        }

        let name_index = search.next_non_comment(index + 1).ok_or(())?;
        let Some(TokenKind::Identifier(name)) = tokens.get(name_index).map(|token| token.kind)
        else {
            return Err(());
        };
        let Some(after_name) = search.next_non_comment(name_index + 1) else {
            return Err(());
        };
        if matches!(tokens[after_name].kind, TokenKind::LeftParen) {
            return Err(());
        }
        let tail = DeclarationTail::from(LocalDeclarationTailStart {
            tokens,
            start: name_index + 1,
        });
        let tail_start = tail.statement_end().ok_or(())?;
        let declarator_end = tail.declarator_end(tail_start).ok_or(())?;
        let scope_end = LocalDeclarationScope {
            tokens,
            declaration_start: start.start,
            declaration_tail_start: tail_start,
        }
        .end();
        let name_span = tokens[name_index].span;

        Ok(Self {
            name,
            ty,
            type_index: index,
            name_index,
            tail_start,
            declarator_end,
            scope_end,
            name_span,
            type_span: tokens[index].span,
        })
    }
}

/// Candidate scoped declaration fact start token.
#[derive(Clone, Copy)]
pub(super) struct ScopedDeclarationStart<'facts, 'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Known source-declared struct names.
    pub(super) struct_names: &'facts [&'src str],
    /// Accepted declaration type names.
    pub(super) type_mode: ScopedDeclarationTypeMode,
    /// Candidate type token index.
    pub(super) start: usize,
}

impl<'src> ScopedDeclarationStart<'_, '_, 'src> {
    /// Attempts to parse a declaration fact at `start`.
    pub(super) fn declaration(self) -> Option<ScopedDeclarationDeclarator<'src>> {
        let tokens = self.tokens;
        if tokens
            .get(self.start)
            .is_none_or(|token| token.kind.is_comment())
        {
            return None;
        }
        let search = TokenSearch::new(tokens);
        let mut type_index = self.start;
        while tokens[type_index].kind.is_declaration_modifier() {
            type_index = search.next_non_comment(type_index + 1)?;
        }

        let TokenKind::Identifier(ty) = tokens.get(type_index).map(|token| token.kind)? else {
            return None;
        };
        if !self.type_mode.accepts(ty, self.struct_names) {
            return None;
        }
        if (DeclarationCandidate {
            tokens,
            start: self.start,
        })
        .is_function_parameter()
        {
            return None;
        }

        let name_index = search.next_non_comment(type_index + 1)?;
        let TokenKind::Identifier(name) = tokens.get(name_index).map(|token| token.kind)? else {
            return None;
        };
        let after_name = search.next_non_comment(name_index + 1)?;
        if matches!(tokens[after_name].kind, TokenKind::LeftParen) {
            return None;
        }
        let tail = DeclarationTail::from(LocalDeclarationTailStart {
            tokens,
            start: name_index + 1,
        });
        let tail_start = tail.statement_end()?;
        let scope_end = LocalDeclarationScope {
            tokens,
            declaration_start: self.start,
            declaration_tail_start: tail_start,
        }
        .end();

        Some(ScopedDeclarationDeclarator {
            fact: ScopedDeclarationFact {
                name,
                ty,
                visible_start: name_index + 1,
                scope_end,
            },
            name_index,
            tail_start,
        })
    }
}

/// Declarators belonging to one scoped declaration fact statement.
pub(super) struct ScopedDeclarationDeclarators<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Next parsed declarator in the statement.
    pub(super) next: Option<ScopedDeclarationDeclarator<'src>>,
}

/// Internal scoped declarator with statement data needed for comma declarators.
#[derive(Clone, Copy)]
pub(super) struct ScopedDeclarationDeclarator<'src> {
    /// Shared declaration fact.
    fact: ScopedDeclarationFact<'src>,
    /// Declared name token index.
    name_index: usize,
    /// First token after the declaration statement.
    tail_start: usize,
}

impl<'src> Iterator for ScopedDeclarationDeclarators<'_, 'src> {
    type Item = ScopedDeclarationFact<'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next?;
        let next_name = NextDeclarator {
            tokens: self.tokens,
            start: current.name_index + 1,
            end: current.tail_start.saturating_sub(1),
        }
        .name_index();
        self.next = next_name.and_then(|name_index| {
            let TokenKind::Identifier(name) = self.tokens[name_index].kind else {
                return None;
            };
            Some(ScopedDeclarationDeclarator {
                fact: ScopedDeclarationFact {
                    name,
                    ty: current.fact.ty(),
                    visible_start: name_index + 1,
                    scope_end: current.fact.scope_end(),
                },
                name_index,
                tail_start: current.tail_start,
            })
        });
        Some(current.fact)
    }
}

/// Struct type names declared as `struct Name { ... };`.
pub(super) struct StructTypeNames<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Next token index to inspect.
    cursor: usize,
}

impl<'tokens, 'src> From<&'tokens [Token<'src>]> for StructTypeNames<'tokens, 'src> {
    fn from(tokens: &'tokens [Token<'src>]) -> Self {
        Self { tokens, cursor: 0 }
    }
}

impl<'src> Iterator for StructTypeNames<'_, 'src> {
    type Item = &'src str;

    fn next(&mut self) -> Option<Self::Item> {
        let search = TokenSearch::new(self.tokens);
        while self.cursor < self.tokens.len() {
            let struct_index = self.cursor;
            self.cursor += 1;
            if !matches!(
                self.tokens[struct_index].kind,
                TokenKind::Identifier("struct")
            ) {
                continue;
            }
            let Some(name_index) = search.next_non_comment(struct_index + 1) else {
                continue;
            };
            let TokenKind::Identifier(name) = self.tokens[name_index].kind else {
                continue;
            };
            let Some(open) = search.next_non_comment(name_index + 1) else {
                continue;
            };
            if matches!(self.tokens[open].kind, TokenKind::LeftBrace) {
                return Some(name);
            }
        }
        None
    }
}

/// Finds the next top-level declarator name in a declaration statement.
pub(super) struct NextDeclarator<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token to inspect after the current declarator name.
    pub(super) start: usize,
    /// Exclusive statement-end token bound.
    pub(super) end: usize,
}

impl NextDeclarator<'_, '_> {
    /// Returns the next declarator name token.
    pub(super) fn name_index(self) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.start..self.end {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::Comma if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    let name = TokenSearch::new(self.tokens).next_non_comment(index + 1)?;
                    if name < self.end && matches!(self.tokens[name].kind, TokenKind::Identifier(_))
                    {
                        return Some(name);
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// Scope extent for one local declaration.
#[derive(Clone, Copy)]
struct LocalDeclarationScope<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// Declaration start token.
    declaration_start: usize,
    /// First token after the declaration statement.
    declaration_tail_start: usize,
}

impl LocalDeclarationScope<'_, '_> {
    /// Returns the first token outside the declaring block.
    fn end(self) -> usize {
        if let Some(end) = self.for_loop_scope_end() {
            return end;
        }
        let mut depth = 0usize;
        for index in (0..self.declaration_start).rev() {
            match self.tokens[index].kind {
                TokenKind::RightBrace => depth += 1,
                TokenKind::LeftBrace => {
                    if depth == 0 {
                        return self.matching_scope_end_after(index);
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        self.tokens.len()
    }

    /// Returns the end of an enclosing `for (...)` statement when the
    /// declaration appears in the initializer section.
    fn for_loop_scope_end(self) -> Option<usize> {
        let open = self.enclosing_for_header_open()?;
        let close = self.matching_right_paren(open)?;
        if self.declaration_tail_start > close {
            return None;
        }
        let Some(after_header) = TokenSearch::new(self.tokens).next_non_comment(close + 1) else {
            return Some(self.tokens.len());
        };
        if matches!(self.tokens[after_header].kind, TokenKind::LeftBrace) {
            return Some(self.matching_scope_end_after(after_header));
        }
        self.statement_end_after(after_header)
    }

    /// Returns the opening parenthesis of a `for` header containing this
    /// declaration.
    fn enclosing_for_header_open(self) -> Option<usize> {
        let mut depth = 0usize;
        for index in (0..self.declaration_start).rev() {
            match self.tokens[index].kind {
                TokenKind::RightParen => depth += 1,
                TokenKind::LeftParen if depth == 0 => {
                    let previous = TokenSearch::new(self.tokens).previous_non_comment(index)?;
                    return matches!(self.tokens[previous].kind, TokenKind::Identifier("for"))
                        .then_some(index);
                }
                TokenKind::LeftParen => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }

    /// Returns the matching right parenthesis for `open`.
    fn matching_right_paren(self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the first token after the matching right brace for `open`.
    fn matching_scope_end_after(self, open: usize) -> usize {
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftBrace => depth += 1,
                TokenKind::RightBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        self.tokens.len()
    }

    /// Returns the token after a single-statement loop body.
    fn statement_end_after(self, start: usize) -> Option<usize> {
        ControlledStatementEnd {
            tokens: self.tokens,
        }
        .statement_end_after(start)
    }
}

/// Scanner for single controlled statements.
#[derive(Clone, Copy)]
pub struct ControlledStatementEnd<'tokens, 'src> {
    /// Tokens being scanned.
    pub(crate) tokens: &'tokens [Token<'src>],
}

impl ControlledStatementEnd<'_, '_> {
    /// Returns the first token outside the statement starting at `start`.
    pub(crate) fn statement_end_after(self, start: usize) -> Option<usize> {
        let start = TokenSearch::new(self.tokens).next_non_comment(start)?;
        match self.tokens[start].kind {
            TokenKind::LeftBrace => Some(self.matching_scope_end_after(start)),
            TokenKind::Identifier("if") => self.if_statement_end_after(start),
            TokenKind::Identifier("for" | "while" | "switch") => {
                self.header_body_statement_end_after(start)
            }
            TokenKind::Identifier("do") => self.do_while_statement_end_after(start),
            _ => self.simple_statement_end_after(start),
        }
    }

    /// Returns the first token outside an `if` statement and attached `else`.
    fn if_statement_end_after(self, start: usize) -> Option<usize> {
        let search = TokenSearch::new(self.tokens);
        let open = search.next_non_comment(start + 1)?;
        if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
            return self.simple_statement_end_after(start);
        }
        let close = self.matching_right_paren(open)?;
        let then_start = search.next_non_comment(close + 1)?;
        let then_end = self.statement_end_after(then_start)?;
        let Some(next) = search.next_non_comment(then_end) else {
            return Some(then_end);
        };
        if matches!(self.tokens[next].kind, TokenKind::Identifier("else")) {
            let else_start = search.next_non_comment(next + 1)?;
            self.statement_end_after(else_start)
        } else {
            Some(then_end)
        }
    }

    /// Returns the first token outside a header-controlled statement body.
    fn header_body_statement_end_after(self, start: usize) -> Option<usize> {
        let search = TokenSearch::new(self.tokens);
        let open = search.next_non_comment(start + 1)?;
        if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
            return self.simple_statement_end_after(start);
        }
        let close = self.matching_right_paren(open)?;
        let body_start = search.next_non_comment(close + 1)?;
        self.statement_end_after(body_start)
    }

    /// Returns the first token outside a `do ... while (...);` statement.
    fn do_while_statement_end_after(self, start: usize) -> Option<usize> {
        let search = TokenSearch::new(self.tokens);
        let body_start = search.next_non_comment(start + 1)?;
        let body_end = self.statement_end_after(body_start)?;
        let while_index = search.next_non_comment(body_end)?;
        if !matches!(
            self.tokens[while_index].kind,
            TokenKind::Identifier("while")
        ) {
            return Some(body_end);
        }
        let open = search.next_non_comment(while_index + 1)?;
        if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
            return self.simple_statement_end_after(while_index);
        }
        let close = self.matching_right_paren(open)?;
        let semicolon = search.next_non_comment(close + 1)?;
        matches!(self.tokens[semicolon].kind, TokenKind::Semicolon).then_some(semicolon + 1)
    }

    /// Returns the matching right parenthesis for `open`.
    fn matching_right_paren(self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the first token after the matching right brace for `open`.
    fn matching_scope_end_after(self, open: usize) -> usize {
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftBrace => depth += 1,
                TokenKind::RightBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        self.tokens.len()
    }

    /// Returns the token after the semicolon ending a simple statement.
    fn simple_statement_end_after(self, start: usize) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in start..self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Semicolon
                    if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 =>
                {
                    return Some(index + 1);
                }
                _ => {}
            }
        }
        Some(self.tokens.len())
    }
}

/// Candidate local declaration tail start.
#[derive(Clone, Copy)]
pub(super) struct LocalDeclarationTailStart<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// First token after the declared name.
    pub(super) start: usize,
}

/// Token range after a candidate local declaration name.
#[derive(Clone, Copy)]
pub(super) struct DeclarationTail<'tokens, 'src> {
    /// Tokens being scanned.
    tokens: &'tokens [Token<'src>],
    /// First tail token.
    start: usize,
}

impl<'tokens, 'src> From<LocalDeclarationTailStart<'tokens, 'src>>
    for DeclarationTail<'tokens, 'src>
{
    fn from(start: LocalDeclarationTailStart<'tokens, 'src>) -> Self {
        Self {
            tokens: start.tokens,
            start: start.start,
        }
    }
}

impl DeclarationTail<'_, '_> {
    /// Returns the first token after a declaration statement.
    pub(super) fn statement_end(self) -> Option<usize> {
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(self.start) {
            match token.kind {
                TokenKind::LeftParen | TokenKind::LeftBrace => depth += 1,
                TokenKind::RightParen | TokenKind::RightBrace => depth = depth.saturating_sub(1),
                TokenKind::Semicolon if depth == 0 => return Some(index + 1),
                _ => {}
            }
        }
        None
    }

    /// Returns the first token after this declarator's initializer.
    pub(super) fn declarator_end(self, statement_end: usize) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.start..statement_end.saturating_sub(1) {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Comma if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    return Some(index + 1);
                }
                _ => {}
            }
        }
        Some(statement_end)
    }
}

//! Cursors over lifetime-free typed token streams.

use std::{
    ops::{Index, Range},
    slice,
};

use super::{KeywordType, LiteralValue, OperatorType, Token, TokenIndexRange, TypedToken};
use crate::SourceSpan;

/// Borrowed cursor attached to token stream storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenCursor<'tokens> {
    /// Tokens being searched.
    tokens: &'tokens [Token],
}

impl<'tokens> TokenCursor<'tokens> {
    /// Creates a cursor over `tokens`.
    #[must_use]
    pub const fn new(tokens: &'tokens [Token]) -> Self {
        Self { tokens }
    }

    /// Returns the cursor token storage.
    #[must_use]
    pub const fn as_slice(self) -> &'tokens [Token] {
        self.tokens
    }

    /// Returns the token count.
    #[must_use]
    pub const fn len(self) -> usize {
        self.tokens.len()
    }

    /// Returns whether the cursor has no tokens.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.tokens.is_empty()
    }

    /// Returns the token at `index`, when present.
    #[must_use]
    pub fn get(self, index: usize) -> Option<&'tokens Token> {
        self.tokens.get(index)
    }

    /// Returns an iterator over tokens in source order.
    pub fn iter(self) -> slice::Iter<'tokens, Token> {
        self.tokens.iter()
    }

    /// Returns a cursor over a token subslice.
    #[must_use]
    pub fn range(self, range: Range<usize>) -> Self {
        Self {
            tokens: &self.tokens[range],
        }
    }

    /// Finds the next token at or after `start`.
    #[must_use]
    pub fn next_non_comment(self, start: usize) -> Option<usize> {
        (start < self.tokens.len()).then_some(start)
    }

    /// Finds the previous token before `before`.
    #[must_use]
    pub fn previous_non_comment(self, before: usize) -> Option<usize> {
        before
            .checked_sub(1)
            .filter(|index| *index < self.tokens.len())
    }

    /// Trims half-open token bounds to an inclusive non-empty range.
    #[must_use]
    pub fn non_comment_range(self, start: usize, end: usize) -> Option<(usize, usize)> {
        let start = self.next_non_comment(start)?;
        let end = self.previous_non_comment(end)?;
        (start <= end).then_some((start, end))
    }

    /// Creates a span from a token start and exclusive end bound.
    #[must_use]
    pub fn range_span(self, start: usize, end: usize) -> Option<SourceSpan> {
        let last = self.previous_non_comment(end)?;
        (start <= last)
            .then(|| {
                SourceSpan::new(
                    self.tokens[start].span().start(),
                    self.tokens[last].span().end(),
                )
                .ok()
            })
            .flatten()
    }

    /// Creates a source span covering a token segment.
    ///
    /// # Panics
    ///
    /// Panics if token spans are not in source order.
    #[must_use]
    pub fn segment_span(self) -> SourceSpan {
        let start = self.tokens.first().map_or(0, |token| token.span().start());
        let end = self.tokens.last().map_or(start, |token| token.span().end());
        SourceSpan::new(start, end).expect("token order should produce a valid source span")
    }

    /// Finds the token-index range whose spans are contained by `span`.
    #[must_use]
    pub fn contained_range(self, span: SourceSpan) -> TokenIndexRange {
        let start = self
            .tokens
            .iter()
            .position(|token| token.span().start() >= span.start())
            .unwrap_or(self.tokens.len());
        let end = self.tokens[start..]
            .iter()
            .position(|token| token.span().end() > span.end())
            .map_or(self.tokens.len(), |index| start + index);
        TokenIndexRange { start, end }
    }

    /// Finds the non-empty token-index range contained by the source byte
    /// bounds.
    #[must_use]
    pub fn contained_byte_range(self, start: usize, end: usize) -> Option<TokenIndexRange> {
        let start = self
            .tokens
            .iter()
            .position(|token| token.span().start() >= start)?;
        let end = self.tokens[start..]
            .iter()
            .position(|token| token.span().end() > end)
            .map_or(self.tokens.len(), |index| start + index);
        (start < end).then_some(TokenIndexRange { start, end })
    }

    /// Returns the first declarator name in a declaration token range.
    #[must_use]
    pub fn first_declarator_name(self, span: SourceSpan) -> Option<&'tokens str> {
        let first = self
            .tokens
            .iter()
            .position(|token| token.span().start() >= span.start())?;
        let semicolon = self
            .tokens
            .iter()
            .enumerate()
            .skip(first)
            .find_map(|(index, token)| {
                (token.span().end() <= span.end() && matches!(token.kind(), TypedToken::Semicolon))
                    .then_some(index)
            })?;
        let mut identifiers = self
            .tokens
            .iter()
            .take(semicolon)
            .skip(first)
            .filter_map(|token| {
                (!token.kind().is_declaration_modifier())
                    .then(|| token.kind().source_text())
                    .flatten()
            });
        let _type_name = identifiers.next()?;
        identifiers.next()
    }

    /// Parses an integer-valued field in a half-open token range.
    #[must_use]
    pub fn integer_field_value(self, start: usize, end: usize, name: &str) -> Option<u32> {
        for index in start..end {
            if !matches!(self.tokens[index].kind(), TypedToken::Identifier(identifier) if identifier == name)
            {
                continue;
            }
            let equals = self.next_non_comment(index + 1)?;
            if equals >= end || !self.tokens[equals].kind().is_simple_assignment_operator() {
                continue;
            }
            let value = self.next_non_comment(equals + 1)?;
            if value >= end {
                continue;
            }
            let TypedToken::Literal(LiteralValue::Number(text)) = self.tokens[value].kind() else {
                continue;
            };
            return text.parse::<u32>().ok();
        }
        None
    }

    /// Advances `cursor` to the next statement ending at a top-level
    /// semicolon, returning the statement start and semicolon token indices.
    #[must_use]
    pub fn next_semicolon_statement(self, cursor: &mut usize) -> Option<(usize, usize)> {
        let start = self.next_non_comment(*cursor)?;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match token.kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    *cursor = index + 1;
                    return Some((start, index));
                }
                TypedToken::LeftBrace | TypedToken::RightBrace
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    *cursor = index + 1;
                    return self.next_semicolon_statement(cursor);
                }
                _ => {}
            }
        }

        *cursor = self.tokens.len();
        None
    }

    /// Returns the first top-level semicolon at or after `start`.
    #[must_use]
    pub fn top_level_semicolon_from(self, start: usize) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            match token.kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    return Some(index);
                }
                TypedToken::LeftBrace | TypedToken::RightBrace
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    /// Splits a half-open token range on top-level semicolons.
    #[must_use]
    pub fn split_top_level_semicolon_sections(
        self,
        range: TokenIndexRange,
    ) -> Option<Vec<TokenIndexRange>> {
        let mut sections = Vec::new();
        let mut start = range.start();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self
            .tokens
            .iter()
            .enumerate()
            .take(range.end())
            .skip(range.start())
        {
            match token.kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    sections.push(TokenIndexRange { start, end: index });
                    start = index + 1;
                }
                _ => {}
            }
        }
        sections.push(TokenIndexRange {
            start,
            end: range.end(),
        });
        Some(sections)
    }

    /// Finds the next operator token at top level inside a half-open range.
    #[must_use]
    pub fn next_top_level_operator(
        self,
        range: TokenIndexRange,
        operator: OperatorType,
    ) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for (index, token) in self
            .tokens
            .iter()
            .enumerate()
            .take(range.end())
            .skip(range.start())
        {
            match token.kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Operator(found)
                    if *found == operator && paren_depth == 0 && bracket_depth == 0 =>
                {
                    return Some(index);
                }
                _ => {}
            }
        }
        None
    }

    /// Returns whether `+` or `-` starts a unary expression in the current
    /// inclusive token range.
    #[must_use]
    pub fn is_unary_sign_in_range(self, index: usize, start: usize) -> bool {
        if !matches!(
            self.tokens[index].kind(),
            TypedToken::Operator(operator) if operator.is_additive()
        ) {
            return false;
        }
        let Some(previous) = self.previous_non_comment(index) else {
            return true;
        };
        previous < start
            || matches!(self.tokens[previous].kind(), TypedToken::Comma)
            || self.tokens[previous].kind().is_left_paren()
            || matches!(
                self.tokens[previous].kind(),
                TypedToken::Operator(operator) if operator.is_unary_boundary()
            )
    }

    /// Advances `cursor` to the next syntactic `for (...)` header, returning
    /// opening and closing parenthesis token indices.
    #[must_use]
    pub fn next_for_loop_header(self, cursor: &mut usize) -> Option<(usize, usize)> {
        while *cursor < self.tokens.len() {
            let for_index = *cursor;
            *cursor += 1;
            if !self.tokens[for_index].kind().is_keyword(KeywordType::For) {
                continue;
            }
            let open = self.next_non_comment(for_index + 1)?;
            if !self.tokens[open].kind().is_left_paren() {
                continue;
            }
            let close = self.matching_right_paren(open)?;
            *cursor = close + 1;
            return Some((open, close));
        }
        None
    }

    /// Returns the exclusive end of a top-level comma-delimited segment.
    #[must_use]
    pub fn top_level_comma_segment_end(self, start: usize, close: usize) -> usize {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        for index in start..close {
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.saturating_sub(1),
                TypedToken::LeftBrace => brace_depth += 1,
                TypedToken::RightBrace => brace_depth = brace_depth.saturating_sub(1),
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TypedToken::Comma if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return index;
                }
                _ => {}
            }
        }
        close
    }

    /// Finds the right parenthesis that matches `open`.
    #[must_use]
    pub fn matching_right_paren(self, open: usize) -> Option<usize> {
        self.matching(open, TokenMatcher::LeftParen, TokenMatcher::RightParen)
    }

    /// Finds the innermost left parenthesis before `start`.
    #[must_use]
    pub fn enclosing_left_paren_before(self, start: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in (0..start).rev() {
            match self.tokens[index].kind() {
                kind if kind.is_right_paren() => depth += 1,
                kind if kind.is_left_paren() && depth == 0 => return Some(index),
                kind if kind.is_left_paren() => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }

    /// Finds the right brace that matches `open`.
    #[must_use]
    pub fn matching_right_brace(self, open: usize) -> Option<usize> {
        self.matching(open, TokenMatcher::LeftBrace, TokenMatcher::RightBrace)
    }

    /// Returns the first token after the matching right brace for `open`.
    #[must_use]
    pub fn scope_end_after(self, open: usize) -> usize {
        self.matching_right_brace(open)
            .map_or(self.tokens.len(), |close| close + 1)
    }

    /// Returns the first token outside the statement starting at `start`.
    #[must_use]
    pub fn controlled_statement_end_after(self, start: usize) -> Option<usize> {
        let start = self.next_non_comment(start)?;
        match self.tokens[start].kind() {
            TypedToken::LeftBrace => Some(self.scope_end_after(start)),
            kind if kind.is_keyword(KeywordType::If) => self.if_statement_end_after(start),
            kind if kind.is_keyword(KeywordType::For)
                || kind.is_keyword(KeywordType::While)
                || kind.is_keyword(KeywordType::Switch) =>
            {
                self.header_body_statement_end_after(start)
            }
            kind if kind.is_keyword(KeywordType::Do) => self.do_while_statement_end_after(start),
            _ => self.simple_statement_end_after(start),
        }
    }

    /// Finds the right square bracket that matches `open`.
    #[must_use]
    pub fn matching_right_square(self, open: usize) -> Option<usize> {
        self.matching(open, TokenMatcher::LeftSquare, TokenMatcher::RightSquare)
    }

    /// Returns the non-empty expression enclosed by a square-bracket pair
    /// before `limit`.
    #[must_use]
    pub fn square_bracket_expression(self, open: usize, limit: usize) -> Option<TokenIndexRange> {
        if !self.tokens[open].kind().is_left_square() {
            return None;
        }
        let close = self.matching_right_square(open)?;
        if close >= limit {
            return None;
        }
        let start = self.next_non_comment(open + 1)?;
        let end = self.previous_non_comment(close)?;
        (start <= end).then_some(TokenIndexRange::new(start, end + 1))
    }

    /// Iterates identifier tokens in source order.
    pub fn identifiers(self) -> impl Iterator<Item = IdentifierToken<'tokens>> + 'tokens {
        self.tokens.iter().filter_map(|token| {
            let text = match token.kind() {
                TypedToken::Identifier(text) => text.as_str(),
                TypedToken::Keyword(keyword) => keyword.text(),
                TypedToken::TypeMark(primitive) => primitive.text(),
                _ => return None,
            };
            Some(IdentifierToken {
                text,
                span: token.span(),
            })
        })
    }

    /// Finds the matching right delimiter using a nesting counter.
    fn matching(self, open: usize, left: TokenMatcher, right: TokenMatcher) -> Option<usize> {
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(open) {
            if left.matches(token.kind()) {
                depth += 1;
            } else if right.matches(token.kind()) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
        }
        None
    }

    /// Returns the first token outside an `if` statement and attached `else`.
    fn if_statement_end_after(self, start: usize) -> Option<usize> {
        let open = self.next_non_comment(start + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.simple_statement_end_after(start);
        }
        let close = self.matching_right_paren(open)?;
        let then_start = self.next_non_comment(close + 1)?;
        let then_end = self.controlled_statement_end_after(then_start)?;
        let Some(next) = self.next_non_comment(then_end) else {
            return Some(then_end);
        };
        if self.tokens[next].kind().is_keyword(KeywordType::Else) {
            let else_start = self.next_non_comment(next + 1)?;
            self.controlled_statement_end_after(else_start)
        } else {
            Some(then_end)
        }
    }

    /// Returns the first token outside a header-controlled statement body.
    fn header_body_statement_end_after(self, start: usize) -> Option<usize> {
        let open = self.next_non_comment(start + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.simple_statement_end_after(start);
        }
        let close = self.matching_right_paren(open)?;
        let body_start = self.next_non_comment(close + 1)?;
        self.controlled_statement_end_after(body_start)
    }

    /// Returns the first token outside a `do ... while (...);` statement.
    fn do_while_statement_end_after(self, start: usize) -> Option<usize> {
        let body_start = self.next_non_comment(start + 1)?;
        let body_end = self.controlled_statement_end_after(body_start)?;
        let while_index = self.next_non_comment(body_end)?;
        if !self.tokens[while_index]
            .kind()
            .is_keyword(KeywordType::While)
        {
            return Some(body_end);
        }
        let open = self.next_non_comment(while_index + 1)?;
        if !self.tokens[open].kind().is_left_paren() {
            return self.simple_statement_end_after(while_index);
        }
        let close = self.matching_right_paren(open)?;
        let semicolon = self.next_non_comment(close + 1)?;
        matches!(self.tokens[semicolon].kind(), TypedToken::Semicolon).then_some(semicolon + 1)
    }

    /// Returns the token after the semicolon ending a simple statement.
    fn simple_statement_end_after(self, start: usize) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in start..self.tokens.len() {
            match self.tokens[index].kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.checked_sub(1)?,
                TypedToken::LeftBrace => brace_depth += 1,
                TypedToken::RightBrace => brace_depth = brace_depth.checked_sub(1)?,
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.checked_sub(1)?;
                }
                TypedToken::Semicolon
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

impl Index<usize> for TokenCursor<'_> {
    type Output = Token;

    fn index(&self, index: usize) -> &Self::Output {
        &self.tokens[index]
    }
}

impl Index<Range<usize>> for TokenCursor<'_> {
    type Output = [Token];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.tokens[index]
    }
}

/// Identifier token text paired with its source span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentifierToken<'tokens> {
    /// Identifier spelling.
    text: &'tokens str,
    /// Span covering the identifier text.
    span: SourceSpan,
}

impl<'tokens> IdentifierToken<'tokens> {
    /// Returns the identifier spelling.
    #[must_use]
    pub const fn text(self) -> &'tokens str {
        self.text
    }

    /// Returns the identifier source span.
    #[must_use]
    pub const fn span(self) -> SourceSpan {
        self.span
    }
}

/// Delimiter token matched by token stream searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenMatcher {
    /// Left brace token.
    LeftBrace,
    /// Right brace token.
    RightBrace,
    /// Left parenthesis token.
    LeftParen,
    /// Right parenthesis token.
    RightParen,
    /// Left square bracket token.
    LeftSquare,
    /// Right square bracket token.
    RightSquare,
}

impl TokenMatcher {
    /// Returns whether `kind` matches this delimiter.
    fn matches(self, kind: &TypedToken) -> bool {
        match self {
            Self::LeftBrace => matches!(kind, TypedToken::LeftBrace),
            Self::RightBrace => matches!(kind, TypedToken::RightBrace),
            Self::LeftParen => kind.is_left_paren(),
            Self::RightParen => kind.is_right_paren(),
            Self::LeftSquare => kind.is_left_square(),
            Self::RightSquare => kind.is_right_square(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tokenizer::TokenStream;

    #[test]
    fn cursor_range_span_uses_typed_tokens() {
        let stream = TokenStream::lex("value // comment").expect("source lexes");
        let tokens = stream.as_slice();

        let span = stream
            .cursor()
            .range_span(0, tokens.len())
            .expect("range has span");

        assert_eq!(span.start(), 0);
        assert_eq!(span.end(), 5);
    }

    #[test]
    fn cursor_range_span_rejects_empty_range() {
        let stream = TokenStream::lex("// comment").expect("source lexes");

        assert_eq!(stream.cursor().range_span(0, stream.len()), None);
    }
}

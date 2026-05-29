use crate::{
    ShaderResult, SourceSpan,
    legalizer::{Fixup, TokenSearch, tokens::BalancedTokens},
    lexer::{Token, TokenKind},
};

/// Token stream of syntactic `for (...)` headers.
pub(super) struct ForLoopHeaders<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [Token<'src>],
    /// Next token index to inspect.
    cursor: usize,
}

impl<'tokens, 'src> From<&'tokens [Token<'src>]> for ForLoopHeaders<'tokens, 'src> {
    fn from(tokens: &'tokens [Token<'src>]) -> Self {
        Self { tokens, cursor: 0 }
    }
}

impl<'tokens, 'src> Iterator for ForLoopHeaders<'tokens, 'src> {
    type Item = ForLoopHeader<'tokens, 'src>;

    fn next(&mut self) -> Option<Self::Item> {
        let search = TokenSearch::new(self.tokens);
        while self.cursor < self.tokens.len() {
            let for_index = self.cursor;
            self.cursor += 1;
            if !matches!(self.tokens[for_index].kind, TokenKind::Identifier("for")) {
                continue;
            }
            let open = search.next_non_comment(for_index + 1)?;
            if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
                continue;
            }
            let close = BalancedTokens::new(self.tokens).matching_right_paren(open)?;
            self.cursor = close + 1;
            return Some(ForLoopHeader {
                tokens: self.tokens,
                open,
                close,
            });
        }
        None
    }
}

/// One `for (...)` header token range.
#[derive(Clone, Copy)]
pub(super) struct ForLoopHeader<'tokens, 'src> {
    /// Full token stream.
    tokens: &'tokens [Token<'src>],
    /// Opening parenthesis token.
    open: usize,
    /// Closing parenthesis token.
    close: usize,
}

impl ForLoopHeader<'_, '_> {
    /// Splits the header into init, condition, and increment sections.
    fn sections(self) -> Option<ForLoopSections> {
        let mut semicolons = Vec::with_capacity(2);
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.open + 1..self.close {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    semicolons.push(index);
                }
                _ => {}
            }
        }
        (semicolons.len() == 2).then_some(ForLoopSections {
            init_start: self.open + 1,
            init_end: semicolons[0],
            condition_start: semicolons[0] + 1,
            condition_end: semicolons[1],
        })
    }
}

/// Token ranges for relevant `for` header sections.
#[derive(Clone, Copy)]
struct ForLoopSections {
    /// Initializer start token.
    init_start: usize,
    /// Initializer end token, exclusive.
    init_end: usize,
    /// Condition start token.
    condition_start: usize,
    /// Condition end token, exclusive.
    condition_end: usize,
}

/// Integer `for` bounds requiring explicit casts.
pub(super) struct IntegerForLoopBounds<'tokens, 'src> {
    /// Header being inspected.
    pub(super) header: ForLoopHeader<'tokens, 'src>,
}

impl IntegerForLoopBounds<'_, '_> {
    /// Emits fixups for integer init RHS and comparison RHS.
    pub(super) fn fixups(self) -> ShaderResult<Vec<Fixup>> {
        let Some(sections) = self.header.sections() else {
            return Ok(Vec::new());
        };
        let mut fixups = Vec::new();
        let Some(initializer) = IntegerForLoopInitializer {
            header: self.header,
            start: sections.init_start,
            end: sections.init_end,
        }
        .candidate() else {
            return Ok(fixups);
        };
        IntegerCastFixup::around(initializer.rhs).push_to(&mut fixups)?;
        if let Some(span) = (IntegerForLoopCondition {
            header: self.header,
            start: sections.condition_start,
            end: sections.condition_end,
        }
        .rhs_span(initializer.name))
        {
            IntegerCastFixup::around(span).push_to(&mut fixups)?;
        }
        Ok(fixups)
    }
}

/// Parsed integer loop initializer.
struct IntegerLoopInitializer<'src> {
    /// Loop variable name.
    name: &'src str,
    /// Initializer RHS span.
    rhs: SourceSpan,
}

/// Integer loop initializer candidate.
struct IntegerForLoopInitializer<'tokens, 'src> {
    /// Header containing the initializer.
    header: ForLoopHeader<'tokens, 'src>,
    /// Section start token.
    start: usize,
    /// Section end token, exclusive.
    end: usize,
}

impl<'src> IntegerForLoopInitializer<'_, 'src> {
    /// Returns the loop variable and RHS span when the initializer is `int name
    /// = expr`.
    fn candidate(self) -> Option<IntegerLoopInitializer<'src>> {
        let tokens = self.header.tokens;
        let search = TokenSearch::new(tokens);
        let ty = search.next_non_comment(self.start)?;
        if ty >= self.end || !matches!(tokens[ty].kind, TokenKind::Identifier("int")) {
            return None;
        }
        let name = search.next_non_comment(ty + 1)?;
        let TokenKind::Identifier(name_text) = tokens[name].kind else {
            return None;
        };
        if name >= self.end {
            return None;
        }
        let equals = search.next_non_comment(name + 1)?;
        if equals >= self.end || !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
            return None;
        }
        Some(IntegerLoopInitializer {
            name: name_text,
            rhs: tokens.range_span(search.next_non_comment(equals + 1)?, self.end)?,
        })
    }
}

/// Integer loop condition candidate.
struct IntegerForLoopCondition<'tokens, 'src> {
    /// Header containing the condition.
    header: ForLoopHeader<'tokens, 'src>,
    /// Section start token.
    start: usize,
    /// Section end token, exclusive.
    end: usize,
}

impl IntegerForLoopCondition<'_, '_> {
    /// Returns the RHS span when the condition compares the integer loop
    /// variable.
    fn rhs_span(self, loop_variable: &str) -> Option<SourceSpan> {
        let tokens = self.header.tokens;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for index in self.start..self.end {
            match tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Punctuation('<' | '>') if paren_depth == 0 && bracket_depth == 0 => {
                    let search = TokenSearch::new(tokens);
                    let lhs_start = search.next_non_comment(self.start)?;
                    let lhs_end = search.previous_non_comment(index)?;
                    if lhs_start != lhs_end
                        || !matches!(
                            tokens[lhs_start].kind,
                            TokenKind::Identifier(name) if name == loop_variable
                        )
                    {
                        return None;
                    }
                    let mut rhs = search.next_non_comment(index + 1)?;
                    if rhs < self.end && matches!(tokens[rhs].kind, TokenKind::Punctuation('=')) {
                        rhs = search.next_non_comment(rhs + 1)?;
                    }
                    return tokens.range_span(rhs, self.end);
                }
                _ => {}
            }
        }
        None
    }
}

/// Span construction for token slices.
trait TokenRangeSpan {
    /// Creates a span from a token start and exclusive end bound.
    fn range_span(&self, start: usize, end: usize) -> Option<SourceSpan>;
}

impl TokenRangeSpan for [Token<'_>] {
    fn range_span(&self, start: usize, end: usize) -> Option<SourceSpan> {
        let last = TokenSearch::new(self).previous_non_comment(end)?;
        (start <= last)
            .then(|| SourceSpan::new(self[start].span.start(), self[last].span.end()).ok())
            .flatten()
    }
}

/// Insertion fixups that wrap a source span in an `int(...)` cast.
struct IntegerCastFixup {
    /// Source span being wrapped.
    span: SourceSpan,
}

impl IntegerCastFixup {
    /// Creates a cast fixup around `span`.
    const fn around(span: SourceSpan) -> Self {
        Self { span }
    }

    /// Appends the insertion fixups.
    fn push_to(self, fixups: &mut Vec<Fixup>) -> ShaderResult<()> {
        let start = SourceSpan::new(self.span.start(), self.span.start())?;
        let end = SourceSpan::new(self.span.end(), self.span.end())?;
        fixups.push(Fixup::insert(start, "int(".to_owned()));
        fixups.push(Fixup::insert(end, ")".to_owned()));
        Ok(())
    }
}

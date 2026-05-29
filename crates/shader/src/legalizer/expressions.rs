//! Expression replacement templates rendered from settled child fixups.

use super::{
    fixups::{Fixup, FixupReplacement, SourceSpanExt},
    tokens::{BalancedTokens, TokenSearch},
};
use crate::{
    ShaderResult, SourceSpan,
    lexer::{Token, TokenKind},
    syntax::ShaderSourceText,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Replacement text assembled from literal text and rendered source spans.
pub struct ExpressionReplacement {
    /// Ordered replacement parts.
    parts: Vec<ExpressionPart>,
}

impl ExpressionReplacement {
    /// Creates an empty expression replacement.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self { parts: Vec::new() }
    }

    /// Appends literal replacement text.
    #[must_use]
    pub(crate) fn with_text(mut self, text: impl Into<String>) -> Self {
        self.parts.push(ExpressionPart::Text(text.into()));
        self
    }

    /// Appends a source span that should be rendered with child fixups applied.
    #[must_use]
    pub(crate) fn with_source(mut self, span: SourceSpan) -> Self {
        self.parts.push(ExpressionPart::Source(span));
        self
    }

    /// Appends another expression replacement.
    #[must_use]
    pub(crate) fn with_replacement(mut self, replacement: Self) -> Self {
        self.parts.extend(replacement.parts);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One component in an expression replacement.
enum ExpressionPart {
    /// Literal replacement text.
    Text(String),
    /// Source span rendered through child fixups.
    Source(SourceSpan),
}

/// Renders source spans while applying already-collected nested fixups.
pub(super) struct ExpressionRenderer<'fixups, 'src> {
    /// Original shader source.
    pub(super) source: ShaderSourceText<'src>,
    /// Ordered fixups available to child expressions.
    pub(super) fixups: &'fixups [Fixup],
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

/// Index over token sequences that look like function calls.
pub struct FunctionCallIndex<'module, 'src> {
    /// Tokens searched for identifier-open-paren pairs.
    tokens: &'module [Token<'src>],
}

impl<'module, 'src> FunctionCallIndex<'module, 'src> {
    /// Creates a call index over a token slice.
    pub(crate) const fn new(tokens: &'module [Token<'src>]) -> Self {
        Self { tokens }
    }

    /// Iterates syntactic function calls with balanced parenthesis ranges.
    pub(crate) fn iter(&self) -> impl Iterator<Item = FunctionCall<'module, 'src>> + '_ {
        self.tokens.iter().enumerate().filter_map(|(index, token)| {
            let TokenKind::Identifier(_) = token.kind else {
                return None;
            };
            let open_index = TokenSearch::new(self.tokens).next_non_comment(index + 1)?;
            if !matches!(self.tokens[open_index].kind, TokenKind::LeftParen) {
                return None;
            }
            let close_index = BalancedTokens::new(self.tokens).matching_right_paren(open_index)?;
            Some(FunctionCall {
                tokens: self.tokens,
                name_index: index,
                open_index,
                close_index,
            })
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token range for one syntactic function call.
pub struct FunctionCall<'module, 'src> {
    /// Token slice containing the call.
    pub(crate) tokens: &'module [Token<'src>],
    /// Index of the function name token.
    pub(crate) name_index: usize,
    /// Index of the opening parenthesis.
    pub(crate) open_index: usize,
    /// Index of the matching closing parenthesis.
    pub(crate) close_index: usize,
}

impl<'src> FunctionCall<'_, 'src> {
    /// Returns the function name.
    pub(crate) fn name(self) -> &'src str {
        let TokenKind::Identifier(name) = self.tokens[self.name_index].kind else {
            return "";
        };
        name
    }

    /// Returns the source span for the function name.
    pub(crate) const fn name_span(self) -> SourceSpan {
        self.tokens[self.name_index].span
    }

    /// Returns the span covering the entire call expression.
    pub(crate) fn span(self) -> SourceSpan {
        SourceSpan::new(
            self.tokens[self.name_index].span.start(),
            self.tokens[self.close_index].span.end(),
        )
        .unwrap_or(self.name_span())
    }

    /// Counts top-level call arguments, ignoring nested parentheses.
    pub(crate) fn argument_count(self) -> usize {
        let mut depth = 0usize;
        let mut count = 0usize;
        let mut saw_argument = false;
        for token in &self.tokens[self.open_index + 1..self.close_index] {
            match token.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => count += 1,
                TokenKind::Comment(_) => {}
                _ => saw_argument = true,
            }
        }
        if saw_argument { count + 1 } else { 0 }
    }

    /// Returns whether this call is immediately followed by a field swizzle.
    pub(crate) fn has_trailing_swizzle(self) -> bool {
        let search = TokenSearch::new(self.tokens);
        let Some(dot) = search.next_non_comment(self.close_index + 1) else {
            return false;
        };
        if !matches!(self.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return false;
        }
        let Some(field) = search.next_non_comment(dot + 1) else {
            return false;
        };
        matches!(self.tokens[field].kind, TokenKind::Identifier(_))
    }

    /// Returns token boundaries for the first top-level call argument.
    pub(crate) fn first_argument(self) -> Option<FirstCallArgument> {
        let search = TokenSearch::new(self.tokens);
        let start = search.next_non_comment(self.open_index + 1)?;
        if start >= self.close_index {
            return None;
        }

        let mut depth = 0usize;
        for index in start..self.close_index {
            match self.tokens[index].kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => {
                    return Some(FirstCallArgument {
                        start,
                        end: index,
                        comma: Some(index),
                        close: self.close_index,
                    });
                }
                _ => {}
            }
        }

        Some(FirstCallArgument {
            start,
            end: self.close_index,
            comma: None,
            close: self.close_index,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token range for the first argument of a function call.
pub struct FirstCallArgument {
    /// First token of the argument.
    start: usize,
    /// Exclusive token index ending the argument.
    end: usize,
    /// Comma separating the first argument from the remaining arguments.
    comma: Option<usize>,
    /// Closing parenthesis token of the enclosing call.
    close: usize,
}

impl FirstCallArgument {
    /// Returns the first argument start token.
    pub(crate) const fn start(self) -> usize {
        self.start
    }

    /// Returns the source span for the first argument.
    pub(crate) fn argument_span(self, tokens: &[Token<'_>]) -> Option<SourceSpan> {
        let end = TokenSearch::new(tokens).previous_non_comment(self.end)?;
        if end < self.start {
            return None;
        }

        SourceSpan::new(tokens[self.start].span.start(), tokens[end].span.end()).ok()
    }

    /// Returns the source span for arguments after the first argument.
    pub(crate) fn remaining_argument_span(self, tokens: &[Token<'_>]) -> Option<SourceSpan> {
        let comma = self.comma?;
        let search = TokenSearch::new(tokens);
        let start = search.next_non_comment(comma + 1)?;
        let end = search.previous_non_comment(self.close)?;
        if start > end {
            return None;
        }

        SourceSpan::new(tokens[start].span.start(), tokens[end].span.end()).ok()
    }
}

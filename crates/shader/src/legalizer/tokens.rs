//! Token navigation and token-backed semantic detectors.

use crate::{
    ShaderResult, SourceSpan,
    lexer::{Token, TokenKind, TokenStream, TokenStreamExt},
};

/// Parsed preprocessor define directive.
struct DefineDirective<'src> {
    /// Full directive text.
    text: &'src str,
}

impl DefineDirective<'_> {
    /// Returns the byte offset of the macro replacement body.
    fn body_start(self) -> Option<usize> {
        let after_hash = self.text.strip_prefix('#')?;
        let define_start =
            self.text.len() - after_hash.len() + after_hash.len() - after_hash.trim_start().len();
        let rest = self.text[define_start..].strip_prefix("define")?;
        if rest
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return None;
        }

        let mut cursor = self.text.len() - rest.len();
        cursor += rest.len() - rest.trim_start().len();
        let bytes = self.text.as_bytes();
        let first = *bytes.get(cursor)?;
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return None;
        }
        cursor += 1;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            cursor += 1;
        }

        if self
            .text
            .as_bytes()
            .get(cursor)
            .is_some_and(|byte| *byte == b'(')
        {
            let mut depth = 0usize;
            while let Some(byte) = bytes.get(cursor) {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth = depth.checked_sub(1)?;
                        cursor += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    _ => {}
                }
                cursor += 1;
            }
            if depth != 0 {
                return None;
            }
        }
        cursor += self.text[cursor..]
            .bytes()
            .take_while(u8::is_ascii_whitespace)
            .count();
        Some(cursor)
    }
}

/// Token-backed macro directive parsing.
pub trait DefineDirectiveTokenExt<'src> {
    /// Lexes the replacement body of a `#define` directive into source-mapped
    /// tokens, when the directive has a non-empty body.
    fn define_body_tokens(self) -> ShaderResult<Option<Vec<Token<'src>>>>;
}

impl<'src> DefineDirectiveTokenExt<'src> for Token<'src> {
    fn define_body_tokens(self) -> ShaderResult<Option<Vec<Token<'src>>>> {
        let TokenKind::Directive(text) = self.kind else {
            return Ok(None);
        };
        let Some(body_start) = DefineDirective { text }.body_start() else {
            return Ok(None);
        };
        let body = &text[body_start..];
        if body.trim().is_empty() {
            return Ok(None);
        }

        let source_offset = self.span.start() + body_start;
        let tokens = TokenStream::lex(body)?
            .into_iter()
            .map(|token| {
                SourceSpan::new(
                    source_offset + token.span.start(),
                    source_offset + token.span.end(),
                )
                .map(|span| Token {
                    kind: token.kind,
                    span,
                })
            })
            .collect::<ShaderResult<Vec<_>>>()?;
        Ok(Some(tokens))
    }
}

#[derive(Clone, Copy)]
/// Borrowed token stream helper for identifier-only scans.
pub struct TokenView<'module, 'src> {
    /// Tokens from the parsed shader module.
    pub(crate) tokens: &'module [Token<'src>],
}

impl<'module, 'src> TokenView<'module, 'src> {
    /// Iterates identifier tokens while ignoring comments, strings, and
    /// punctuation.
    pub(crate) fn identifiers(self) -> impl Iterator<Item = IdentifierToken<'src>> + 'module {
        self.tokens.iter().filter_map(|token| {
            let TokenKind::Identifier(text) = token.kind else {
                return None;
            };
            Some(IdentifierToken {
                text,
                span: token.span,
            })
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Identifier token text paired with its source span.
pub struct IdentifierToken<'src> {
    /// Identifier spelling.
    text: &'src str,
    /// Span covering the identifier text.
    span: SourceSpan,
}

impl<'src> IdentifierToken<'src> {
    /// Returns the identifier spelling.
    pub(crate) const fn text(self) -> &'src str {
        self.text
    }

    /// Returns the identifier source span.
    pub(crate) const fn span(self) -> SourceSpan {
        self.span
    }
}

/// Detector for writes to a specific stage input variable.
pub struct StageInputWrite<'module, 'src> {
    /// Tokens searched for assignment forms.
    pub(crate) tokens: &'module [Token<'src>],
    /// Stage input name being checked.
    pub(crate) name: &'src str,
}

impl<'src> StageInputWrite<'_, 'src> {
    /// Returns whether any token sequence writes to the target input.
    pub(crate) fn exists(&self) -> bool {
        self.tokens
            .iter()
            .enumerate()
            .any(|(index, token)| self.writes_at(index, token))
    }

    /// Returns whether the identifier at `index` starts a write expression.
    fn writes_at(&self, index: usize, token: &Token<'src>) -> bool {
        if !matches!(token.kind, TokenKind::Identifier(text) if text == self.name) {
            return false;
        }

        let search = TokenSearch::new(self.tokens);
        if let Some(previous) = search.previous_non_comment(index)
            && matches!(
                self.tokens[previous].kind,
                TokenKind::Punctuation('+' | '-')
            )
            && let Some(before_previous) = search.previous_non_comment(previous)
        {
            return matches!(
                (
                    self.tokens[before_previous].kind,
                    self.tokens[previous].kind
                ),
                (TokenKind::Punctuation('+'), TokenKind::Punctuation('+'))
                    | (TokenKind::Punctuation('-'), TokenKind::Punctuation('-'))
            );
        }

        let Some(next) = search.next_non_comment(index + 1) else {
            return false;
        };
        WriteTail {
            tokens: self.tokens,
            start: next,
        }
        .writes()
    }
}

/// Cursor over the token tail after an identifier to classify writes.
struct WriteTail<'module, 'src> {
    /// Tokens containing the identifier tail.
    tokens: &'module [Token<'src>],
    /// First non-comment token after the identifier.
    start: usize,
}

impl WriteTail<'_, '_> {
    /// Returns whether the tail is assignment-like.
    fn writes(self) -> bool {
        let search = TokenSearch::new(self.tokens);
        let mut index = self.start;
        loop {
            match self.tokens[index].kind {
                TokenKind::Punctuation('=') => {
                    return search.next_non_comment(index + 1).is_none_or(|next| {
                        !matches!(self.tokens[next].kind, TokenKind::Punctuation('='))
                    });
                }
                TokenKind::Punctuation('+' | '-') => {
                    let Some(next) = search.next_non_comment(index + 1) else {
                        return false;
                    };
                    if matches!(
                        (self.tokens[index].kind, self.tokens[next].kind),
                        (TokenKind::Punctuation('+'), TokenKind::Punctuation('+'))
                            | (TokenKind::Punctuation('-'), TokenKind::Punctuation('-'))
                    ) {
                        return true;
                    }
                    return matches!(self.tokens[next].kind, TokenKind::Punctuation('='));
                }
                TokenKind::Punctuation('*' | '/' | '%') => {
                    let Some(next) = search.next_non_comment(index + 1) else {
                        return false;
                    };
                    return matches!(self.tokens[next].kind, TokenKind::Punctuation('='));
                }
                TokenKind::Punctuation('.') => {
                    let Some(next) = search.next_non_comment(index + 1) else {
                        return false;
                    };
                    if !matches!(self.tokens[next].kind, TokenKind::Identifier(_)) {
                        return false;
                    }
                    let Some(after) = search.next_non_comment(next + 1) else {
                        return false;
                    };
                    index = after;
                }
                TokenKind::Punctuation('[') => {
                    let Some(close) =
                        BalancedTokens::new(self.tokens).matching_punctuation(index, '[', ']')
                    else {
                        return false;
                    };
                    let Some(after) = search.next_non_comment(close + 1) else {
                        return false;
                    };
                    index = after;
                }
                _ => return false,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Search helper that skips comments while walking tokens.
pub struct TokenSearch<'module, 'src> {
    /// Tokens being searched.
    tokens: &'module [Token<'src>],
}

impl<'module, 'src> TokenSearch<'module, 'src> {
    /// Creates a comment-aware token search helper.
    pub(crate) const fn new(tokens: &'module [Token<'src>]) -> Self {
        Self { tokens }
    }

    /// Finds the next non-comment token at or after `start`.
    pub(crate) fn next_non_comment(self, start: usize) -> Option<usize> {
        self.tokens
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, token)| (!token.kind.is_comment()).then_some(index))
    }

    /// Finds the previous non-comment token before `before`.
    pub(crate) fn previous_non_comment(self, before: usize) -> Option<usize> {
        self.tokens
            .iter()
            .take(before)
            .enumerate()
            .rev()
            .find_map(|(index, token)| (!token.kind.is_comment()).then_some(index))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Balanced delimiter matcher over a token slice.
pub struct BalancedTokens<'module, 'src> {
    /// Tokens searched for balanced delimiters.
    tokens: &'module [Token<'src>],
}

impl<'module, 'src> BalancedTokens<'module, 'src> {
    /// Creates a balanced-token matcher.
    pub(crate) const fn new(tokens: &'module [Token<'src>]) -> Self {
        Self { tokens }
    }

    /// Finds the right parenthesis that matches `open`.
    pub(crate) fn matching_right_paren(self, open: usize) -> Option<usize> {
        self.matching(open, TokenMatcher::LeftParen, TokenMatcher::RightParen)
    }

    /// Finds the matching punctuation token for bracket-like punctuation.
    pub(crate) fn matching_punctuation(
        self,
        open: usize,
        left: char,
        right: char,
    ) -> Option<usize> {
        self.matching(
            open,
            TokenMatcher::Punctuation(left),
            TokenMatcher::Punctuation(right),
        )
    }

    /// Finds the matching right delimiter using a nesting counter.
    fn matching(self, open: usize, left: TokenMatcher, right: TokenMatcher) -> Option<usize> {
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(open) {
            if left.matches(token.kind) {
                depth += 1;
            } else if right.matches(token.kind) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Delimiter token matched by `BalancedTokens`.
enum TokenMatcher {
    /// Left parenthesis token.
    LeftParen,
    /// Right parenthesis token.
    RightParen,
    /// Specific punctuation token.
    Punctuation(char),
}

impl TokenMatcher {
    /// Returns whether `kind` matches this delimiter.
    const fn matches(self, kind: TokenKind<'_>) -> bool {
        match (self, kind) {
            (Self::LeftParen, TokenKind::LeftParen) | (Self::RightParen, TokenKind::RightParen) => {
                true
            }
            (Self::Punctuation(expected), TokenKind::Punctuation(actual)) => expected == actual,
            _ => false,
        }
    }
}

//! Lifetime-free typed token streams.

use std::slice;

use super::{IdentifierToken, Token, TokenCursor, TypedTokenFacts};
use crate::{ShaderResult, lexer::Lexer};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenStream {
    /// Lifetime-free typed tokens in source order.
    tokens: Vec<Token>,
}

impl TokenStream {
    #[must_use]
    pub const fn new(tokens: Vec<Token>) -> Self {
        Self { tokens }
    }

    /// Lexes a shader source into typed tokens with byte spans.
    ///
    /// # Errors
    ///
    /// Returns a parse error when lexing fails or an input byte range cannot be
    /// represented as a valid source span.
    pub fn lex(source: &str) -> ShaderResult<Self> {
        Lexer::tokenize(source)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Token] {
        &self.tokens
    }

    pub fn iter(&self) -> slice::Iter<'_, Token> {
        self.tokens.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    #[must_use]
    pub fn cursor(&self) -> TokenCursor<'_> {
        TokenCursor::new(self.as_slice())
    }

    /// Collects reusable facts from this typed token stream.
    #[must_use]
    pub fn facts(&self) -> TypedTokenFacts {
        TypedTokenFacts::collect(self.cursor())
    }

    #[must_use]
    pub fn into_owned(self) -> Vec<Token> {
        self.tokens
    }

    /// Finds the next token at or after `start`.
    #[must_use]
    pub fn next_non_comment(&self, start: usize) -> Option<usize> {
        self.cursor().next_non_comment(start)
    }

    /// Finds the previous token before `before`.
    #[must_use]
    pub fn previous_non_comment(&self, before: usize) -> Option<usize> {
        self.cursor().previous_non_comment(before)
    }

    /// Finds the right parenthesis that matches `open`.
    #[must_use]
    pub fn matching_right_paren(&self, open: usize) -> Option<usize> {
        self.cursor().matching_right_paren(open)
    }

    /// Finds the right brace that matches `open`.
    #[must_use]
    pub fn matching_right_brace(&self, open: usize) -> Option<usize> {
        self.cursor().matching_right_brace(open)
    }

    /// Finds the right square bracket that matches `open`.
    #[must_use]
    pub fn matching_right_square(&self, open: usize) -> Option<usize> {
        self.cursor().matching_right_square(open)
    }

    /// Iterates identifier tokens in source order.
    pub fn identifiers(&self) -> impl Iterator<Item = IdentifierToken<'_>> + '_ {
        self.cursor().identifiers()
    }
}

impl AsRef<[Token]> for TokenStream {
    fn as_ref(&self) -> &[Token] {
        self.as_slice()
    }
}

impl<'stream> IntoIterator for &'stream TokenStream {
    type Item = &'stream Token;
    type IntoIter = slice::Iter<'stream, Token>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for TokenStream {
    type Item = Token;
    type IntoIter = std::vec::IntoIter<Token>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_owned().into_iter()
    }
}

//! Parser context for one shader source.

use super::{Parser, ShaderModule, ShaderSourceText, source::SourceTextView};
use crate::{
    ShaderResult, ShaderStageKind, SourceSpan,
    lexer::{Token, TokenStream, TokenStreamExt},
};

/// Semantic parsing owner for one shader source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsingContext<'src> {
    /// Shader stage being parsed.
    stage: ShaderStageKind,
    /// Typed source view being parsed.
    source: ShaderSourceText<'src>,
    /// Lexed tokens for the source.
    tokens: TokenStream<'src>,
}

impl<'src> SourceTextView<'src> for ParsingContext<'src> {
    fn source_text(&self) -> ShaderSourceText<'src> {
        self.source
    }
}

impl<'src> ParsingContext<'src> {
    /// Builds a parser context by lexing one typed shader source.
    ///
    /// # Errors
    ///
    /// Returns a parse error when lexing fails or token spans cannot be
    /// represented as [`SourceSpan`] values.
    pub fn new(stage: ShaderStageKind, source: ShaderSourceText<'src>) -> ShaderResult<Self> {
        let tokens = TokenStream::lex(source.as_str())?;
        Ok(Self {
            stage,
            source,
            tokens,
        })
    }

    /// Creates a parser context from an already typed source view.
    ///
    /// # Errors
    ///
    /// Returns a parse error when lexing fails or token spans cannot be
    /// represented as [`SourceSpan`] values.
    pub fn from_str(stage: ShaderStageKind, source: &'src str) -> ShaderResult<Self> {
        Self::new(stage, ShaderSourceText::new(source))
    }

    /// Returns the shader stage represented by this parser context.
    #[must_use]
    pub const fn stage(&self) -> ShaderStageKind {
        self.stage
    }

    /// Returns the typed shader source view.
    #[must_use]
    pub fn source(&self) -> ShaderSourceText<'src> {
        self.source_text()
    }

    /// Borrows the source text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: SourceSpan) -> &'src str {
        self.source().slice(span)
    }

    /// Returns lexed tokens in source order.
    #[must_use]
    pub fn tokens(&self) -> &[Token<'src>] {
        &self.tokens
    }

    /// Parses this context into a lightweight syntax module.
    ///
    /// # Errors
    ///
    /// Returns a parse error when a top-level function/struct body contains
    /// unbalanced braces or parentheses.
    pub fn parse(&self) -> ShaderResult<ShaderModule<'src>> {
        let mut parser = Parser {
            context: self,
            tokens: self.tokens(),
            cursor: 0,
        };
        parser.parse_module()
    }
}

impl<'src> ShaderModule<'src> {
    /// Parses a shader source into the lightweight syntax model.
    ///
    /// # Errors
    ///
    /// Returns a parse error when lexing fails or a top-level function/struct
    /// body contains unbalanced braces or parentheses.
    pub fn parse(stage: ShaderStageKind, source: &'src str) -> ShaderResult<Self> {
        ParsingContext::from_str(stage, source)?.parse()
    }
}

//! Parsed module and top-level syntax items.

use super::{
    FunctionDecl, PreprocessorDirective, ShaderAnnotation, ShaderDeclaration, ShaderSourceText,
};
use crate::{
    ShaderResult, ShaderStageKind, SourceSpan,
    lexer::{Token, TokenStream},
};

/// Top-level shader syntax item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntaxItem<'src> {
    /// Shader interface, struct, or loose top-level declaration.
    Declaration(ShaderDeclaration<'src>),
    /// Function signature plus opaque balanced body span.
    Function(FunctionDecl<'src>),
    /// Preprocessor directive line.
    Directive(PreprocessorDirective<'src>),
    /// Wallpaper Engine metadata annotation.
    Annotation(ShaderAnnotation),
    /// Source range skipped by the lightweight parser.
    Opaque(SourceSpan),
}

/// Parsed source module for one shader stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderModule<'src> {
    /// Shader stage represented by this parsed source.
    stage: ShaderStageKind,
    /// Typed source view used by all spans in this module.
    source: ShaderSourceText<'src>,
    /// Lexed tokens retained as immutable parse output.
    tokens: TokenStream<'src>,
    /// Top-level syntax items in source order.
    items: Vec<SyntaxItem<'src>>,
}

impl<'src> ShaderModule<'src> {
    /// Creates a shader module from parsed items.
    #[must_use]
    pub fn new(
        stage: ShaderStageKind,
        source: ShaderSourceText<'src>,
        tokens: TokenStream<'src>,
        items: Vec<SyntaxItem<'src>>,
    ) -> Self {
        Self {
            stage,
            source,
            tokens,
            items,
        }
    }

    /// Returns the shader stage represented by this module.
    #[must_use]
    pub const fn stage(&self) -> ShaderStageKind {
        self.stage
    }

    /// Returns the typed shader source parsed into this module.
    #[must_use]
    pub const fn source(&self) -> ShaderSourceText<'src> {
        self.source
    }

    /// Borrows the source text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: SourceSpan) -> &'src str {
        self.source.slice(span)
    }

    /// Returns the span covering the full source text.
    ///
    /// # Errors
    ///
    /// Returns an error when the source range cannot be represented as a
    /// [`SourceSpan`].
    pub fn source_span(&self) -> ShaderResult<SourceSpan> {
        SourceSpan::new(0, self.source.as_str().len())
    }

    /// Returns top-level syntax items in source order.
    #[must_use]
    pub fn items(&self) -> &[SyntaxItem<'src>] {
        &self.items
    }

    /// Returns lexed source tokens in source order.
    #[must_use]
    pub fn tokens(&self) -> &[Token<'src>] {
        &self.tokens
    }
}

//! Function declaration syntax records.

use super::{ShaderModule, ShaderSourceText};
use crate::SourceSpan;

/// Function declaration with opaque body span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionDecl<'src> {
    /// Borrowed return type token text.
    return_type: &'src str,
    /// Borrowed function identifier text.
    name: &'src str,
    /// Span covering parameter text without surrounding parentheses.
    parameters: SourceSpan,
    /// Span from the declaration start through the closing parameter
    /// parenthesis.
    signature: SourceSpan,
    /// Span covering the balanced body including surrounding braces.
    body: SourceSpan,
    /// Span covering the full function declaration.
    span: SourceSpan,
}

impl<'src> FunctionDecl<'src> {
    /// Creates a function declaration record.
    #[must_use]
    pub fn new(
        return_type: &'src str,
        name: &'src str,
        parameters: SourceSpan,
        signature: SourceSpan,
        body: SourceSpan,
        span: SourceSpan,
    ) -> Self {
        Self {
            return_type,
            name,
            parameters,
            signature,
            body,
            span,
        }
    }

    /// Returns the function return type text.
    #[must_use]
    pub const fn return_type(&self) -> &'src str {
        self.return_type
    }

    /// Returns the function name.
    #[must_use]
    pub const fn name(&self) -> &'src str {
        self.name
    }

    /// Returns the parameter list text without surrounding parentheses.
    #[must_use]
    pub fn parameters<'source>(&self, source: &'source str) -> &'source str {
        self.parameters_from(ShaderSourceText::new(source))
    }

    /// Returns the parameter list text from a typed source view.
    #[must_use]
    pub fn parameters_from<'source>(&self, source: ShaderSourceText<'source>) -> &'source str {
        source.slice(self.parameters)
    }

    /// Returns the parameter list text from its parsed module.
    #[must_use]
    pub fn parameters_in(&self, module: &ShaderModule<'src>) -> &'src str {
        module.slice(self.parameters)
    }

    /// Returns the function signature span through the closing parenthesis.
    #[must_use]
    pub const fn signature_span(&self) -> SourceSpan {
        self.signature
    }

    /// Returns the balanced body text including surrounding braces.
    #[must_use]
    pub fn body<'source>(&self, source: &'source str) -> &'source str {
        self.body_from(ShaderSourceText::new(source))
    }

    /// Returns the balanced body text including surrounding braces from a typed
    /// source view.
    #[must_use]
    pub fn body_from<'source>(&self, source: ShaderSourceText<'source>) -> &'source str {
        source.slice(self.body)
    }

    /// Returns the balanced body text including surrounding braces from its
    /// parsed module.
    #[must_use]
    pub fn body_in(&self, module: &ShaderModule<'src>) -> &'src str {
        module.slice(self.body)
    }

    /// Returns the function body source span.
    #[must_use]
    pub const fn body_span(&self) -> SourceSpan {
        self.body
    }

    /// Returns the full function source span.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }
}

//! Function declaration syntax records.

use std::marker::PhantomData;

use smol_str::SmolStr;

use super::{ShaderModule, ShaderSourceText};
use crate::{
    SourceSpan,
    codegen::FunctionParameterQualifier,
    tokenizer::{LiteralValue, TokenCursor, TypedToken},
};

/// Function declaration with opaque body span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionDecl<'src> {
    /// Return type token text.
    return_type: SmolStr,
    /// Function identifier text.
    name: SmolStr,
    /// Span covering the function identifier token.
    name_span: SourceSpan,
    /// Span covering parameter text without surrounding parentheses.
    parameters_span: SourceSpan,
    /// Parsed parameter facts in source order.
    parameters: Vec<FunctionParameter>,
    /// Span from the declaration start through the closing parameter
    /// parenthesis.
    signature: SourceSpan,
    /// Span covering the balanced body including surrounding braces.
    body: SourceSpan,
    /// Span covering the full function declaration.
    span: SourceSpan,
    /// Ties source-slicing methods to the module source lifetime.
    source: PhantomData<&'src str>,
}

impl<'src> FunctionDecl<'src> {
    /// Creates a function declaration record.
    ///
    /// The function name span falls back to the full signature span. Use
    /// [`Self::with_spans`] when exact spans are available.
    #[must_use]
    pub fn new(
        return_type: impl Into<SmolStr>,
        name: impl Into<SmolStr>,
        parameters: SourceSpan,
        signature: SourceSpan,
        body: SourceSpan,
        span: SourceSpan,
    ) -> Self {
        Self::with_spans(
            return_type,
            name,
            FunctionDeclSpans {
                name: signature,
                parameters,
                signature,
                body,
                span,
            },
            Vec::new(),
        )
    }

    /// Creates a function declaration record with an exact identifier span.
    #[must_use]
    pub fn with_spans(
        return_type: impl Into<SmolStr>,
        name: impl Into<SmolStr>,
        spans: FunctionDeclSpans,
        parameters: Vec<FunctionParameter>,
    ) -> Self {
        Self {
            return_type: return_type.into(),
            name: name.into(),
            name_span: spans.name,
            parameters_span: spans.parameters,
            parameters,
            signature: spans.signature,
            body: spans.body,
            span: spans.span,
            source: PhantomData,
        }
    }

    /// Returns the function return type text.
    #[must_use]
    pub fn return_type(&self) -> &str {
        self.return_type.as_str()
    }

    /// Returns the function name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the function name source span.
    #[must_use]
    pub const fn name_span(&self) -> SourceSpan {
        self.name_span
    }

    /// Returns the parameter list text without surrounding parentheses.
    #[must_use]
    pub fn parameter_source<'source>(&self, source: &'source str) -> &'source str {
        self.parameter_source_from(ShaderSourceText::new(source))
    }

    /// Returns the parameter list text from a typed source view.
    #[must_use]
    pub fn parameter_source_from<'source>(
        &self,
        source: ShaderSourceText<'source>,
    ) -> &'source str {
        source.slice(self.parameters_span)
    }

    /// Returns the parameter list text from its parsed module.
    #[must_use]
    pub fn parameters_in(&self, module: &ShaderModule<'src>) -> &'src str {
        self.parameter_source_in(module)
    }

    /// Returns the parameter list text from its parsed module.
    #[must_use]
    pub fn parameter_source_in(&self, module: &ShaderModule<'src>) -> &'src str {
        module.slice(self.parameters_span)
    }

    /// Returns parsed function parameter facts.
    #[must_use]
    pub fn parameters(&self) -> &[FunctionParameter] {
        &self.parameters
    }

    /// Returns the source span between the parameter parentheses.
    #[must_use]
    pub const fn parameters_span(&self) -> SourceSpan {
        self.parameters_span
    }

    /// Returns the function signature span through the closing parenthesis.
    #[must_use]
    pub const fn signature_span(&self) -> SourceSpan {
        self.signature
    }

    /// Returns top-level parameter type tokens from this function's signature.
    pub fn parameter_types(&self) -> impl Iterator<Item = &str> {
        self.parameters
            .iter()
            .map(|parameter| parameter.ty().as_str())
    }

    /// Parses function parameter facts from explicit parenthesis bounds.
    #[must_use]
    #[allow(clippy::single_call_fn)]
    pub fn parse_parameters(
        tokens: TokenCursor<'_>,
        open: usize,
        close: usize,
    ) -> Vec<FunctionParameter> {
        let mut items = Vec::new();
        let mut start = open + 1;
        while start < close {
            let end = tokens.top_level_comma_segment_end(start, close);
            let parameter = tokens
                .non_comment_range(start, end)
                .and_then(|(start, end)| {
                    if start == end
                        && tokens[start]
                            .kind()
                            .is_keyword(crate::tokenizer::KeywordType::Void)
                    {
                        return None;
                    }
                    let span =
                        SourceSpan::new(tokens[start].span().start(), tokens[end].span().end())
                            .ok()?;
                    let (ty_index, ty) = (start..=end).find_map(|index| {
                        let kind = tokens[index].kind();
                        let text = kind.source_text()?;
                        (!FunctionParameterQualifier::is_token(kind))
                            .then(|| (index, SmolStr::new(text)))
                    })?;
                    let mut bracket_depth = 0usize;
                    let name_index =
                        (ty_index + 1..=end).find(|index| match tokens[*index].kind() {
                            kind if kind.is_left_square() => {
                                bracket_depth += 1;
                                false
                            }
                            kind if kind.is_right_square() => {
                                bracket_depth = bracket_depth.saturating_sub(1);
                                false
                            }
                            TypedToken::Identifier(_) => bracket_depth == 0,
                            _ => false,
                        });
                    let name = name_index.and_then(|name_index| match tokens[name_index].kind() {
                        TypedToken::Identifier(name) => Some(name.clone()),
                        _ => None,
                    });
                    let array_name =
                        name_index
                            .filter(|_name_index| name.is_some())
                            .and_then(|name_index| {
                                let open = tokens.next_non_comment(name_index + 1)?;
                                let size = tokens.next_non_comment(open + 1)?;
                                let close = tokens.next_non_comment(size + 1)?;
                                if !(start..=end).contains(&close)
                                    || open > end
                                    || !tokens[open].kind().is_left_square()
                                    || !matches!(
                                        tokens[size].kind(),
                                        TypedToken::Literal(LiteralValue::Number(_))
                                            | TypedToken::Identifier(_)
                                    )
                                    || !tokens[close].kind().is_right_square()
                                {
                                    return None;
                                }
                                name.clone()
                            });
                    Some(FunctionParameter {
                        ty,
                        name,
                        span,
                        array_name,
                    })
                });
            if let Some(parameter) = parameter {
                items.push(parameter);
            }
            start = end.saturating_add(1);
        }
        items
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

/// Source spans that define a parsed function declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FunctionDeclSpans {
    /// Function identifier span.
    pub name: SourceSpan,
    /// Parameter list span without parentheses.
    pub parameters: SourceSpan,
    /// Signature span through the closing parenthesis.
    pub signature: SourceSpan,
    /// Body span including braces.
    pub body: SourceSpan,
    /// Full function span.
    pub span: SourceSpan,
}

/// Parsed function parameter fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionParameter {
    /// Parameter type spelling without qualifiers.
    ty: SmolStr,
    /// Parameter name, when the declaration includes one.
    name: Option<SmolStr>,
    /// Span covering this parameter segment.
    span: SourceSpan,
    /// Fixed array parameter name when this parameter is `name[size]`.
    array_name: Option<SmolStr>,
}

impl FunctionParameter {
    /// Returns the parameter type.
    #[must_use]
    pub fn ty(&self) -> &SmolStr {
        &self.ty
    }

    /// Returns the parameter name.
    #[must_use]
    pub fn name(&self) -> Option<&SmolStr> {
        self.name.as_ref()
    }

    /// Returns the parameter name text.
    #[must_use]
    pub fn name_text(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the source span covering this parameter.
    #[must_use]
    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    /// Returns the fixed array parameter name.
    #[must_use]
    pub fn array_name(&self) -> Option<&SmolStr> {
        self.array_name.as_ref()
    }
}

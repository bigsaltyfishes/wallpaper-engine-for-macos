use super::{
    BalancedTokens, FunctionParameterQualifier, FunctionSpecialization, ShaderDiagnostic,
    ShaderError, ShaderModule, ShaderResult, SourceSpan, SyntaxItem, Token, TokenKind,
    specialization::SegmentSpan,
};

/// Same-name functions that also need array-parameter specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionOverloads<'src> {
    /// Specialized signatures for all overloads with this name.
    pub(super) signatures: Vec<FunctionSpecializationSignature<'src>>,
}
impl<'src> TryFrom<FunctionOverloadSource<'_, 'src>> for FunctionOverloads<'src> {
    type Error = ShaderError;

    /// Collects array-parameter overloads sharing the target function name.
    fn try_from(source: FunctionOverloadSource<'_, 'src>) -> ShaderResult<Self> {
        let module = source.module;
        let mut signatures = Vec::new();
        for function in module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) if function.name() == source.function.name() => {
                Some(function)
            }
            _ => None,
        }) {
            let Some(parameters) =
                Option::<FunctionParameters<'_>>::try_from(FunctionParametersSource {
                    tokens: module.tokens(),
                    function,
                })?
            else {
                continue;
            };
            signatures.push(FunctionSpecializationSignature::from(&parameters));
        }
        Ok(Self { signatures })
    }
}
impl FunctionOverloads<'_> {
    /// Returns a controlled error if specialization would create duplicate
    /// same-name signatures.
    pub(super) fn ensure_unambiguous(
        &self,
        parameters: &ArrayFunctionParameters<'_>,
        specialization: &FunctionSpecialization<'_>,
    ) -> ShaderResult<()> {
        let specialized_signature = specialization.retained_parameter_types();
        let collisions = self
            .signatures
            .iter()
            .filter(|signature| signature.retained_parameter_types() == specialized_signature)
            .count();
        let same_call_shape = self
            .signatures
            .iter()
            .filter(|signature| signature.has_array_parameters())
            .filter(|signature| signature.call_shape() == parameters.call_shape())
            .count();
        if collisions <= 1 && same_call_shape <= 1 {
            return Ok(());
        }

        Err(ShaderError::Legalize {
            diagnostics: Box::new([ShaderDiagnostic::new(
                "array-parameter specialization is ambiguous for overloaded function",
            )
            .with_pass("Legalizer")]),
        })
    }
}
/// One same-name overload's signature after possible array specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionSpecializationSignature<'src> {
    /// Whether this overload itself contains fixed-array parameters.
    pub(super) has_array_parameters: bool,
    /// Parameter type signature that would remain after specialization.
    pub(super) retained_parameter_types: Vec<Option<&'src str>>,
    /// Coarse syntactic argument shape accepted by the overload.
    pub(super) call_shape: Vec<ParameterCallShape>,
}
impl<'src> FunctionSpecializationSignature<'src> {
    /// Returns whether this overload itself contains fixed-array parameters.
    pub(super) const fn has_array_parameters(&self) -> bool {
        self.has_array_parameters
    }

    /// Returns the parameter type signature that remains after specialization.
    pub(super) fn retained_parameter_types(&self) -> &[Option<&'src str>] {
        &self.retained_parameter_types
    }

    /// Returns the coarse call-site shape.
    pub(super) fn call_shape(&self) -> &[ParameterCallShape] {
        &self.call_shape
    }
}
impl<'src> From<&FunctionParameters<'src>> for FunctionSpecializationSignature<'src> {
    fn from(parameters: &FunctionParameters<'src>) -> Self {
        Self {
            has_array_parameters: parameters.has_array_parameters(),
            retained_parameter_types: parameters.retained_parameter_types(),
            call_shape: parameters.call_shape(),
        }
    }
}
/// Source data needed to inspect overloads for one function.
#[derive(Clone, Copy)]
pub(super) struct FunctionOverloadSource<'module, 'src> {
    /// Parsed module containing the overload set.
    pub(super) module: &'module ShaderModule<'src>,
    /// Array-parameter function being specialized.
    pub(super) function: &'module crate::syntax::FunctionDecl<'src>,
}
/// Parsed parameters for one function declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionParameters<'src> {
    /// Full source span between the function call parentheses.
    pub(super) span: SourceSpan,
    /// Parameter declarations in source order.
    pub(super) items: Vec<FunctionParameter<'src>>,
}
impl<'src> FunctionParameters<'src> {
    /// Returns whether any parameter is a fixed-size array.
    pub(super) fn has_array_parameters(&self) -> bool {
        self.items.iter().any(|parameter| parameter.array.is_some())
    }

    /// Returns the parameter type signature left after specialization.
    pub(super) fn retained_parameter_types(&self) -> Vec<Option<&'src str>> {
        self.items
            .iter()
            .filter(|parameter| parameter.array.is_none())
            .map(|parameter| parameter.ty)
            .collect()
    }

    /// Returns the syntactic signature shape available at call sites.
    pub(super) fn call_shape(&self) -> Vec<ParameterCallShape> {
        self.items
            .iter()
            .map(|parameter| {
                if parameter.array.is_some() {
                    ParameterCallShape::TopLevelArrayIdentifier
                } else {
                    ParameterCallShape::AnyExpression
                }
            })
            .collect()
    }
}
/// Parsed parameters for one function declaration that has fixed arrays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ArrayFunctionParameters<'src> {
    /// Full source span between the function call parentheses.
    pub(super) span: SourceSpan,
    /// Parameter declarations in source order.
    pub(super) items: Vec<FunctionParameter<'src>>,
}
impl<'src> ArrayFunctionParameters<'src> {
    /// Returns the parameter type signature left after specialization.
    pub(super) fn retained_parameter_types(&self) -> Vec<Option<&'src str>> {
        self.items
            .iter()
            .filter(|parameter| parameter.array.is_none())
            .map(|parameter| parameter.ty)
            .collect()
    }

    /// Returns the syntactic signature shape available at call sites.
    pub(super) fn call_shape(&self) -> Vec<ParameterCallShape> {
        self.items
            .iter()
            .map(|parameter| {
                if parameter.array.is_some() {
                    ParameterCallShape::TopLevelArrayIdentifier
                } else {
                    ParameterCallShape::AnyExpression
                }
            })
            .collect()
    }
}
impl<'src> From<FunctionParameters<'src>> for ArrayFunctionParameters<'src> {
    fn from(parameters: FunctionParameters<'src>) -> Self {
        Self {
            span: parameters.span,
            items: parameters.items,
        }
    }
}
/// Coarse call-site shape used when expression typing is unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParameterCallShape {
    /// A retained scalar/vector/etc expression.
    AnyExpression,
    /// A removed fixed-array parameter that must be a top-level array name.
    TopLevelArrayIdentifier,
}
/// Source data needed to parse one function parameter list.
#[derive(Clone, Copy)]
pub(super) struct FunctionParametersSource<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Parsed function declaration.
    pub(super) function: &'tokens crate::syntax::FunctionDecl<'src>,
}
impl<'src> TryFrom<FunctionParametersSource<'_, 'src>> for Option<FunctionParameters<'src>> {
    type Error = crate::ShaderError;

    fn try_from(source: FunctionParametersSource<'_, 'src>) -> ShaderResult<Self> {
        let tokens = source.tokens;
        let Some(header) = Option::<FunctionHeader>::from(FunctionHeaderSource {
            tokens,
            signature: source.function.signature_span(),
        }) else {
            return Ok(None);
        };
        let span = SourceSpan::new(
            tokens[header.open].span.end(),
            tokens[header.close].span.start(),
        )?;
        let mut items = Vec::new();
        let mut start = header.open + 1;
        while start < header.close {
            let end = ParameterEnd {
                tokens,
                start,
                close: header.close,
            }
            .end();
            let parameter = FunctionParameter::from(&tokens[start..end]);
            items.push(parameter);
            start = end.saturating_add(1);
        }

        Ok(Some(FunctionParameters { span, items }))
    }
}
/// Source data needed to parse one array-parameter function parameter list.
#[derive(Clone, Copy)]
pub(super) struct ArrayFunctionParametersSource<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Parsed function declaration.
    pub(super) function: &'tokens crate::syntax::FunctionDecl<'src>,
}
impl<'src> TryFrom<ArrayFunctionParametersSource<'_, 'src>>
    for Option<ArrayFunctionParameters<'src>>
{
    type Error = crate::ShaderError;

    fn try_from(source: ArrayFunctionParametersSource<'_, 'src>) -> ShaderResult<Self> {
        let Some(parameters) =
            Option::<FunctionParameters<'_>>::try_from(FunctionParametersSource {
                tokens: source.tokens,
                function: source.function,
            })?
        else {
            return Ok(None);
        };

        Ok(parameters
            .has_array_parameters()
            .then(|| ArrayFunctionParameters::from(parameters)))
    }
}
/// One comma-delimited function parameter segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionParameter<'src> {
    /// Original source span for this parameter segment.
    pub(super) span: SourceSpan,
    /// Parameter type spelling, excluding storage/precision qualifiers.
    pub(super) ty: Option<&'src str>,
    /// Fixed-array details, when this segment is a fixed-array parameter.
    pub(super) array: Option<ArrayFunctionParameter<'src>>,
}
impl<'src> From<&[Token<'src>]> for FunctionParameter<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let span = SegmentSpan::from(tokens).span();
        let ty = ParameterType::from(tokens).ty();
        let meaningful = tokens
            .iter()
            .filter(|token| !token.kind.is_comment())
            .collect::<Vec<_>>();
        let array = match meaningful.as_slice() {
            [ty, name, open, size, close]
                if matches!(ty.kind, TokenKind::Identifier(_))
                    && matches!(name.kind, TokenKind::Identifier(_))
                    && matches!(open.kind, TokenKind::Punctuation('['))
                    && matches!(size.kind, TokenKind::Number(_))
                    && matches!(close.kind, TokenKind::Punctuation(']')) =>
            {
                let TokenKind::Identifier(name) = name.kind else {
                    unreachable!("array parameter name guard ensures identifier")
                };
                Some(ArrayFunctionParameter { name })
            }
            _ => None,
        };
        Self { span, ty, array }
    }
}
/// Type token parsed from one function parameter segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ParameterType<'src> {
    /// Parsed type spelling, if present.
    pub(super) ty: Option<&'src str>,
}
impl<'src> ParameterType<'src> {
    /// Returns the parsed type spelling.
    pub(super) const fn ty(self) -> Option<&'src str> {
        self.ty
    }
}
impl<'src> From<&[Token<'src>]> for ParameterType<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        for token in tokens.iter().filter(|token| !token.kind.is_comment()) {
            let TokenKind::Identifier(text) = token.kind else {
                continue;
            };
            if FunctionParameterQualifier::from(text).is_qualifier() {
                continue;
            }
            return Self { ty: Some(text) };
        }

        Self { ty: None }
    }
}
/// One fixed-array function parameter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ArrayFunctionParameter<'src> {
    /// Parameter identifier used inside the function body.
    pub(super) name: &'src str,
}
/// Open and close token indices for a function declaration header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FunctionHeader {
    /// Opening parenthesis token index.
    pub(super) open: usize,
    /// Closing parenthesis token index.
    pub(super) close: usize,
}
/// Source data needed to find a function header.
#[derive(Clone, Copy)]
pub(super) struct FunctionHeaderSource<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Parsed function signature span.
    pub(super) signature: SourceSpan,
}
impl From<FunctionHeaderSource<'_, '_>> for Option<FunctionHeader> {
    fn from(source: FunctionHeaderSource<'_, '_>) -> Self {
        let tokens = source.tokens;
        let signature = source.signature;
        let open = tokens.iter().position(|token| {
            token.span.start() >= signature.start() && matches!(token.kind, TokenKind::LeftParen)
        })?;
        let close = BalancedTokens::new(tokens).matching_right_paren(open)?;
        (tokens[close].span.end() <= signature.end()).then_some(FunctionHeader { open, close })
    }
}
/// End finder for one comma-delimited parameter segment.
#[derive(Clone, Copy)]
pub(super) struct ParameterEnd<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Segment start.
    pub(super) start: usize,
    /// Closing parenthesis index.
    pub(super) close: usize,
}
impl ParameterEnd<'_, '_> {
    /// Returns the exclusive end token index.
    pub(super) fn end(self) -> usize {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        for index in self.start..self.close {
            match self.tokens[index].kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::Comma if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return index;
                }
                _ => {}
            }
        }
        self.close
    }
}
/// Exact single-identifier argument parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SingleIdentifier<'src> {
    /// Parsed identifier, if present.
    pub(super) identifier: Option<&'src str>,
}
impl<'src> SingleIdentifier<'src> {
    /// Returns the parsed identifier.
    pub(super) const fn identifier(self) -> Option<&'src str> {
        self.identifier
    }
}
impl<'src> From<&[Token<'src>]> for SingleIdentifier<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let mut meaningful = tokens.iter().filter(|token| !token.kind.is_comment());
        let Some(token) = meaningful.next() else {
            return Self { identifier: None };
        };
        if meaningful.next().is_some() {
            return Self { identifier: None };
        }
        Self {
            identifier: token.kind.identifier_text(),
        }
    }
}

use super::{
    BalancedTokens, ShaderModule, ShaderResult, SourceSpan, SyntaxItem, Token, TokenKind,
    TokenSearch,
    calls::{CallArguments, FunctionCallArguments, SpecializedCall},
    signatures::{ArrayFunctionParameter, ArrayFunctionParameters},
};

/// Safe specialization plan for one function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionSpecialization<'src> {
    /// Replacement function parameter list.
    pub(super) parameters: String,
    /// Parameter type signature left after specialization.
    pub(super) retained_parameter_types: Vec<Option<&'src str>>,
    /// Fixed-array parameters paired with stable global array arguments.
    pub(super) array_parameters: Vec<(ArrayFunctionParameter<'src>, &'src str)>,
    /// Replacement argument lists for call sites.
    pub(super) calls: Vec<SpecializedCall>,
}
impl<'src> FunctionSpecialization<'src> {
    /// Returns the parameter type signature left after specialization.
    pub(super) fn retained_parameter_types(&self) -> &[Option<&'src str>] {
        &self.retained_parameter_types
    }
}
/// Source data needed to build one specialization plan.
#[derive(Clone, Copy)]
pub(super) struct SpecializationSource<'module, 'src> {
    /// Parsed module containing the function and calls.
    pub(super) module: &'module ShaderModule<'src>,
    /// Parsed function parameters.
    pub(super) parameters: &'module ArrayFunctionParameters<'src>,
    /// Parsed call arguments.
    pub(super) arguments: &'module CallArguments<'src>,
}
impl<'src> TryFrom<SpecializationSource<'_, 'src>> for Option<FunctionSpecialization<'src>> {
    type Error = crate::ShaderError;

    fn try_from(source: SpecializationSource<'_, 'src>) -> ShaderResult<Self> {
        let parameters = source.parameters;
        let calls = &source.arguments.calls;
        if calls
            .iter()
            .any(|call| call.items.len() != parameters.items.len())
        {
            return Ok(None);
        }

        let mut array_parameters = Vec::new();
        for (index, parameter) in parameters.items.iter().enumerate() {
            let Some(array_parameter) = parameter.array.clone() else {
                continue;
            };
            let Some(identifier) = StableTopLevelArrayArgument {
                module: source.module,
                calls,
                index,
            }
            .identifier() else {
                return Ok(None);
            };
            array_parameters.push((array_parameter, identifier));
        }

        if array_parameters.is_empty() {
            return Ok(None);
        }

        let retained_parameters = parameters
            .items
            .iter()
            .filter(|parameter| parameter.array.is_none())
            .map(|parameter| source.module.slice(parameter.span))
            .collect::<Vec<_>>();
        let retained_parameter_types = parameters.retained_parameter_types();
        let parameters = retained_parameters.join(", ");
        let calls = calls
            .iter()
            .map(|call| {
                let retained_arguments = call
                    .items
                    .iter()
                    .zip(source.parameters.items.iter())
                    .filter(|(_argument, parameter)| parameter.array.is_none())
                    .map(|(argument, _parameter)| source.module.slice(argument.span))
                    .collect::<Vec<_>>();
                SpecializedCall {
                    span: call.span,
                    arguments: retained_arguments.join(", "),
                }
            })
            .collect();

        Ok(Some(FunctionSpecialization {
            parameters,
            retained_parameter_types,
            array_parameters,
            calls,
        }))
    }
}
/// Stable argument identifier for one specialized array-parameter position.
#[derive(Clone, Copy)]
pub(super) struct StableTopLevelArrayArgument<'calls, 'module, 'src> {
    /// Parsed module containing top-level declarations.
    pub(super) module: &'module ShaderModule<'src>,
    /// Calls being checked.
    pub(super) calls: &'calls [FunctionCallArguments<'src>],
    /// Argument position being specialized.
    pub(super) index: usize,
}
impl<'src> StableTopLevelArrayArgument<'_, '_, 'src> {
    /// Returns the shared top-level array identifier, if every call is safe.
    pub(super) fn identifier(self) -> Option<&'src str> {
        let mut stable = None;
        for call in self.calls {
            let identifier = call.items.get(self.index)?.identifier?;
            if !bool::from(TopLevelArrayDeclaration {
                module: self.module,
                name: identifier,
            }) {
                return None;
            }
            if stable.is_some_and(|existing| existing != identifier) {
                return None;
            }
            stable = Some(identifier);
        }
        stable
    }
}
/// Top-level array declaration lookup.
#[derive(Clone, Copy)]
pub(super) struct TopLevelArrayDeclaration<'module, 'src> {
    /// Parsed module being searched.
    pub(super) module: &'module ShaderModule<'src>,
    /// Declaration name to find.
    pub(super) name: &'src str,
}
impl From<TopLevelArrayDeclaration<'_, '_>> for bool {
    fn from(source: TopLevelArrayDeclaration<'_, '_>) -> Self {
        let module = source.module;
        let name = source.name;
        module.items().iter().any(|item| {
            let SyntaxItem::Declaration(declaration) = item else {
                return false;
            };
            declaration.name() == Some(name)
                && ArrayDeclarationSuffix {
                    tokens: module.tokens(),
                    span: declaration.span(),
                    name,
                }
                .exists()
        })
    }
}
/// Checks whether a declaration name is followed by an array suffix.
#[derive(Clone, Copy)]
pub(super) struct ArrayDeclarationSuffix<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Declaration source span.
    pub(super) span: SourceSpan,
    /// Declaration name.
    pub(super) name: &'src str,
}
impl ArrayDeclarationSuffix<'_, '_> {
    /// Returns whether the declaration has a syntactic `[N]` suffix.
    pub(super) fn exists(self) -> bool {
        let Some(name_index) = self.tokens.iter().position(|token| {
            token.span.start() >= self.span.start()
                && token.span.end() <= self.span.end()
                && matches!(token.kind, TokenKind::Identifier(text) if text == self.name)
        }) else {
            return false;
        };
        let Some(open) = TokenSearch::new(self.tokens).next_non_comment(name_index + 1) else {
            return false;
        };
        if !matches!(self.tokens[open].kind, TokenKind::Punctuation('[')) {
            return false;
        }
        let Some(close) = BalancedTokens::new(self.tokens).matching_punctuation(open, '[', ']')
        else {
            return false;
        };
        self.tokens[close].span.end() <= self.span.end()
    }
}
/// Source span for a non-empty comma-delimited token segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SegmentSpan {
    /// Segment span.
    pub(super) span: SourceSpan,
}
impl SegmentSpan {
    /// Returns the segment span.
    pub(super) const fn span(self) -> SourceSpan {
        self.span
    }
}
impl From<&[Token<'_>]> for SegmentSpan {
    fn from(tokens: &[Token<'_>]) -> Self {
        let start = tokens.first().map_or(0, |token| token.span.start());
        let end = tokens.last().map_or(start, |token| token.span.end());
        Self {
            span: SourceSpan::new(start, end)
                .expect("token order should produce a valid source span"),
        }
    }
}
/// Token-index range for a source span.
#[derive(Clone, Copy)]
pub(super) struct TokenSpanRange {
    /// First token in the source span.
    pub(super) start: usize,
    /// First token outside the source span.
    pub(super) end: usize,
}
/// Source span bounds used to find a token-index range.
pub(super) struct TokenSpanRangeSource<'tokens, 'src> {
    /// Tokens being searched.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Inclusive source start.
    pub(super) start: usize,
    /// Exclusive source end.
    pub(super) end: usize,
}
impl TokenSpanRangeSource<'_, '_> {
    /// Returns the token indices contained by the source span.
    pub(super) fn range(self) -> Option<TokenSpanRange> {
        let start = self
            .tokens
            .iter()
            .position(|token| token.span.start() >= self.start)?;
        let end = self.tokens[start..]
            .iter()
            .position(|token| token.span.end() > self.end)
            .map_or(self.tokens.len(), |index| start + index);
        (start < end).then_some(TokenSpanRange { start, end })
    }
}

use super::{
    ArrayFunctionParameters, BalancedTokens, FunctionCallIndex, ParameterEnd, SegmentSpan,
    ShaderModule, ShaderResult, SingleIdentifier, SourceSpan, Token, TokenKind, TokenSearch,
    TopLevelArrayDeclaration,
};

/// Parsed arguments passed to all calls of one function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CallArguments<'src> {
    /// Function calls in source order.
    pub(super) calls: Vec<FunctionCallArguments<'src>>,
}
/// Source data needed to collect function call arguments.
#[derive(Clone, Copy)]
pub(super) struct CallArgumentsSource<'tokens, 'src> {
    /// Parsed module containing the function calls.
    pub(super) module: &'tokens ShaderModule<'src>,
    /// Function name being specialized.
    pub(super) name: &'src str,
    /// Array-helper parameter signature being matched.
    pub(super) parameters: &'tokens ArrayFunctionParameters<'src>,
}
impl<'src> TryFrom<CallArgumentsSource<'_, 'src>> for Option<CallArguments<'src>> {
    type Error = crate::ShaderError;

    fn try_from(source: CallArgumentsSource<'_, 'src>) -> ShaderResult<Self> {
        let module = source.module;
        let tokens = module.tokens();
        let mut calls = Vec::new();
        for call in FunctionCallIndex::new(tokens)
            .iter()
            .filter(|call| call.name() == source.name)
        {
            if bool::from(FunctionDefinitionCallSource {
                tokens,
                name_index: call.name_index,
            }) {
                continue;
            }
            let Ok(arguments) = FunctionCallArguments::try_from(CallArgumentListSource {
                tokens,
                open: call.open_index,
                close: call.close_index,
            }) else {
                return Ok(None);
            };
            if !arguments.matches_array_helper_signature(module, source.parameters) {
                continue;
            }
            calls.push(arguments);
        }

        if calls.is_empty() {
            return Ok(None);
        }
        Ok(Some(CallArguments { calls }))
    }
}
/// Source data needed to parse one call argument list.
#[derive(Clone, Copy)]
pub(super) struct CallArgumentListSource<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Call opening parenthesis token index.
    pub(super) open: usize,
    /// Call closing parenthesis token index.
    pub(super) close: usize,
}
impl<'src> TryFrom<CallArgumentListSource<'_, 'src>> for FunctionCallArguments<'src> {
    type Error = ();

    fn try_from(source: CallArgumentListSource<'_, 'src>) -> Result<Self, Self::Error> {
        let mut arguments = Vec::new();
        let mut start = source.open + 1;
        while start < source.close {
            let end = ParameterEnd {
                tokens: source.tokens,
                start,
                close: source.close,
            }
            .end();
            arguments.push(FunctionCallArgument::from(&source.tokens[start..end]));
            start = end.saturating_add(1);
        }
        let span = SourceSpan::new(
            source.tokens[source.open].span.end(),
            source.tokens[source.close].span.start(),
        )
        .map_err(|_error| ())?;
        (!arguments.is_empty())
            .then_some(FunctionCallArguments {
                span,
                items: arguments,
            })
            .ok_or(())
    }
}
/// Function-call classifier used to skip declaration headers.
#[derive(Clone, Copy)]
pub(super) struct FunctionDefinitionCallSource<'tokens, 'src> {
    /// Tokens from the owning module.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Function name token index.
    pub(super) name_index: usize,
}
impl From<FunctionDefinitionCallSource<'_, '_>> for bool {
    fn from(source: FunctionDefinitionCallSource<'_, '_>) -> Self {
        let tokens = source.tokens;
        let search = TokenSearch::new(tokens);
        let Some(open) = search.next_non_comment(source.name_index + 1) else {
            return false;
        };
        let Some(close) = BalancedTokens::new(tokens).matching_right_paren(open) else {
            return false;
        };
        let Some(next) = search.next_non_comment(close + 1) else {
            return false;
        };
        matches!(tokens[next].kind, TokenKind::LeftBrace)
    }
}
/// One parsed function call argument list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionCallArguments<'src> {
    /// Full source span between the call parentheses.
    pub(super) span: SourceSpan,
    /// Argument segments in source order.
    pub(super) items: Vec<FunctionCallArgument<'src>>,
}
impl<'src> FunctionCallArguments<'src> {
    /// Returns whether this call can safely target the array-helper signature.
    pub(super) fn matches_array_helper_signature(
        &self,
        module: &ShaderModule<'src>,
        parameters: &ArrayFunctionParameters<'src>,
    ) -> bool {
        self.items.len() == parameters.items.len()
            && self
                .items
                .iter()
                .zip(parameters.items.iter())
                .all(|(argument, parameter)| {
                    parameter.array.is_none()
                        || argument.identifier.is_some_and(|name| {
                            bool::from(TopLevelArrayDeclaration { module, name })
                        })
                })
    }
}
/// One comma-delimited function call argument segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FunctionCallArgument<'src> {
    /// Original source span for this argument segment.
    pub(super) span: SourceSpan,
    /// Identifier text when the argument is exactly one identifier token.
    pub(super) identifier: Option<&'src str>,
}
impl<'src> From<&[Token<'src>]> for FunctionCallArgument<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let span = SegmentSpan::from(tokens).span();
        let identifier = SingleIdentifier::from(tokens).identifier();
        Self { span, identifier }
    }
}
/// One call-site replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SpecializedCall {
    /// Span between the call parentheses.
    pub(super) span: SourceSpan,
    /// Replacement argument list.
    pub(super) arguments: String,
}
/// Candidate identifier that may be part of a member access.
pub(super) struct MemberFieldIdentifier<'tokens, 'src> {
    /// Tokens being searched.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Identifier token index.
    pub(super) index: usize,
}
impl From<MemberFieldIdentifier<'_, '_>> for bool {
    fn from(source: MemberFieldIdentifier<'_, '_>) -> Self {
        let Some(previous) = TokenSearch::new(source.tokens).previous_non_comment(source.index)
        else {
            return false;
        };
        matches!(source.tokens[previous].kind, TokenKind::Punctuation('.'))
    }
}

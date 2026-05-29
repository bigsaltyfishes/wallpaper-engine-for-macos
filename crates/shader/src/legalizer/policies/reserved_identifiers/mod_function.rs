use super::{
    FunctionCall, FunctionDecl, FunctionParameterQualifier, ScalarExpression, ScalarTypeFacts,
    ShaderModule, SourceSpan, SyntaxItem, Token, TokenKind,
};

/// User `mod` collision plus scalar facts for call classification.
pub(super) struct ClassifiedModCollision<'src> {
    /// Parsed collision class.
    pub(super) collision: ModCollision,
    /// Known scalar variable declarations.
    pub(super) scalar_facts: ScalarTypeFacts<'src>,
}
/// Parsed source classes that need user `mod` collision rewrites.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ModCollision {
    /// Source declares `mod` with one argument.
    pub(super) has_unary: bool,
    /// Source declares `float mod(float, float)`.
    pub(super) has_scalar_binary: bool,
    /// Declaration name spans included in the collision class.
    pub(super) name_spans: Vec<SourceSpan>,
}
/// Source facts needed to classify user-defined `mod` declarations.
pub(super) struct ModCollisionClass<'module, 'src> {
    /// Parsed shader module.
    pub(super) module: &'module ShaderModule<'src>,
    /// Fallback declaration entries from the declaration plan.
    pub(super) fallback_functions: Vec<crate::legalizer::FunctionEntry<'src>>,
}
impl ModCollisionClass<'_, '_> {
    /// Classifies parsed `mod` declarations using syntax tokens.
    pub(super) fn classify(self) -> ModCollision {
        let mut collision = ModCollision::default();
        for function in self.module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) if function.name() == "mod" => Some(function),
            _ => None,
        }) {
            let declaration = ModDeclaration {
                function,
                module: self.module,
            };
            if declaration.argument_count() == 1 {
                collision.has_unary = true;
                collision.name_spans.push(declaration.name_span());
            } else if declaration.is_scalar_binary() {
                collision.has_scalar_binary = true;
                collision.name_spans.push(declaration.name_span());
            }
        }

        if collision.name_spans.is_empty() {
            collision.name_spans = self
                .fallback_functions
                .into_iter()
                .map(|function| function.name_span)
                .collect::<Vec<_>>();
            collision.has_unary = !collision.name_spans.is_empty();
        }

        collision
    }
}
/// Parsed function declaration being tested as a user `mod` overload.
#[derive(Clone, Copy)]
pub(super) struct ModDeclaration<'module, 'src> {
    /// Parsed function declaration.
    pub(super) function: &'module FunctionDecl<'src>,
    /// Parsed shader module.
    pub(super) module: &'module ShaderModule<'src>,
}
impl<'module, 'src> ModDeclaration<'module, 'src> {
    /// Returns the span of the function name token.
    pub(super) fn name_span(self) -> SourceSpan {
        self.module
            .tokens()
            .iter()
            .find(|token| {
                token.span.start() >= self.function.signature_span().start()
                    && token.span.end() <= self.function.signature_span().end()
                    && matches!(token.kind, TokenKind::Identifier(text) if text == self.function.name())
            })
            .map_or(self.function.signature_span(), |token| token.span)
    }

    /// Returns whether this declaration is `float mod(float, float)`.
    pub(super) fn is_scalar_binary(self) -> bool {
        self.function.return_type() == "float"
            && self.argument_count() == 2
            && self.parameter_types().as_slice() == ["float", "float"]
    }

    /// Counts top-level function declaration parameters.
    pub(super) fn argument_count(self) -> usize {
        self.parameter_types().len()
    }

    /// Extracts top-level parameter type tokens.
    pub(super) fn parameter_types(self) -> Vec<&'src str> {
        let Some(parameters) = self.parameter_tokens() else {
            return Vec::new();
        };
        ParameterTypes { tokens: parameters }.collect()
    }

    /// Returns tokens inside the declaration's parameter list.
    pub(super) fn parameter_tokens(self) -> Option<&'module [Token<'src>]> {
        let signature = TokenRange::new(self.module.tokens(), self.function.signature_span());
        let open = signature
            .tokens()
            .iter()
            .position(|token| matches!(token.kind, TokenKind::LeftParen))?;
        let close = signature
            .tokens()
            .iter()
            .rposition(|token| matches!(token.kind, TokenKind::RightParen))?;
        (open < close).then_some(&signature.tokens()[open + 1..close])
    }
}
/// Parameter token stream.
pub(super) struct ParameterTypes<'module, 'src> {
    /// Tokens inside the parameter list.
    pub(super) tokens: &'module [Token<'src>],
}
impl<'src> ParameterTypes<'_, 'src> {
    /// Collects top-level parameter type spellings.
    pub(super) fn collect(self) -> Vec<&'src str> {
        let mut depth = 0usize;
        let mut current_type = None;
        let mut types = Vec::new();
        for token in self.tokens {
            match token.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => {
                    if let Some(ty) = current_type.take() {
                        types.push(ty);
                    }
                }
                TokenKind::Identifier(text)
                    if depth == 0 && FunctionParameterQualifier::from(text).is_qualifier() => {}
                TokenKind::Identifier(text) if depth == 0 && current_type.is_none() => {
                    current_type = Some(text);
                }
                _ => {}
            }
        }
        if let Some(ty) = current_type {
            types.push(ty);
        }
        types
    }
}
/// User `mod` call candidate.
#[derive(Clone, Copy)]
pub(super) struct ModCall<'module, 'src> {
    /// Syntactic function call.
    pub(super) call: FunctionCall<'module, 'src>,
}
impl<'module, 'src> From<FunctionCall<'module, 'src>> for ModCall<'module, 'src> {
    fn from(call: FunctionCall<'module, 'src>) -> Self {
        Self { call }
    }
}
/// Borrowed token range covered by a source span.
#[derive(Clone, Copy)]
pub(super) struct TokenRange<'module, 'src> {
    /// Tokens in the source range.
    pub(super) tokens: &'module [Token<'src>],
    /// Start index of the range in the original token stream.
    pub(super) start: usize,
}
impl<'module, 'src> TokenRange<'module, 'src> {
    /// Borrows tokens whose spans are contained by `span`.
    pub(super) fn new(tokens: &'module [Token<'src>], span: SourceSpan) -> Self {
        let start = tokens
            .iter()
            .position(|token| token.span.start() >= span.start())
            .unwrap_or(tokens.len());
        let end = tokens[start..]
            .iter()
            .position(|token| token.span.end() > span.end())
            .map_or(tokens.len(), |index| start + index);
        Self {
            tokens: &tokens[start..end],
            start,
        }
    }

    /// Returns the borrowed tokens.
    pub(super) const fn tokens(self) -> &'module [Token<'src>] {
        self.tokens
    }
}
/// Scalar user `mod` argument candidate.
pub(super) struct ModArgument<'module, 'src> {
    /// Tokens in the argument.
    pub(super) tokens: TokenRange<'module, 'src>,
}
impl<'module, 'src> ModArgument<'module, 'src> {
    /// Creates a scalar argument candidate.
    pub(super) const fn new(tokens: TokenRange<'module, 'src>) -> Self {
        Self { tokens }
    }

    /// Returns whether the argument is scalar enough for this overload.
    pub(super) fn scalar(self, facts: &ScalarTypeFacts<'_>) -> bool {
        let tokens = self
            .tokens
            .tokens()
            .iter()
            .enumerate()
            .filter(|(_index, token)| !matches!(token.kind, TokenKind::Comment(_)))
            .collect::<Vec<_>>();
        ScalarExpression {
            tokens: &tokens,
            base_index: self.tokens.start,
            facts,
        }
        .is_scalar_range(0, tokens.len())
    }
}

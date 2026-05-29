use super::{
    ModArgument, ModCall, ScopedDeclarationFacts, ScopedDeclarationFactsConfig,
    ScopedDeclarationTypeMode, Token, TokenKind, TokenRange,
};

impl ModCall<'_, '_> {
    /// Returns whether this call has scalar arguments for the user overload.
    pub(super) fn scalar(self, facts: &ScalarTypeFacts<'_>) -> bool {
        if self.call.argument_count() != 2 {
            return false;
        }
        let Some(first) = self.call.first_argument() else {
            return false;
        };
        let Some(second) = first.remaining_argument_span(self.call.tokens) else {
            return false;
        };
        let Some(first) = first.argument_span(self.call.tokens) else {
            return false;
        };
        ModArgument::new(TokenRange::new(self.call.tokens, first)).scalar(facts)
            && ModArgument::new(TokenRange::new(self.call.tokens, second)).scalar(facts)
    }
}
/// Known scoped declarations used to classify user `mod(float,float)` calls.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScalarTypeFacts<'src> {
    /// Declared scalar and blocker names in source order.
    pub(super) bindings: Vec<ScalarBinding<'src>>,
}
impl<'src> From<&[Token<'src>]> for ScalarTypeFacts<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let facts = ScopedDeclarationFacts::from_tokens(
            tokens,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );
        Self {
            bindings: facts
                .declarations()
                .iter()
                .map(|declaration| ScalarBinding {
                    name: declaration.name(),
                    ty: ScalarValueType::from_declared_type(declaration.ty(), facts.struct_names()),
                    visible_start: declaration.visible_start(),
                    scope_end: declaration.scope_end(),
                })
                .collect(),
        }
    }
}
impl ScalarTypeFacts<'_> {
    /// Returns whether a single identifier is known scalar.
    pub(super) fn contains(&self, name: &str, index: usize) -> bool {
        matches!(
            self.visible_type(name, index),
            Some(ScalarValueType::Scalar)
        )
    }

    /// Returns the nearest visible declaration type for `name` at `index`.
    pub(super) fn visible_type(&self, name: &str, index: usize) -> Option<ScalarValueType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(index))
            .map(|binding| binding.ty)
    }
}
/// One scoped scalar or blocker binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScalarBinding<'src> {
    /// Declared name.
    pub(super) name: &'src str,
    /// Declared value type class.
    pub(super) ty: ScalarValueType,
    /// First token where this binding can be referenced.
    pub(super) visible_start: usize,
    /// First token outside the binding scope.
    pub(super) scope_end: usize,
}
impl ScalarBinding<'_> {
    /// Returns whether this binding is visible at `index`.
    pub(super) const fn visible_at(self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}
/// Type class for user `mod(float,float)` call classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScalarValueType {
    /// Float/int/uint/bool scalar.
    Scalar,
    /// Nearest declaration is known not to be a scalar value.
    NonScalar,
    /// Type is not known by this lightweight classifier.
    Unknown,
}
impl From<&str> for ScalarValueType {
    fn from(ty: &str) -> Self {
        Self::from_declared_type(ty, &[])
    }
}
impl ScalarValueType {
    /// Classifies a declaration type spelling for scalar user `mod` routing.
    pub(super) fn from_declared_type(ty: &str, struct_names: &[&str]) -> Self {
        match ty {
            "bool" | "int" | "uint" | "float" | "float1" => Self::Scalar,
            "float2" | "float3" | "float4" | "vec2" | "vec3" | "vec4" | "ivec2" | "ivec3"
            | "ivec4" | "uvec2" | "uvec3" | "uvec4" | "bvec2" | "bvec3" | "bvec4" | "mat2"
            | "mat3" | "mat4" | "mat2x2" | "mat2x3" | "mat2x4" | "mat3x2" | "mat3x3" | "mat3x4"
            | "mat4x2" | "mat4x3" | "mat4x4" => Self::NonScalar,
            _ if struct_names.contains(&ty) => Self::NonScalar,
            _ => Self::Unknown,
        }
    }
}
/// Scalar expression classifier for user `mod(float,float)` arguments.
pub(super) struct ScalarExpression<'tokens, 'facts, 'src> {
    /// Non-comment tokens in expression order.
    pub(super) tokens: &'tokens [(usize, &'tokens Token<'src>)],
    /// Original token index of local token zero.
    pub(super) base_index: usize,
    /// Known scalar facts.
    pub(super) facts: &'facts ScalarTypeFacts<'src>,
}
impl ScalarExpression<'_, '_, '_> {
    /// Returns whether the token subrange is a scalar expression.
    pub(super) fn is_scalar_range(&self, start: usize, end: usize) -> bool {
        if start >= end {
            return false;
        }
        let (start, end) = self.trim_balanced_parentheses(start, end);
        if self.scalar_atom(start, end) {
            return true;
        }
        if self.float_constructor(start, end) {
            return true;
        }
        if matches!(self.tokens[start].1.kind, TokenKind::Punctuation('+' | '-')) {
            return self.is_scalar_range(start + 1, end);
        }
        if let Some(operator) = self.top_level_binary_operator(start, end) {
            return self.is_scalar_range(start, operator)
                && self.is_scalar_range(operator + 1, end);
        }
        false
    }

    /// Removes pairs of balanced parentheses that wrap the whole expression.
    pub(super) fn trim_balanced_parentheses(
        &self,
        mut start: usize,
        mut end: usize,
    ) -> (usize, usize) {
        while end.saturating_sub(start) >= 2
            && matches!(self.tokens[start].1.kind, TokenKind::LeftParen)
            && matches!(self.tokens[end - 1].1.kind, TokenKind::RightParen)
            && self.matching_right_paren(start, end) == Some(end - 1)
        {
            start += 1;
            end -= 1;
        }
        (start, end)
    }

    /// Returns whether the subrange is a scalar literal or known scalar name.
    pub(super) fn scalar_atom(&self, start: usize, end: usize) -> bool {
        match &self.tokens[start..end] {
            [(_, token)] => {
                SignedNumber::new(&[**token]).is_some()
                    || matches!(
                        token.kind,
                        TokenKind::Identifier(name) if self.facts.contains(
                            name,
                            self.original_index(start),
                        )
                    )
            }
            [(_, sign), (_, number)] if is_sign(sign) => {
                SignedNumber::new(&[**sign, **number]).is_some()
            }
            _ => false,
        }
    }

    /// Returns whether the subrange is `float(scalar)` or `float1(scalar)`.
    pub(super) fn float_constructor(&self, start: usize, end: usize) -> bool {
        if end.saturating_sub(start) < 4 {
            return false;
        }
        if !matches!(
            self.tokens[start].1.kind,
            TokenKind::Identifier("float" | "float1")
        ) || !matches!(self.tokens[start + 1].1.kind, TokenKind::LeftParen)
            || !matches!(self.tokens[end - 1].1.kind, TokenKind::RightParen)
            || self.matching_right_paren(start + 1, end) != Some(end - 1)
        {
            return false;
        }
        self.is_scalar_range(start + 2, end - 1)
    }

    /// Finds a top-level scalar binary operator, preferring lower-precedence
    /// split points.
    pub(super) fn top_level_binary_operator(&self, start: usize, end: usize) -> Option<usize> {
        self.top_level_operator(start, end, &['+', '-'])
            .or_else(|| self.top_level_operator(start, end, &['*', '/', '%']))
    }

    /// Finds the last top-level operator in `operators`.
    pub(super) fn top_level_operator(
        &self,
        start: usize,
        end: usize,
        operators: &[char],
    ) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut found = None;
        for index in start..end {
            match self.tokens[index].1.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::Punctuation(operator)
                    if paren_depth == 0
                        && bracket_depth == 0
                        && operators.contains(&operator)
                        && index > start
                        && index + 1 < end
                        && !self.is_unary_sign(index, start) =>
                {
                    found = Some(index);
                }
                _ => {}
            }
        }
        found
    }

    /// Returns whether `+` or `-` starts a unary expression in the current
    /// range.
    pub(super) fn is_unary_sign(&self, index: usize, start: usize) -> bool {
        if !matches!(self.tokens[index].1.kind, TokenKind::Punctuation('+' | '-')) {
            return false;
        }
        if index <= start {
            return true;
        }
        matches!(
            self.tokens[index - 1].1.kind,
            TokenKind::LeftParen
                | TokenKind::Comma
                | TokenKind::Punctuation('=' | '?' | ':' | '<' | '>' | '+' | '-' | '*' | '/' | '%')
        )
    }

    /// Returns the matching right parenthesis within this expression range.
    pub(super) fn matching_right_paren(&self, open: usize, end: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in open..end {
            match self.tokens[index].1.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Returns the token index in the original module token stream.
    pub(super) const fn original_index(&self, index: usize) -> usize {
        self.base_index + self.tokens[index].0
    }
}
/// Optionally signed numeric literal token sequence.
pub(super) struct SignedNumber;
impl SignedNumber {
    /// Creates a signed-number marker when the tokens represent one.
    pub(super) fn new(tokens: &[Token<'_>]) -> Option<Self> {
        if matches!(tokens, [token] if matches!(token.kind, TokenKind::Number(_)))
            || matches!(tokens, [sign, number] if is_sign(sign) && matches!(number.kind, TokenKind::Number(_)))
        {
            Some(Self)
        } else {
            None
        }
    }
}
/// Returns whether a token is `+` or `-`.
pub(super) const fn is_sign(token: &Token<'_>) -> bool {
    matches!(token.kind, TokenKind::Punctuation('+' | '-'))
}

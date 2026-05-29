use super::expr::Lvalue;
use crate::{
    legalizer::{
        ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
        TokenSearch, tokens::BalancedTokens,
    },
    lexer::{Token, TokenKind},
};

/// Known scalar symbol facts from simple declarations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SymbolFacts<'src> {
    /// Scoped declarations in source order.
    bindings: Vec<SymbolBinding<'src>>,
    /// Object-like numeric macro definitions visible across the source.
    macros: Vec<MacroSymbol<'src>>,
}

impl<'src> From<&[Token<'src>]> for SymbolFacts<'src> {
    fn from(tokens: &[Token<'src>]) -> Self {
        let facts = ScopedDeclarationFacts::from_tokens(
            tokens,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );
        Self {
            bindings: facts
                .declarations()
                .iter()
                .map(|declaration| SymbolBinding {
                    name: declaration.name(),
                    ty: SymbolType::from(declaration.ty()),
                    visible_start: declaration.visible_start(),
                    scope_end: declaration.scope_end(),
                })
                .collect(),
            macros: tokens
                .iter()
                .filter_map(|token| MacroSymbol::try_from(*token).ok())
                .collect(),
        }
    }
}

impl SymbolFacts<'_> {
    /// Returns whether `kind` clearly denotes a boolean expression.
    pub(super) fn bool_identifier(&self, tokens: &[Token<'_>], index: usize) -> bool {
        let TokenKind::Identifier(name) = tokens[index].kind else {
            return false;
        };

        matches!(name, "true" | "false")
            || matches!(self.visible_type(name, index), Some(SymbolType::Bool))
    }

    /// Returns whether `lvalue` is known to be a float scalar/component.
    pub(super) fn float_lvalue(&self, lvalue: Lvalue<'_>) -> bool {
        match self.visible_type(lvalue.base, lvalue.end) {
            Some(SymbolType::Float) => !lvalue.has_member,
            Some(SymbolType::FloatVector) => lvalue.has_member,
            _ => false,
        }
    }

    /// Returns the known scalar numeric type for a condition expression.
    pub(super) fn numeric_expression_type(
        &self,
        tokens: &[Token<'_>],
        start: usize,
        end: usize,
    ) -> Option<NumericScalarType> {
        match (ExpressionType {
            tokens,
            facts: self,
        })
        .range_type(start, end)?
        {
            SymbolType::Int => Some(NumericScalarType::Int),
            SymbolType::Uint => Some(NumericScalarType::Uint),
            SymbolType::Float => Some(NumericScalarType::Float),
            SymbolType::Bool | SymbolType::FloatVector | SymbolType::NonFloatAggregate => None,
        }
    }

    /// Returns the nearest visible declaration type for `name` at `index`.
    pub(super) fn visible_type(&self, name: &str, index: usize) -> Option<SymbolType> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name && binding.visible_at(index))
            .map(|binding| binding.ty)
            .or_else(|| {
                self.macros
                    .iter()
                    .rev()
                    .find(|symbol| symbol.name == name)
                    .map(|symbol| symbol.ty)
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Numeric object-like macro usable in expression type inference.
struct MacroSymbol<'src> {
    /// Macro identifier.
    name: &'src str,
    /// Numeric scalar type of the replacement.
    ty: SymbolType,
}

impl<'src> TryFrom<Token<'src>> for MacroSymbol<'src> {
    type Error = ();

    fn try_from(token: Token<'src>) -> Result<Self, Self::Error> {
        let TokenKind::Directive(text) = token.kind else {
            return Err(());
        };
        let mut parts = text.split_whitespace();
        if parts.next() != Some("#define") {
            return Err(());
        }
        let name = parts.next().ok_or(())?;
        if name.contains('(') {
            return Err(());
        }
        let value = parts.next().ok_or(())?;
        if parts.next().is_some() {
            return Err(());
        }
        let ty = SymbolType::from_numeric_literal(value).ok_or(())?;
        Ok(Self { name, ty })
    }
}

#[derive(Clone, Copy)]
/// Recursive scalar type inference for simple condition expressions.
struct ExpressionType<'tokens, 'facts, 'src> {
    /// Tokens being classified.
    tokens: &'tokens [Token<'src>],
    /// Known symbol facts.
    facts: &'facts SymbolFacts<'src>,
}

impl ExpressionType<'_, '_, '_> {
    /// Returns the known scalar type for an inclusive token range.
    fn range_type(self, start: usize, end: usize) -> Option<SymbolType> {
        let start = self.next_non_comment(start, end)?;
        let end = self.previous_non_comment(start, end)?;
        if start > end {
            return None;
        }
        if matches!(self.tokens[start].kind, TokenKind::Punctuation('+' | '-')) {
            return self.range_type(start + 1, end);
        }
        if self.outer_parens(start, end) {
            return self.range_type(start + 1, end - 1);
        }
        if let Some(operator) = self.top_level_operator(start, end, &['+', '-']) {
            let left = self.range_type(start, operator.saturating_sub(1));
            let right = self.range_type(operator + 1, end);
            return SymbolType::strict_arithmetic_result(left, right);
        }
        if let Some(operator) = self.top_level_operator(start, end, &['*', '/', '%']) {
            let left = self.range_type(start, operator.saturating_sub(1));
            let right = self.range_type(operator + 1, end);
            return SymbolType::strict_arithmetic_result(left, right);
        }
        if start == end {
            return self.single_token_type(start);
        }
        if self.function_member_selection(start, end) {
            return Some(SymbolType::Float);
        }
        if matches!(self.tokens[end].kind, TokenKind::Identifier(_))
            && let Some(lvalue) = Lvalue::ending_at(self.tokens, end)
            && lvalue.has_member
        {
            return Some(SymbolType::Float);
        }
        None
    }

    /// Returns whether this range selects a scalar member from a function call,
    /// such as `texSample2D(...).x`.
    fn function_member_selection(self, start: usize, end: usize) -> bool {
        if !matches!(self.tokens[end].kind, TokenKind::Identifier(_)) {
            return false;
        }
        let search = TokenSearch::new(self.tokens);
        let Some(dot) = search.previous_non_comment(end) else {
            return false;
        };
        if !matches!(self.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return false;
        }
        let Some(close) = search.previous_non_comment(dot) else {
            return false;
        };
        if !matches!(self.tokens[close].kind, TokenKind::RightParen) {
            return false;
        }
        let Some(open) = self.matching_left_paren(close) else {
            return false;
        };
        let Some(name) = search.previous_non_comment(open) else {
            return false;
        };
        start <= name && matches!(self.tokens[name].kind, TokenKind::Identifier(_))
    }

    /// Finds the left parenthesis that matches `close`.
    fn matching_left_paren(self, close: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in (0..=close).rev() {
            match self.tokens[index].kind {
                TokenKind::RightParen => depth += 1,
                TokenKind::LeftParen => {
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

    /// Returns the known scalar type for a single token.
    fn single_token_type(self, index: usize) -> Option<SymbolType> {
        match self.tokens[index].kind {
            TokenKind::Identifier(name) => self.facts.visible_type(name, index),
            TokenKind::Number(text) => SymbolType::from_numeric_literal(text),
            _ => None,
        }
    }

    /// Returns the next non-comment token inside the inclusive range.
    fn next_non_comment(self, start: usize, end: usize) -> Option<usize> {
        let index = TokenSearch::new(self.tokens).next_non_comment(start)?;
        (index <= end).then_some(index)
    }

    /// Returns the previous non-comment token inside the inclusive range.
    fn previous_non_comment(self, start: usize, end: usize) -> Option<usize> {
        let index = TokenSearch::new(self.tokens).previous_non_comment(end + 1)?;
        (start <= index).then_some(index)
    }

    /// Returns whether the range is fully wrapped by one outer parenthesis
    /// pair.
    fn outer_parens(self, start: usize, end: usize) -> bool {
        if !matches!(self.tokens[start].kind, TokenKind::LeftParen)
            || !matches!(self.tokens[end].kind, TokenKind::RightParen)
        {
            return false;
        }
        BalancedTokens::new(self.tokens).matching_right_paren(start) == Some(end)
    }

    /// Returns the rightmost top-level operator whose spelling is in
    /// `operators`.
    fn top_level_operator(self, start: usize, end: usize, operators: &[char]) -> Option<usize> {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut found = None;

        for (index, token) in self.tokens.iter().enumerate().take(end + 1).skip(start) {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.checked_sub(1)?,
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.checked_sub(1)?,
                TokenKind::Punctuation(operator)
                    if paren_depth == 0
                        && bracket_depth == 0
                        && operators.contains(&operator)
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
    fn is_unary_sign(self, index: usize, start: usize) -> bool {
        if !matches!(self.tokens[index].kind, TokenKind::Punctuation('+' | '-')) {
            return false;
        }
        let Some(previous) = TokenSearch::new(self.tokens).previous_non_comment(index) else {
            return true;
        };
        if previous < start {
            return true;
        }
        matches!(
            self.tokens[previous].kind,
            TokenKind::LeftParen
                | TokenKind::Comma
                | TokenKind::Punctuation('=' | '?' | ':' | '<' | '>' | '+' | '-' | '*' | '/' | '%')
        )
    }
}

/// One scoped symbol binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SymbolBinding<'src> {
    /// Declared name.
    name: &'src str,
    /// Declared type class.
    ty: SymbolType,
    /// First token where this binding may be referenced.
    visible_start: usize,
    /// First token outside this binding's scope.
    scope_end: usize,
}

impl SymbolBinding<'_> {
    /// Returns whether this binding is visible at `index`.
    const fn visible_at(self, index: usize) -> bool {
        self.visible_start <= index && index < self.scope_end
    }
}

/// Type facts needed by control-flow coercions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SymbolType {
    /// Boolean scalar.
    Bool,
    /// Signed integer scalar.
    Int,
    /// Unsigned integer scalar.
    Uint,
    /// Floating-point scalar.
    Float,
    /// Vector whose components are floating-point scalars.
    FloatVector,
    /// Non-float or unknown non-scalar value that can shadow a scalar name.
    NonFloatAggregate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Scalar numeric type facts for condition coercion.
pub(super) enum NumericScalarType {
    /// Signed integer scalar.
    Int,
    /// Unsigned integer scalar.
    Uint,
    /// Floating-point scalar.
    Float,
}

impl From<&str> for SymbolType {
    fn from(ty: &str) -> Self {
        match ty {
            "bool" => Self::Bool,
            "int" => Self::Int,
            "uint" => Self::Uint,
            "float" | "float1" => Self::Float,
            "vec2" | "vec3" | "vec4" | "float2" | "float3" | "float4" => Self::FloatVector,
            _ => Self::NonFloatAggregate,
        }
    }
}

impl SymbolType {
    /// Returns the scalar type for a GLSL numeric literal spelling.
    fn from_numeric_literal(text: &str) -> Option<Self> {
        if text.contains(['.', 'e', 'E']) {
            Some(Self::Float)
        } else if text.ends_with(['u', 'U']) {
            Some(Self::Uint)
        } else if text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'))
        {
            Some(Self::Int)
        } else {
            None
        }
    }

    /// Returns the scalar result type for a known arithmetic expression.
    fn strict_arithmetic_result(left: Option<Self>, right: Option<Self>) -> Option<Self> {
        if matches!(left, Some(Self::Float)) || matches!(right, Some(Self::Float)) {
            Some(Self::Float)
        } else if matches!((left, right), (Some(Self::Int), Some(Self::Int))) {
            Some(Self::Int)
        } else if matches!((left, right), (Some(Self::Uint), Some(Self::Uint))) {
            Some(Self::Uint)
        } else {
            None
        }
    }
}

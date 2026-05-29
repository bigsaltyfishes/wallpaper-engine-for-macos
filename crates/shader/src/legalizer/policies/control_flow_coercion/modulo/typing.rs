use super::{
    BalancedTokens, Lvalue, SymbolFacts, SymbolType, Token, TokenKind, TokenSearch,
    rewrite::ExpressionSource,
};

/// Recursive scalar type inference for simple arithmetic expressions.
#[derive(Clone, Copy)]
pub(super) struct ExpressionType<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Known symbol facts.
    pub(super) facts: &'tokens SymbolFacts<'src>,
}
impl ExpressionType<'_, '_> {
    /// Returns the known scalar type for an operand token range.
    pub(super) fn range_type(self, start: usize, end: usize) -> Option<SymbolType> {
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
            return SymbolType::arithmetic_result(left, right);
        }
        if let Some(operator) = self.top_level_operator(start, end, &['*', '/', '%']) {
            let left = self.range_type(start, operator.saturating_sub(1));
            let right = self.range_type(operator + 1, end);
            return SymbolType::arithmetic_result(left, right);
        }
        if matches!(self.tokens[end].kind, TokenKind::Identifier(_))
            && let Some(lvalue) = Lvalue::ending_at(self.tokens, end)
            && lvalue.has_member
        {
            return self.facts.float_lvalue(lvalue).then_some(SymbolType::Float);
        }
        if start == end {
            return self.single_token_type(start);
        }
        if let Some(open) = self.next_non_comment(start + 1, end)
            && matches!(self.tokens[open].kind, TokenKind::LeftParen)
        {
            return match self.tokens[start].kind {
                TokenKind::Identifier("float") => Some(SymbolType::Float),
                TokenKind::Identifier("int") => Some(SymbolType::Int),
                TokenKind::Identifier("uint") => Some(SymbolType::Uint),
                _ => None,
            };
        }
        None
    }

    /// Returns the known scalar type for a single token operand.
    pub(super) fn single_token_type(self, index: usize) -> Option<SymbolType> {
        match self.tokens[index].kind {
            TokenKind::Identifier(name) => self.facts.visible_type(name, index),
            TokenKind::Number(text) if text.contains(['.', 'e', 'E']) => Some(SymbolType::Float),
            TokenKind::Number(text) if text.ends_with(['u', 'U']) => Some(SymbolType::Uint),
            TokenKind::Number(_) => Some(SymbolType::Int),
            _ => None,
        }
    }

    /// Returns the next non-comment token inside the inclusive range.
    pub(super) fn next_non_comment(self, start: usize, end: usize) -> Option<usize> {
        let index = TokenSearch::new(self.tokens).next_non_comment(start)?;
        (index <= end).then_some(index)
    }

    /// Returns the previous non-comment token inside the inclusive range.
    pub(super) fn previous_non_comment(self, start: usize, end: usize) -> Option<usize> {
        let index = TokenSearch::new(self.tokens).previous_non_comment(end + 1)?;
        (start <= index).then_some(index)
    }

    /// Returns whether the range is fully wrapped by one outer parenthesis
    /// pair.
    pub(super) fn outer_parens(self, start: usize, end: usize) -> bool {
        if !matches!(self.tokens[start].kind, TokenKind::LeftParen)
            || !matches!(self.tokens[end].kind, TokenKind::RightParen)
        {
            return false;
        }
        BalancedTokens::new(self.tokens).matching_right_paren(start) == Some(end)
    }

    /// Returns the rightmost top-level operator whose spelling is in
    /// `operators`.
    pub(super) fn top_level_operator(
        self,
        start: usize,
        end: usize,
        operators: &[char],
    ) -> Option<usize> {
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
    pub(super) fn is_unary_sign(self, index: usize, start: usize) -> bool {
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
impl SymbolType {
    /// Returns whether both operands are known integer modulo operands.
    pub(super) fn integer_modulo_operands(left: Option<Self>, right: Option<Self>) -> bool {
        matches!(
            (left, right),
            (Some(Self::Int | Self::Uint), Some(Self::Int | Self::Uint))
        )
    }

    /// Returns the scalar result type for a known arithmetic expression.
    pub(super) fn arithmetic_result(left: Option<Self>, right: Option<Self>) -> Option<Self> {
        if matches!(left, Some(Self::Float)) || matches!(right, Some(Self::Float)) {
            Some(Self::Float)
        } else if Self::integer_modulo_operands(left, right) {
            Some(Self::integer_result(left, right))
        } else {
            None
        }
    }

    /// Returns the integer result type for an integer binary expression.
    pub(super) fn integer_result(left: Option<Self>, right: Option<Self>) -> Self {
        if matches!((left, right), (Some(Self::Uint), Some(Self::Uint))) {
            Self::Uint
        } else {
            Self::Int
        }
    }
}

impl ExpressionSource {
    /// Emits a binary expression from two lowered operands.
    pub(super) fn binary(left: Self, operator: impl Into<String>, right: Self) -> Self {
        Self::default()
            .with_expression(left)
            .with_text(operator)
            .with_expression(right)
    }

    /// Coerces a known integer operand to float for arithmetic modulo lowering.
    pub(super) fn into_float_operand(self, ty: Option<SymbolType>) -> Self {
        if matches!(ty, Some(SymbolType::Int | SymbolType::Uint)) {
            Self::text("float(").with_expression(self).with_text(")")
        } else {
            self
        }
    }
}

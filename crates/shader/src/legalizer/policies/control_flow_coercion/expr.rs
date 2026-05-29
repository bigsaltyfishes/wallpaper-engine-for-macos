use super::{
    statements::Statement,
    symbols::{NumericScalarType, SymbolFacts},
};
use crate::{
    SourceSpan,
    legalizer::TokenSearch,
    lexer::{Token, TokenKind},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token-backed assignable expression.
pub(super) struct Lvalue<'src> {
    /// First token in the lvalue.
    pub(super) start: usize,
    /// Last token in the lvalue.
    pub(super) end: usize,
    /// Base identifier name.
    pub(super) base: &'src str,
    /// Whether the lvalue selects a scalar member such as `color.x`.
    pub(super) has_member: bool,
}

impl<'src> Lvalue<'src> {
    /// Finds a simple identifier/member/index lvalue ending at `end`.
    pub(super) fn ending_at(tokens: &[Token<'src>], end: usize) -> Option<Self> {
        let (mut start, mut base, mut has_member) = match tokens[end].kind {
            TokenKind::Identifier(name) => (end, name, false),
            TokenKind::Punctuation(']') => {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..=end).rev() {
                    match tokens[index].kind {
                        TokenKind::Punctuation(']') => depth += 1,
                        TokenKind::Punctuation('[') => {
                            depth = depth.checked_sub(1)?;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let base_end = TokenSearch::new(tokens).previous_non_comment(open?)?;
                let lvalue = Self::ending_at(tokens, base_end)?;
                (lvalue.start, lvalue.base, lvalue.has_member)
            }
            _ => return None,
        };

        let search = TokenSearch::new(tokens);
        while let Some(dot) = search.previous_non_comment(start) {
            if !matches!(tokens[dot].kind, TokenKind::Punctuation('.')) {
                break;
            }
            let base_end = search.previous_non_comment(dot)?;
            let lvalue = Self::ending_at(tokens, base_end)?;
            start = lvalue.start;
            base = lvalue.base;
            has_member = true;
        }

        Some(Self {
            start,
            end,
            base,
            has_member,
        })
    }
}

/// Boolean-valued expression marker.
pub(super) struct BoolExpression;

/// Input used to classify a token range as boolean-valued.
#[derive(Clone, Copy)]
pub(super) struct BoolExpressionInput<'statement, 'tokens, 'src> {
    /// Statement containing the expression.
    pub(super) statement: Statement<'tokens, 'src>,
    /// First expression token.
    pub(super) start: usize,
    /// Last expression token.
    pub(super) end: usize,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}

impl TryFrom<BoolExpressionInput<'_, '_, '_>> for BoolExpression {
    type Error = ();

    fn try_from(input: BoolExpressionInput<'_, '_, '_>) -> Result<Self, Self::Error> {
        let tokens = input.statement.tokens;
        if input.start > input.end {
            return Err(());
        }
        if input.start == input.end && input.facts.bool_identifier(tokens, input.start) {
            return Ok(Self);
        }
        if matches!(tokens[input.start].kind, TokenKind::Punctuation('!')) {
            return Ok(Self);
        }
        if matches!(tokens[input.start].kind, TokenKind::LeftParen)
            && matches!(tokens[input.end].kind, TokenKind::RightParen)
            && BoolExpression::try_from(BoolExpressionInput {
                statement: input.statement,
                start: input.start + 1,
                end: input.end.saturating_sub(1),
                facts: input.facts,
            })
            .is_ok()
        {
            return Ok(Self);
        }

        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut previous: Option<TokenKind<'_>> = None;
        for token in &tokens[input.start..=input.end] {
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::Punctuation('[') => bracket_depth += 1,
                TokenKind::Punctuation(']') => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::Punctuation('<' | '>' | '!')
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return Ok(Self);
                }
                TokenKind::Punctuation('=')
                    if paren_depth == 0
                        && bracket_depth == 0
                        && matches!(
                            previous,
                            Some(TokenKind::Punctuation('=' | '!' | '<' | '>'))
                        ) =>
                {
                    return Ok(Self);
                }
                TokenKind::Punctuation('&' | '|')
                    if paren_depth == 0 && bracket_depth == 0 && previous == Some(token.kind) =>
                {
                    return Ok(Self);
                }
                _ => {}
            }
            previous = Some(token.kind);
        }

        Err(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Token-backed numeric condition expression.
pub(super) struct NumericCondition {
    /// Source span covering the expression.
    pub(super) span: SourceSpan,
    /// Scalar numeric type of the expression.
    pub(super) ty: NumericScalarType,
}

impl NumericCondition {
    /// Returns the zero literal that matches the expression scalar type.
    pub(super) const fn zero_literal(self) -> &'static str {
        match self.ty {
            NumericScalarType::Float => "0.0",
            NumericScalarType::Int => "0",
            NumericScalarType::Uint => "0u",
        }
    }
}

/// Input used to classify condition token ranges that GLSL requires as bools.
#[derive(Clone, Copy)]
pub(super) struct NumericConditionInput<'statement, 'tokens, 'src> {
    /// Statement containing the expression.
    pub(super) statement: Statement<'tokens, 'src>,
    /// First expression token.
    pub(super) start: usize,
    /// Last expression token.
    pub(super) end: usize,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}

impl TryFrom<NumericConditionInput<'_, '_, '_>> for NumericCondition {
    type Error = ();

    fn try_from(input: NumericConditionInput<'_, '_, '_>) -> Result<Self, Self::Error> {
        if BoolExpression::try_from(BoolExpressionInput {
            statement: input.statement,
            start: input.start,
            end: input.end,
            facts: input.facts,
        })
        .is_ok()
        {
            return Err(());
        }
        let Some(ty) =
            input
                .facts
                .numeric_expression_type(input.statement.tokens, input.start, input.end)
        else {
            return Err(());
        };
        SourceSpan::new(
            input.statement.tokens[input.start].span.start(),
            input.statement.tokens[input.end].span.end(),
        )
        .map(|span| Self { span, ty })
        .map_err(|_error| ())
    }
}

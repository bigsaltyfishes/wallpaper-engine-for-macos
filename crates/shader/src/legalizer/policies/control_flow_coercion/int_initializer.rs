use super::{
    statements::Statement,
    symbols::{SymbolFacts, SymbolType},
};
use crate::{
    legalizer::{ExpressionReplacement, Fixup, TokenSearch},
    lexer::{Token, TokenKind},
};

/// Integer variable initialized from an expression whose result is float.
pub(super) struct IntFloatInitializer<'statement, 'tokens, 'src> {
    /// Statement being inspected.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}

impl IntFloatInitializer<'_, '_, '_> {
    /// Emits initializer casts for int declarations initialized by
    /// float-valued expressions.
    pub(super) fn fixups(self) -> Vec<Fixup> {
        let tokens = self.statement.tokens;
        let Some(declarations) = self.statement.declaration_declarators("int") else {
            return Vec::new();
        };

        declarations
            .filter_map(|declaration| {
                let initializer = IntFloatDeclarator {
                    declaration,
                    facts: self.facts,
                }
                .float_initializer(tokens)?;
                let replacement = ExpressionReplacement::new()
                    .with_text("int(")
                    .with_source(initializer.span())
                    .with_text(")");
                Some(Fixup::replace(initializer.span(), replacement))
            })
            .collect()
    }
}

/// One int declarator candidate.
#[derive(Clone, Copy)]
struct IntFloatDeclarator<'facts, 'src> {
    /// Parsed declarator.
    declaration: crate::legalizer::LocalDeclaration<'src>,
    /// Known symbol facts.
    facts: &'facts SymbolFacts<'src>,
}

impl IntFloatDeclarator<'_, '_> {
    /// Returns this declarator's initializer when it is float-valued.
    fn float_initializer(
        self,
        tokens: &[Token<'_>],
    ) -> Option<crate::legalizer::DeclaratorInitializer> {
        let initializer = self.declaration.initializer(tokens)?;
        ExpressionKind {
            tokens,
            facts: self.facts,
        }
        .range_type(initializer.start(), initializer.end())
        .is_some_and(|ty| ty == SymbolType::Float)
        .then_some(initializer)
    }
}

/// Recursive scalar type inference for integer declaration initializers.
#[derive(Clone, Copy)]
struct ExpressionKind<'tokens, 'facts, 'src> {
    /// Tokens being classified.
    tokens: &'tokens [Token<'src>],
    /// Known symbol facts.
    facts: &'facts SymbolFacts<'src>,
}

impl ExpressionKind<'_, '_, '_> {
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
            return Some(arithmetic_result_type(left?, right?));
        }
        if let Some(operator) = self.top_level_operator(start, end, &['*', '/', '%']) {
            let left = self.range_type(start, operator.saturating_sub(1));
            let right = self.range_type(operator + 1, end);
            return Some(arithmetic_result_type(left?, right?));
        }
        if start == end {
            return self.single_token_type(start);
        }
        if let Some(ty) = self.whole_function_call_return_type(start, end) {
            return Some(ty);
        }
        if self.float_member_selection(start, end) {
            return Some(SymbolType::Float);
        }
        None
    }

    /// Returns the known scalar type for a single token.
    fn single_token_type(self, index: usize) -> Option<SymbolType> {
        match self.tokens[index].kind {
            TokenKind::Identifier(name) => self.facts.visible_type(name, index),
            TokenKind::Number(text) if text.contains(['.', 'e', 'E']) => Some(SymbolType::Float),
            TokenKind::Number(text) if text.ends_with(['u', 'U']) => Some(SymbolType::Uint),
            TokenKind::Number(text)
                if text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-')) =>
            {
                Some(SymbolType::Int)
            }
            _ => None,
        }
    }

    /// Returns a function call if the whole range is exactly one call.
    fn whole_function_call_return_type(self, start: usize, end: usize) -> Option<SymbolType> {
        let search = TokenSearch::new(self.tokens);
        let balanced = crate::legalizer::tokens::BalancedTokens::new(self.tokens);
        let TokenKind::Identifier(name) = self.tokens[start].kind else {
            return None;
        };
        let open = search.next_non_comment(start + 1)?;
        if !matches!(self.tokens[open].kind, TokenKind::LeftParen) {
            return None;
        }
        let close = balanced.matching_right_paren(open)?;
        (close == end
            && matches!(
                name,
                "acos"
                    | "asin"
                    | "atan"
                    | "ceil"
                    | "cos"
                    | "degrees"
                    | "exp"
                    | "exp2"
                    | "floor"
                    | "fract"
                    | "fwidth"
                    | "log"
                    | "log2"
                    | "mod"
                    | "pow"
                    | "radians"
                    | "sin"
                    | "sqrt"
                    | "tan"
                    | "trunc"
            ))
        .then_some(SymbolType::Float)
    }

    /// Returns whether the range is a known float scalar member selection.
    fn float_member_selection(self, start: usize, end: usize) -> bool {
        let TokenKind::Identifier(field) = self.tokens[end].kind else {
            return false;
        };
        if field.is_empty()
            || !field.bytes().all(|component| {
                matches!(
                    component,
                    b'x' | b'y' | b'z' | b'w' | b'r' | b'g' | b'b' | b'a'
                )
            })
        {
            return false;
        }
        let search = TokenSearch::new(self.tokens);
        let Some(dot) = search.previous_non_comment(end) else {
            return false;
        };
        if !matches!(self.tokens[dot].kind, TokenKind::Punctuation('.')) {
            return false;
        }
        let Some(base_end) = search.previous_non_comment(dot) else {
            return false;
        };

        self.float_vector_lvalue(start, base_end) || self.float_vector_call(start, base_end)
    }

    /// Returns whether the range is a known float vector lvalue.
    fn float_vector_lvalue(self, start: usize, end: usize) -> bool {
        if start != end {
            return false;
        }
        let TokenKind::Identifier(name) = self.tokens[start].kind else {
            return false;
        };
        matches!(
            self.facts.visible_type(name, start),
            Some(SymbolType::FloatVector)
        )
    }

    /// Returns whether the range is a call known to return a float vector.
    fn float_vector_call(self, start: usize, end: usize) -> bool {
        let search = TokenSearch::new(self.tokens);
        if !matches!(self.tokens[end].kind, TokenKind::RightParen) {
            return false;
        }
        let Some(open) = self.matching_left_paren(end) else {
            return false;
        };
        if open <= start {
            return false;
        }
        let Some(name_index) = search.previous_non_comment(open) else {
            return false;
        };
        if name_index != start {
            return false;
        }
        matches!(
            self.tokens[name_index].kind,
            TokenKind::Identifier(
                "texture"
                    | "texture2D"
                    | "textureLod"
                    | "texture2DLod"
                    | "texSample2D"
                    | "texSample2DLod"
            )
        )
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
        crate::legalizer::tokens::BalancedTokens::new(self.tokens).matching_right_paren(start)
            == Some(end)
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

/// Returns the scalar result type for a known arithmetic expression.
const fn arithmetic_result_type(left: SymbolType, right: SymbolType) -> SymbolType {
    if matches!(left, SymbolType::Float) || matches!(right, SymbolType::Float) {
        SymbolType::Float
    } else if matches!(left, SymbolType::Uint) || matches!(right, SymbolType::Uint) {
        SymbolType::Uint
    } else {
        SymbolType::Int
    }
}

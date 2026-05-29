use super::{
    expr::{BoolExpression, BoolExpressionInput, Lvalue},
    statements::Statement,
    symbols::SymbolFacts,
};
use crate::{
    legalizer::{ExpressionReplacement, Fixup, TokenSearch},
    lexer::TokenKind,
};

/// Float initialized from a boolean expression.
pub(super) struct BoolFloatInitializer<'statement, 'tokens, 'src> {
    /// Statement being inspected.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}

impl BoolFloatInitializer<'_, '_, '_> {
    /// Emits all boolean-expression initializer coercions in this declaration.
    pub(super) fn fixups(self) -> Vec<Fixup> {
        let tokens = self.statement.tokens;
        let Some(declarations) = self.statement.declaration_declarators("float") else {
            return Vec::new();
        };
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            if BoolExpression::try_from(BoolExpressionInput {
                statement: self.statement,
                start: initializer.start(),
                end: initializer.end(),
                facts: self.facts,
            })
            .is_err()
            {
                continue;
            }
            let rhs_span = initializer.span();
            let replacement = ExpressionReplacement::new()
                .with_text("((")
                .with_source(rhs_span)
                .with_text(") ? 1.0 : 0.0)");
            fixups.push(Fixup::replace(rhs_span, replacement));
        }
        fixups
    }
}

/// Float multiplied by a boolean via compound assignment.
pub(super) struct FloatTimesBool<'statement, 'tokens, 'src> {
    /// Statement being inspected.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}

impl TryFrom<FloatTimesBool<'_, '_, '_>> for Fixup {
    type Error = ();

    fn try_from(input: FloatTimesBool<'_, '_, '_>) -> Result<Self, Self::Error> {
        let tokens = input.statement.tokens;
        let search = TokenSearch::new(tokens);
        for index in input.statement.start..input.statement.semicolon {
            if !matches!(tokens[index].kind, TokenKind::Punctuation('*')) {
                continue;
            }
            let equals = search.next_non_comment(index + 1).ok_or(())?;
            if !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
                continue;
            }
            let lhs_end = search.previous_non_comment(index).ok_or(())?;
            let lhs = Lvalue::ending_at(tokens, lhs_end).ok_or(())?;
            if !input.facts.float_lvalue(lhs) {
                continue;
            }
            let rhs = search.next_non_comment(equals + 1).ok_or(())?;
            if !input.facts.bool_identifier(tokens, rhs) {
                continue;
            }
            let after_rhs = search.next_non_comment(rhs + 1).ok_or(())?;
            if after_rhs != input.statement.semicolon {
                continue;
            }
            let replacement = ExpressionReplacement::new()
                .with_text("(")
                .with_source(tokens[rhs].span)
                .with_text(" ? 1.0 : 0.0)");
            return Ok(Fixup::replace(tokens[rhs].span, replacement));
        }
        Err(())
    }
}

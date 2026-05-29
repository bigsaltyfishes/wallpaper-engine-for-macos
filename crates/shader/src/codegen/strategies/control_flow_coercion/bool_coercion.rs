use super::{
    expr::{BoolExpression, BoolExpressionInput},
    statements::StatementFixupInput,
    symbols::SymbolFacts,
};
use crate::{
    codegen::{ExpressionReplacement, Fixup, expressions::analysis::Lvalue},
    tokenizer::{
        AssignmentOperator, OperatorType as ShaderOp, TokenCursor, TypedToken, TypedTokenFacts,
    },
};

/// Float initialized from a boolean expression.
pub(super) struct BoolFloatInitializer<'statement, 'src> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'statement TypedTokenFacts,
}

impl BoolFloatInitializer<'_, '_> {
    /// Emits all boolean-expression initializer coercions in this declaration.
    pub(super) fn fixups(self, tokens: TokenCursor<'_>) -> Vec<Fixup> {
        let declarations =
            self.statement
                .declaration_declarators(tokens, self.token_facts, "float");
        if declarations.is_empty() {
            return Vec::new();
        }
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            if BoolExpression::classify(
                BoolExpressionInput {
                    start: initializer.start(),
                    end: initializer.end(),
                    facts: self.facts,
                    token_facts: self.token_facts,
                },
                tokens,
            )
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
#[derive(Clone, Copy)]
pub(super) struct FloatTimesBool<'statement, 'src> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
}

impl FloatTimesBool<'_, '_> {
    /// Builds a fixup for float compound multiplication by a boolean.
    pub(super) fn build_fixup(self, tokens: TokenCursor<'_>) -> Result<Fixup, ()> {
        let search = tokens;
        let semicolon = self.statement.semicolon(tokens).ok_or(())?;
        for index in self.statement.start()..semicolon {
            if !matches!(
                tokens[index].kind(),
                TypedToken::Operator(ShaderOp::Assignment(AssignmentOperator::MultiplyAssign,))
            ) {
                continue;
            }
            let lhs_end = search.previous_non_comment(index).ok_or(())?;
            let lhs = Lvalue::ending_at(tokens, lhs_end).ok_or(())?;
            if !self.facts.float_lvalue(&lhs) {
                continue;
            }
            let rhs = search.next_non_comment(index + 1).ok_or(())?;
            if !self.facts.bool_identifier(tokens, rhs) {
                continue;
            }
            let after_rhs = search.next_non_comment(rhs + 1).ok_or(())?;
            if after_rhs != semicolon {
                continue;
            }
            let replacement = ExpressionReplacement::new()
                .with_text("(")
                .with_source(tokens[rhs].span())
                .with_text(" ? 1.0 : 0.0)");
            return Ok(Fixup::replace(tokens[rhs].span(), replacement));
        }
        Err(())
    }
}

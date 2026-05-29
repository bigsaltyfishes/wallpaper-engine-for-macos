/// Operand detection for float modulo lowering.
mod operands;
/// Replacement construction for float modulo lowering.
mod rewrite;
/// Type inference helpers for float modulo lowering.
mod typing;

use super::{
    statements::StatementFixupInput,
    symbols::{SymbolFacts, SymbolType},
};
use crate::{
    SourceSpan,
    codegen::{DeclaratorInitializer, ExpressionReplacement, Fixup, expressions::analysis::Lvalue},
    tokenizer::{
        AssignmentOperator, OperatorType as ShaderOp, TokenCursor, TypedToken, TypedTokenFacts,
    },
};

/// Float modulo assignment candidate.
#[derive(Clone, Copy)]
pub(super) struct FloatModulo<'statement, 'src> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'statement TypedTokenFacts,
}

impl FloatModulo<'_, '_> {
    /// Builds one modulo fixup for a float modulo assignment.
    fn build_fixup(self, tokens: TokenCursor<'_>) -> Result<Fixup, ()> {
        let search = tokens;
        let Some(semicolon) = self.statement.semicolon(tokens) else {
            return Err(());
        };
        for index in self.statement.start()..semicolon {
            if !matches!(
                tokens[index].kind(),
                TypedToken::Operator(ShaderOp::Assignment(AssignmentOperator::RemainderAssign,))
            ) {
                continue;
            }
            let lhs_end = search.previous_non_comment(index).ok_or(())?;
            let lhs = Lvalue::ending_at(tokens, lhs_end).ok_or(())?;
            if !self.facts.float_lvalue(&lhs) {
                continue;
            }
            let lhs_span = SourceSpan::new(
                tokens[lhs.start].span().start(),
                tokens[lhs.end].span().end(),
            )
            .map_err(|_error| ())?;
            let rhs_span = self.statement.rhs_span(tokens, index).ok_or(())?;
            let statement_span = SourceSpan::new(
                tokens[lhs.start].span().start(),
                tokens[semicolon].span().end(),
            )
            .map_err(|_error| ())?;
            let rhs = ExpressionReplacement::new().with_source(rhs_span);
            let replacement = ExpressionReplacement::new()
                .with_source(lhs_span)
                .with_text(" = ((")
                .with_source(lhs_span)
                .with_text(") - (")
                .with_replacement(rhs.clone())
                .with_text(") * trunc((")
                .with_source(lhs_span)
                .with_text(") / (")
                .with_replacement(rhs)
                .with_text(")));");
            return Ok(Fixup::replace(statement_span, replacement));
        }
        self.direct_assignment_fixup(tokens)
    }

    /// Emits all float modulo lowering fixups for this statement.
    pub(super) fn lowering_fixups(self, tokens: TokenCursor<'_>) -> Vec<Fixup> {
        if let Some(direct) = self.direct_declaration_fixups(tokens) {
            return direct;
        }
        if let Ok(direct) = self.direct_assignment_fixup(tokens) {
            return vec![direct];
        }
        if let Some(constructor_fixups) = self.constructor_fixups(tokens) {
            return constructor_fixups;
        }
        if let Some(integer_declaration_fixups) = self.integer_declaration_fixups(tokens) {
            return integer_declaration_fixups;
        }
        self.build_fixup(tokens).into_iter().collect()
    }
}

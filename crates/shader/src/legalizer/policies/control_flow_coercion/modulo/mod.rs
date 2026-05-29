/// Operand detection for float modulo lowering.
mod operands;
/// Replacement construction for float modulo lowering.
mod rewrite;
/// Type inference helpers for float modulo lowering.
mod typing;

use self::{
    operands::{ConstructorModulo, DirectFloatModulo, IntegerDeclarationModulo},
    rewrite::{ExpressionSource, ExpressionSourcePart},
};
use super::{
    expr::Lvalue,
    statements::Statement,
    symbols::{SymbolFacts, SymbolType},
};
use crate::{
    SourceSpan,
    legalizer::{
        DeclaratorInitializer, ExpressionReplacement, Fixup, TokenSearch, tokens::BalancedTokens,
    },
    lexer::{Token, TokenKind},
};

/// Float modulo assignment candidate.
#[derive(Clone, Copy)]
pub(super) struct FloatModulo<'statement, 'tokens, 'src> {
    /// Statement being inspected.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Known symbol facts.
    pub(super) facts: &'statement SymbolFacts<'src>,
}
impl TryFrom<FloatModulo<'_, '_, '_>> for Fixup {
    type Error = ();

    fn try_from(input: FloatModulo<'_, '_, '_>) -> Result<Self, Self::Error> {
        let tokens = input.statement.tokens;
        let search = TokenSearch::new(tokens);
        for index in input.statement.start..input.statement.semicolon {
            if !matches!(tokens[index].kind, TokenKind::Punctuation('%')) {
                continue;
            }
            let equals = search.next_non_comment(index + 1).ok_or(())?;
            if matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
                let lhs_end = search.previous_non_comment(index).ok_or(())?;
                let lhs = Lvalue::ending_at(tokens, lhs_end).ok_or(())?;
                if !input.facts.float_lvalue(lhs) {
                    continue;
                }
                let lhs_span =
                    SourceSpan::new(tokens[lhs.start].span.start(), tokens[lhs.end].span.end())
                        .map_err(|_error| ())?;
                let rhs_span = input.statement.rhs_span(equals).ok_or(())?;
                let statement_span = SourceSpan::new(
                    tokens[lhs.start].span.start(),
                    tokens[input.statement.semicolon].span.end(),
                )
                .map_err(|_error| ())?;
                let rhs = ExpressionSource {
                    parts: vec![ExpressionSourcePart::Source(rhs_span)],
                    changed: false,
                };
                let replacement = ExpressionReplacement::new()
                    .with_source(lhs_span)
                    .with_text(" = ((")
                    .with_source(lhs_span)
                    .with_text(") - (")
                    .with_replacement(rhs.clone().replacement())
                    .with_text(") * trunc((")
                    .with_source(lhs_span)
                    .with_text(") / (")
                    .with_replacement(rhs.replacement())
                    .with_text(")));");
                return Ok(Fixup::replace(statement_span, replacement));
            }
        }
        Fixup::try_from(DirectFloatModulo(input))
    }
}
impl FloatModulo<'_, '_, '_> {
    /// Emits all modulo fixups for this statement.
    pub(super) fn fixups(self) -> Vec<Fixup> {
        if let Some(direct) = DirectFloatModulo(self).declaration_fixups() {
            return direct;
        }
        if let Ok(direct) = Fixup::try_from(DirectFloatModulo(self)) {
            return vec![direct];
        }
        if let Some(constructor_fixups) = ConstructorModulo(self).fixups() {
            return constructor_fixups;
        }
        if let Some(integer_declaration_fixups) = IntegerDeclarationModulo(self).fixups() {
            return integer_declaration_fixups;
        }
        Fixup::try_from(self).into_iter().collect()
    }
}

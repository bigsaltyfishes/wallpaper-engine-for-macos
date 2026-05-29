use super::{
    statements::StatementFixupInput,
    symbols::{SymbolFacts, SymbolType},
};
use crate::{
    codegen::{
        ExpressionReplacement, Fixup,
        expressions::analysis::{ScalarExpressionAnalyzer, ScalarExpressionFlavor},
    },
    tokenizer::{TokenCursor, TypedTokenFacts},
};

/// Array subscript whose index expression is float-valued.
pub(super) struct FloatArraySubscriptIndex<'statement, 'src> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'statement TypedTokenFacts,
}

impl FloatArraySubscriptIndex<'_, '_> {
    /// Emits casts for array subscript indices that are float expressions.
    pub(super) fn fixups(self, tokens: TokenCursor<'_>) -> Vec<Fixup> {
        let mut fixups = Vec::new();
        for open in self.statement.start()..self.statement.end() {
            let Some(expression) = tokens.square_bracket_expression(open, self.statement.end())
            else {
                continue;
            };
            let Some(end) = expression.last() else {
                continue;
            };
            let Some(ty) = (ScalarExpressionAnalyzer {
                facts: self.facts,
                token_facts: self.token_facts,
                flavor: ScalarExpressionFlavor::IntInitializer,
            })
            .range_type(tokens, expression.start(), end) else {
                continue;
            };
            if ty != SymbolType::Float {
                continue;
            }
            let Some(span) = tokens.range_span(expression.start(), expression.end()) else {
                continue;
            };
            let replacement = ExpressionReplacement::new()
                .with_text("int(")
                .with_source(span)
                .with_text(")");
            fixups.push(Fixup::replace(span, replacement));
        }
        fixups
    }
}

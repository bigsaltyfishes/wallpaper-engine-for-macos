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

/// Integer variable initialized from an expression whose result is float.
pub(super) struct IntFloatInitializer<'statement, 'src> {
    /// Statement being inspected.
    pub statement: StatementFixupInput<'statement>,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Shared tokenizer declaration facts.
    pub token_facts: &'statement TypedTokenFacts,
}

impl IntFloatInitializer<'_, '_> {
    /// Emits initializer casts for int declarations initialized by
    /// float-valued expressions.
    pub(super) fn fixups(self, tokens: TokenCursor<'_>) -> Vec<Fixup> {
        let declarations = self
            .statement
            .declaration_declarators(tokens, self.token_facts, "int");
        if declarations.is_empty() {
            return Vec::new();
        }

        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = (IntFloatDeclarator {
                declaration,
                facts: self.facts,
                token_facts: self.token_facts,
            })
            .float_initializer(tokens) else {
                continue;
            };
            let replacement = ExpressionReplacement::new()
                .with_text("int(")
                .with_source(initializer.span())
                .with_text(")");
            fixups.push(Fixup::replace(initializer.span(), replacement));
        }
        fixups
    }
}

/// One int declarator candidate.
#[derive(Clone)]
struct IntFloatDeclarator<'facts, 'src> {
    /// Parsed declarator.
    declaration: crate::codegen::LocalDeclaration,
    /// Known symbol facts.
    facts: &'facts SymbolFacts<'src>,
    /// Cached tokenizer facts for call lookups.
    token_facts: &'facts TypedTokenFacts,
}

impl IntFloatDeclarator<'_, '_> {
    /// Returns this declarator's initializer when it is float-valued.
    fn float_initializer(
        self,
        tokens: TokenCursor<'_>,
    ) -> Option<crate::codegen::DeclaratorInitializer> {
        let initializer = self.declaration.initializer(tokens)?;
        ScalarExpressionAnalyzer {
            facts: self.facts,
            token_facts: self.token_facts,
            flavor: ScalarExpressionFlavor::IntInitializer,
        }
        .range_type(tokens, initializer.start(), initializer.end())
        .is_some_and(|ty| ty == SymbolType::Float)
        .then_some(initializer)
    }
}

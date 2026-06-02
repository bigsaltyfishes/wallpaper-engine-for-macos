use super::{
    DeclaratorInitializer, ExpressionReplacement, Fixup, FloatModulo,
    rewrite::{ModuloLowerer, ModuloLoweringMode},
};
use crate::tokenizer::{ArithmeticOperator, OperatorType::Arithmetic, TokenIndexRange};

impl FloatModulo<'_, '_> {
    /// Builds a direct modulo assignment fixup.
    pub(super) fn direct_assignment_fixup(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
    ) -> Result<Fixup, ()> {
        if !self
            .statement
            .declaration_declarators(tokens, self.token_facts, "float")
            .is_empty()
        {
            return Err(());
        }
        let (lhs, equals) = self.statement.lvalue_assignment(tokens).ok_or(())?;
        if !self.facts.float_lvalue(&lhs) {
            return Err(());
        }
        let search = tokens;
        let start = search.next_non_comment(equals + 1).ok_or(())?;
        let end = search
            .previous_non_comment(self.statement.semicolon(tokens).ok_or(())?)
            .ok_or(())?;
        if start > end {
            return Err(());
        }
        let initializer =
            DeclaratorInitializer::from_inclusive_tokens(tokens, start, end).ok_or(())?;
        self.top_level_modulo_fixup(tokens, initializer, ModuloLoweringMode::BuiltinFmod)
    }

    /// Emits direct modulo replacements for each float declaration initializer.
    pub(super) fn direct_declaration_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
    ) -> Option<Vec<Fixup>> {
        let declarations =
            self.statement
                .declaration_declarators(tokens, self.token_facts, "float");
        if declarations.is_empty() {
            return None;
        }
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            let Ok(fixup) =
                self.top_level_modulo_fixup(tokens, initializer, ModuloLoweringMode::BuiltinFmod)
            else {
                continue;
            };
            fixups.push(fixup);
        }
        Some(fixups)
    }

    /// Emits constructor argument modulo replacements in declaration
    /// initializers and assignment right-hand sides.
    pub(super) fn constructor_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
    ) -> Option<Vec<Fixup>> {
        if let Some(fixups) = self.constructor_declaration_fixups(tokens) {
            return Some(fixups);
        }
        let (_, equals) = self.statement.lvalue_assignment(tokens)?;
        let search = tokens;
        let start = search.next_non_comment(equals + 1)?;
        let end = search.previous_non_comment(self.statement.semicolon(tokens)?)?;
        let initializer = DeclaratorInitializer::from_inclusive_tokens(tokens, start, end)?;
        self.constructor_initializer_fixups(tokens, initializer)
    }

    /// Emits constructor argument modulo replacements for local declaration
    /// initializers.
    pub(super) fn constructor_declaration_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
    ) -> Option<Vec<Fixup>> {
        let declarations = self
            .statement
            .local_declaration_declarators(tokens, self.token_facts);
        if declarations.is_empty() {
            return None;
        }
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            if let Some(initializer_fixups) =
                self.constructor_initializer_fixups(tokens, initializer)
            {
                fixups.extend(initializer_fixups);
            }
        }
        (!fixups.is_empty()).then_some(fixups)
    }

    /// Emits constructor argument modulo replacements inside one initializer or
    /// right-hand side expression.
    pub(super) fn constructor_initializer_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
        initializer: DeclaratorInitializer,
    ) -> Option<Vec<Fixup>> {
        let search = tokens;
        let balanced = tokens;
        let mut ranges = Vec::new();
        let mut index = initializer.start();
        while index <= initializer.end() {
            let Some(name_text) = tokens[index].kind().source_text() else {
                index += 1;
                continue;
            };
            if !matches!(name_text, "int" | "uint") {
                index += 1;
                continue;
            }
            let name = index;
            let Some(open) = search.next_non_comment(name + 1) else {
                break;
            };
            if !tokens[open].kind().is_left_paren() {
                index += 1;
                continue;
            }
            let Some(close) = balanced.matching_right_paren(open) else {
                index += 1;
                continue;
            };
            if close > initializer.end() {
                index += 1;
                continue;
            }
            index = close + 1;
            let Some(start) = search.next_non_comment(open + 1) else {
                continue;
            };
            let Some(end) = search.previous_non_comment(close) else {
                continue;
            };
            if start > end {
                continue;
            }
            if let Some(range) = DeclaratorInitializer::from_inclusive_tokens(tokens, start, end) {
                ranges.push(range);
            }
        }
        let fixups: Vec<_> = ranges
            .into_iter()
            .filter_map(|range| {
                let lowered = ModuloLowerer {
                    facts: self.facts,
                    token_facts: self.token_facts,
                    mode: ModuloLoweringMode::BuiltinFmod,
                }
                .lower(
                    tokens,
                    TokenIndexRange::from_inclusive(range.start(), range.end()),
                )
                .ok()?;
                lowered
                    .is_changed()
                    .then(|| Fixup::replace(range.span(), lowered))
            })
            .collect();
        (!fixups.is_empty()).then_some(fixups)
    }

    /// Emits initializer replacements when a float modulo expression feeds an
    /// integer declaration without an explicit constructor.
    pub(super) fn integer_declaration_fixups(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
    ) -> Option<Vec<Fixup>> {
        let declarations = self
            .statement
            .local_declaration_declarators(tokens, self.token_facts);
        if declarations.is_empty() {
            return None;
        }
        let mut fixups = Vec::new();

        for declaration in declarations {
            if !matches!(declaration.ty(), "int" | "uint") {
                continue;
            }
            let Some(initializer) = declaration.initializer(tokens) else {
                continue;
            };
            let modulo_lowering = ModuloLowerer {
                facts: self.facts,
                token_facts: self.token_facts,
                mode: ModuloLoweringMode::NagaCompatible,
            };
            let Ok(rewritten) = modulo_lowering.lower_initializer(tokens, initializer) else {
                continue;
            };
            if !rewritten.is_changed() {
                continue;
            }
            let replacement = ExpressionReplacement::new()
                .with_text(declaration.ty())
                .with_text("(")
                .with_replacement(rewritten)
                .with_text(")");
            fixups.push(Fixup::replace(initializer.span(), replacement));
        }

        (!fixups.is_empty()).then_some(fixups)
    }

    /// Parses a top-level modulo expression for lowering.
    pub(super) fn top_level_modulo_fixup(
        self,
        tokens: crate::tokenizer::TokenCursor<'_>,
        initializer: DeclaratorInitializer,
        mode: ModuloLoweringMode,
    ) -> Result<Fixup, ()> {
        let start = initializer.start();
        let end = initializer.end();
        let has_remainder = self
            .token_facts
            .expressions_contained(TokenIndexRange::from_inclusive(start, end))
            .any(|operator| {
                operator.top_level_operators().iter().any(|operator| {
                    matches!(
                        operator.operator(),
                        Arithmetic(ArithmeticOperator::Remainder)
                    )
                })
            });
        if has_remainder {
            let lowered = ModuloLowerer {
                facts: self.facts,
                token_facts: self.token_facts,
                mode,
            }
            .lower_initializer(tokens, initializer)?;

            return Ok(Fixup::replace(initializer.span(), lowered));
        }

        Err(())
    }
}

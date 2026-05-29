use super::{
    BalancedTokens, DeclaratorInitializer, ExpressionReplacement, Fixup, FloatModulo, SourceSpan,
    Statement, SymbolFacts, TokenKind, TokenSearch,
    rewrite::{ModuloInitializer, ModuloLoweringMode, ModuloRange},
};

/// Float modulo expression on the right side of a direct assignment.
pub(super) struct DirectFloatModulo<'statement, 'tokens, 'src>(
    pub(super) FloatModulo<'statement, 'tokens, 'src>,
);
impl TryFrom<DirectFloatModulo<'_, '_, '_>> for Fixup {
    type Error = ();

    fn try_from(input: DirectFloatModulo<'_, '_, '_>) -> Result<Self, Self::Error> {
        let input = input.0;
        if input.statement.declaration_declarators("float").is_some() {
            return Err(());
        }
        let (lhs, equals) = input.statement.lvalue_assignment().ok_or(())?;
        if !input.facts.float_lvalue(lhs) {
            return Err(());
        }
        let search = TokenSearch::new(input.statement.tokens);
        let start = search.next_non_comment(equals + 1).ok_or(())?;
        let end = search
            .previous_non_comment(input.statement.semicolon)
            .ok_or(())?;
        if start > end {
            return Err(());
        }
        let span = SourceSpan::new(
            input.statement.tokens[start].span.start(),
            input.statement.tokens[end].span.end(),
        )
        .map_err(|_error| ())?;
        let initializer = StatementInitializer { start, end, span };
        let expression = ModuloExpression;
        let split = TopLevelModulo::try_from(TopLevelModuloInput {
            statement: input.statement,
            initializer,
            expression: &expression,
            facts: input.facts,
        })?;
        Ok(split.fixup)
    }
}
impl DirectFloatModulo<'_, '_, '_> {
    /// Emits direct modulo replacements for each float declaration initializer.
    pub(super) fn declaration_fixups(self) -> Option<Vec<Fixup>> {
        let input = self.0;
        let declarations = input.statement.declaration_declarators("float")?;
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(input.statement.tokens) else {
                continue;
            };
            let initializer = initializer.into();
            let expression = ModuloExpression;
            let Ok(split) = TopLevelModulo::try_from(TopLevelModuloInput {
                statement: input.statement,
                initializer,
                expression: &expression,
                facts: input.facts,
            }) else {
                continue;
            };
            fixups.push(split.fixup);
        }
        Some(fixups)
    }
}
/// Modulo expressions inside int/uint constructor arguments.
#[derive(Clone, Copy)]
pub(super) struct ConstructorModulo<'statement, 'tokens, 'src>(
    pub(super) FloatModulo<'statement, 'tokens, 'src>,
);
impl ConstructorModulo<'_, '_, '_> {
    /// Emits constructor argument modulo replacements in declaration
    /// initializers and assignment right-hand sides.
    pub(super) fn fixups(self) -> Option<Vec<Fixup>> {
        let input = self.0;
        if let Some(fixups) = self.declaration_fixups() {
            return Some(fixups);
        }
        let (_, equals) = input.statement.lvalue_assignment()?;
        let search = TokenSearch::new(input.statement.tokens);
        let start = search.next_non_comment(equals + 1)?;
        let end = search.previous_non_comment(input.statement.semicolon)?;
        let span = input.statement.rhs_span(equals)?;
        let initializer = StatementInitializer { start, end, span };
        self.initializer_fixups(initializer)
    }

    /// Emits constructor argument modulo replacements for local declaration
    /// initializers.
    pub(super) fn declaration_fixups(self) -> Option<Vec<Fixup>> {
        let input = self.0;
        let declarations = input.statement.local_declaration_declarators()?;
        let mut fixups = Vec::new();
        for declaration in declarations {
            let Some(initializer) = declaration.initializer(input.statement.tokens) else {
                continue;
            };
            if let Some(initializer_fixups) = self.initializer_fixups(initializer.into()) {
                fixups.extend(initializer_fixups);
            }
        }
        (!fixups.is_empty()).then_some(fixups)
    }

    /// Emits constructor argument modulo replacements inside one initializer or
    /// right-hand side expression.
    pub(super) fn initializer_fixups(
        self,
        initializer: StatementInitializer,
    ) -> Option<Vec<Fixup>> {
        let input = self.0;
        let expression = ModuloExpression;
        let fixups: Vec<_> = ConstructorModuloRanges {
            statement: input.statement,
            initializer,
        }
        .filter_map(|range| {
            let lowered = ModuloRange {
                expression: &expression,
                tokens: input.statement.tokens,
                start: range.start,
                end: range.end,
                facts: input.facts,
                mode: ModuloLoweringMode::BuiltinFmod,
            }
            .lower()
            .ok()?;
            lowered
                .is_changed()
                .then(|| Fixup::replace(range.span, lowered.replacement()))
        })
        .collect();
        (!fixups.is_empty()).then_some(fixups)
    }
}
/// Modulo expressions directly initializing int/uint declarations.
#[derive(Clone, Copy)]
pub(super) struct IntegerDeclarationModulo<'statement, 'tokens, 'src>(
    pub(super) FloatModulo<'statement, 'tokens, 'src>,
);
impl IntegerDeclarationModulo<'_, '_, '_> {
    /// Emits initializer replacements when a float modulo expression feeds an
    /// integer declaration without an explicit constructor.
    pub(super) fn fixups(self) -> Option<Vec<Fixup>> {
        let input = self.0;
        let declarations = input.statement.local_declaration_declarators()?;
        let mut fixups = Vec::new();

        for declaration in declarations {
            if !matches!(declaration.ty(), "int" | "uint") {
                continue;
            }
            let Some(initializer) = declaration.initializer(input.statement.tokens) else {
                continue;
            };
            let initializer = StatementInitializer::from(initializer);
            let expression = ModuloExpression;
            let Ok(lowered) = ModuloInitializer {
                expression: &expression,
                tokens: input.statement.tokens,
                initializer,
                facts: input.facts,
                mode: ModuloLoweringMode::NagaCompatible,
            }
            .lower() else {
                continue;
            };
            if !lowered.is_changed() {
                continue;
            }
            let replacement = ExpressionReplacement::new()
                .with_text(declaration.ty())
                .with_text("(")
                .with_replacement(lowered.replacement())
                .with_text(")");
            fixups.push(Fixup::replace(initializer.span, replacement));
        }

        (!fixups.is_empty()).then_some(fixups)
    }
}
/// Token range for an int/uint constructor argument list.
#[derive(Clone, Copy)]
pub(super) struct ConstructorArgumentRange {
    /// First argument token.
    pub(super) start: usize,
    /// Last argument token.
    pub(super) end: usize,
    /// Source span covering the constructor argument list.
    pub(super) span: SourceSpan,
}
/// Iterator over int/uint constructor argument ranges in an expression.
pub(super) struct ConstructorModuloRanges<'tokens, 'src> {
    /// Statement containing the expression.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Initializer or RHS range to inspect.
    pub(super) initializer: StatementInitializer,
}
impl Iterator for ConstructorModuloRanges<'_, '_> {
    type Item = ConstructorArgumentRange;

    fn next(&mut self) -> Option<Self::Item> {
        let tokens = self.statement.tokens;
        let search = TokenSearch::new(tokens);
        let balanced = BalancedTokens::new(tokens);
        let mut index = self.initializer.start;
        while index <= self.initializer.end {
            let name = if let TokenKind::Identifier("int" | "uint") = tokens[index].kind {
                index
            } else {
                index += 1;
                continue;
            };
            let Some(open) = search.next_non_comment(name + 1) else {
                break;
            };
            if !matches!(tokens[open].kind, TokenKind::LeftParen) {
                index += 1;
                continue;
            }
            let Some(close) = balanced.matching_right_paren(open) else {
                index += 1;
                continue;
            };
            if close > self.initializer.end {
                index += 1;
                continue;
            }
            self.initializer.start = close + 1;
            let Some(start) = search.next_non_comment(open + 1) else {
                continue;
            };
            let Some(end) = search.previous_non_comment(close) else {
                continue;
            };
            if start > end {
                continue;
            }
            let span = SourceSpan::new(tokens[start].span.start(), tokens[end].span.end()).ok()?;
            return Some(ConstructorArgumentRange { start, end, span });
        }
        None
    }
}
/// Initializer range used by direct assignment coercions.
#[derive(Clone, Copy)]
pub(super) struct StatementInitializer {
    /// First non-comment initializer token.
    pub(super) start: usize,
    /// Last non-comment initializer token.
    pub(super) end: usize,
    /// Source span covering the initializer expression.
    pub(super) span: SourceSpan,
}
impl From<DeclaratorInitializer> for StatementInitializer {
    /// Creates an initializer range from a parsed declaration initializer.
    fn from(initializer: DeclaratorInitializer) -> Self {
        Self {
            start: initializer.start(),
            end: initializer.end(),
            span: initializer.span(),
        }
    }
}
/// Token ranges around a modulo expression.
pub(super) struct TopLevelModulo {
    /// Fixup replacing the whole initializer with this modulo lowered.
    pub(super) fixup: Fixup,
}
/// Input used to lower `%` in a statement RHS.
pub(super) struct TopLevelModuloInput<'tokens, 'src> {
    /// Statement containing the expression.
    pub(super) statement: Statement<'tokens, 'src>,
    /// Initializer or RHS range to inspect.
    pub(super) initializer: StatementInitializer,
    /// Current expression text for this initializer.
    pub(super) expression: &'tokens ModuloExpression,
    /// Known symbol facts.
    pub(super) facts: &'tokens SymbolFacts<'src>,
}
impl TryFrom<TopLevelModuloInput<'_, '_>> for TopLevelModulo {
    type Error = ();

    fn try_from(input: TopLevelModuloInput<'_, '_>) -> Result<Self, Self::Error> {
        let tokens = input.statement.tokens;
        let search = TokenSearch::new(tokens);
        let start = input.initializer.start;
        let end = input.initializer.end;
        for (index, token) in tokens.iter().enumerate().take(end + 1).skip(start) {
            if !matches!(token.kind, TokenKind::Punctuation('%')) {
                continue;
            }
            if matches!(
                search
                    .next_non_comment(index + 1)
                    .map(|next| tokens[next].kind),
                Some(TokenKind::Punctuation('='))
            ) {
                continue;
            }
            let lowered = ModuloInitializer {
                expression: input.expression,
                tokens,
                initializer: input.initializer,
                facts: input.facts,
                mode: ModuloLoweringMode::BuiltinFmod,
            }
            .lower()?;
            let fixup = Fixup::replace(input.initializer.span, lowered.replacement());

            return Ok(Self { fixup });
        }

        Err(())
    }
}
/// Marker for token-backed modulo lowering.
pub(super) struct ModuloExpression;

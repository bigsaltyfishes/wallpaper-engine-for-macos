use crate::{
    SourceSpan,
    codegen::{LocalDeclaration, expressions::analysis::Lvalue},
    tokenizer::{StatementFact, TokenCursor, TypedToken, TypedTokenFacts},
};

/// One token-backed statement ending at a semicolon.
#[derive(Clone, Copy)]
pub(super) struct StatementFixupInput<'facts> {
    /// Tokenizer statement fact.
    pub fact: &'facts StatementFact,
}

impl StatementFixupInput<'_> {
    /// Returns the first token in the statement.
    pub(super) fn start(self) -> usize {
        self.fact.range().start()
    }

    /// Returns the first token outside the statement.
    pub(super) fn end(self) -> usize {
        self.fact.range().end()
    }

    /// Returns the semicolon token for semicolon-terminated statements.
    pub(super) fn semicolon(self, tokens: TokenCursor<'_>) -> Option<usize> {
        let index = self.end().checked_sub(1)?;
        matches!(tokens[index].kind(), TypedToken::Semicolon).then_some(index)
    }

    /// Returns the expression span between `=` and this statement's semicolon.
    pub(super) fn rhs_span(self, tokens: TokenCursor<'_>, equals: usize) -> Option<SourceSpan> {
        let search = tokens;
        let start = search.next_non_comment(equals + 1)?;
        let end = search.previous_non_comment(self.semicolon(tokens)?)?;
        SourceSpan::new(tokens[start].span().start(), tokens[end].span().end()).ok()
    }

    /// Returns the `=` token for a simple lvalue assignment statement.
    pub(super) fn lvalue_assignment(self, tokens: TokenCursor<'_>) -> Option<(Lvalue, usize)> {
        let search = tokens;
        let semicolon = self.semicolon(tokens)?;
        let equals = (self.start()..semicolon).find(|index| {
            if !tokens[*index].kind().is_simple_assignment_operator() {
                return false;
            }
            let previous = search.previous_non_comment(*index);
            let next = search.next_non_comment(*index + 1);
            !matches!(
                previous.map(|previous| tokens[previous].kind()),
                Some(TypedToken::Operator(operator))
                    if operator.is_assignment() || operator.is_comparison()
            ) && !matches!(
                next.map(|next| tokens[next].kind()),
                Some(kind) if kind.is_simple_assignment_operator()
            )
        })?;
        let lhs_end = search.previous_non_comment(equals)?;
        let lhs = Lvalue::ending_at(tokens, lhs_end)?;
        (lhs.start == self.start()).then_some((lhs, equals))
    }

    /// Returns declarators when this statement starts with a local declaration
    /// of `ty`.
    pub(super) fn declaration_declarators(
        self,
        tokens: TokenCursor<'_>,
        facts: &TypedTokenFacts,
        ty: &str,
    ) -> Vec<LocalDeclaration> {
        self.local_declarations(tokens, facts)
            .into_iter()
            .filter(|declaration| declaration.ty() == ty)
            .collect()
    }

    /// Returns declarators when this statement starts with a local declaration.
    pub(super) fn local_declaration_declarators(
        self,
        tokens: TokenCursor<'_>,
        facts: &TypedTokenFacts,
    ) -> Vec<LocalDeclaration> {
        self.local_declarations(tokens, facts)
    }

    /// Returns local declarations at this statement start.
    fn local_declarations(
        self,
        tokens: TokenCursor<'_>,
        facts: &TypedTokenFacts,
    ) -> Vec<LocalDeclaration> {
        facts
            .declarations_at_statement_start(self.start())
            .iter()
            .filter_map(|fact| LocalDeclaration::from_declaration_fact(tokens, fact))
            .collect()
    }
}

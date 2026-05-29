use super::{
    ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, SourceSpan,
    TypedToken,
};
use crate::{
    syntax::ShaderModule,
    tokenizer::{AccessOperator, OperatorType::Access, TokenCursor, TokenIndexRange},
};

/// Finds body-local uses of a removed array parameter.
#[derive(Clone, Copy)]
pub(super) struct ArrayParameterUseScanner {
    /// Function-body token range.
    pub body: TokenIndexRange,
}

impl ArrayParameterUseScanner {
    /// Collects identifier spans that still refer to the removed parameter.
    pub(super) fn use_spans(
        self,
        module: &ShaderModule<'_>,
        tokens: TokenCursor<'_>,
        name: &str,
    ) -> Vec<SourceSpan> {
        let shadows = self.shadowed_scopes(module, name);
        let mut spans = Vec::new();
        for index in self.body.start()..self.body.end() {
            if shadows
                .iter()
                .any(|shadow| index >= shadow.start() && index < shadow.end())
            {
                continue;
            }
            if matches!(tokens[index].kind(), TypedToken::Identifier(text) if text == name)
                && !tokens.previous_non_comment(index).is_some_and(|previous| {
                    matches!(
                        tokens[previous].kind(),
                        TypedToken::Operator(Access(AccessOperator::Member))
                    )
                })
            {
                spans.push(tokens[index].span());
            }
        }
        spans
    }

    /// Returns token ranges where local declarations own the same name.
    fn shadowed_scopes(self, module: &ShaderModule<'_>, name: &str) -> Vec<TokenIndexRange> {
        ScopedDeclarationFacts::collect(
            module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        )
        .declarations()
        .iter()
        .filter(|declaration| {
            declaration.name() == name
                && declaration.visible_start() > self.body.start() + 1
                && declaration.visible_start() < self.body.end()
        })
        .map(|declaration| TokenIndexRange {
            start: declaration.visible_start().saturating_sub(1),
            end: declaration.scope_end().min(self.body.end()),
        })
        .collect()
    }
}

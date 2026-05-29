use super::{
    ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, SourceSpan,
    Token, TokenKind,
    calls::MemberFieldIdentifier,
    specialization::{TokenSpanRange, TokenSpanRangeSource},
};

/// Scope-aware identifier uses of one removed array parameter.
pub(super) struct ArrayParameterUses<'tokens, 'src> {
    /// Tokens being scanned.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Identifier name to match.
    pub(super) name: &'src str,
    /// Inclusive body source start.
    pub(super) start: usize,
    /// Exclusive body source end.
    pub(super) end: usize,
}
impl ArrayParameterUses<'_, '_> {
    /// Collects identifier spans that still refer to the removed parameter.
    pub(super) fn spans(self) -> Vec<SourceSpan> {
        let Some(range) = TokenSpanRangeSource {
            tokens: self.tokens,
            start: self.start,
            end: self.end,
        }
        .range() else {
            return Vec::new();
        };
        let shadows = ShadowedArrayParameterScopes {
            tokens: self.tokens,
            name: self.name,
            range,
        }
        .ranges();
        let mut spans = Vec::new();
        for index in range.start..range.end {
            if shadows
                .iter()
                .any(|shadow| index >= shadow.start && index < shadow.end)
            {
                continue;
            }
            if matches!(self.tokens[index].kind, TokenKind::Identifier(text) if text == self.name)
                && !bool::from(MemberFieldIdentifier {
                    tokens: self.tokens,
                    index,
                })
            {
                spans.push(self.tokens[index].span);
            }
        }
        spans
    }
}
/// Local scopes that shadow a removed array parameter.
pub(super) struct ShadowedArrayParameterScopes<'tokens, 'src> {
    /// Full token stream.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Removed array parameter name.
    pub(super) name: &'src str,
    /// Function body token range.
    pub(super) range: TokenSpanRange,
}
impl ShadowedArrayParameterScopes<'_, '_> {
    /// Returns token ranges where local declarations own the same name.
    pub(super) fn ranges(self) -> Vec<TokenSpanRange> {
        ScopedDeclarationFacts::from_tokens(
            self.tokens,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::Any,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        )
        .declarations()
        .iter()
        .filter(|declaration| {
            declaration.name() == self.name
                && declaration.visible_start() > self.range.start + 1
                && declaration.visible_start() < self.range.end
        })
        .map(|declaration| TokenSpanRange {
            start: declaration.visible_start().saturating_sub(1),
            end: declaration.scope_end().min(self.range.end),
        })
        .collect()
    }
}

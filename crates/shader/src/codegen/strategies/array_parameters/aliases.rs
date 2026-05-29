use smol_str::SmolStr;

use super::{
    ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, SourceSpan,
    TypedToken,
};
use crate::{
    codegen::{Fixup, LocalDeclaration},
    syntax::ShaderModule,
    tokenizer::{AccessOperator, OperatorType::Access, TokenIndexRange},
};

/// Local fixed-array aliases initialized from another fixed array.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ArrayAliases {
    /// Alias plans in source order.
    pub items: Vec<ArrayAlias>,
}

impl ArrayAliases {
    /// Collects local array alias declarations that can be erased safely.
    pub(super) fn collect(&mut self, module: &ShaderModule<'_>) {
        let tokens = module.token_stream().cursor();
        for fact in module.token_facts().declarations() {
            let Some(local) = LocalDeclaration::from_declaration_fact(tokens, fact) else {
                continue;
            };
            if local.array_suffix(tokens).is_none() {
                continue;
            }
            let Some(initializer) = local.initializer(tokens) else {
                continue;
            };
            if initializer.start() != initializer.end() {
                continue;
            }
            let TypedToken::Identifier(target) = tokens[initializer.start()].kind() else {
                continue;
            };
            if !module.has_top_level_array_declaration(target.as_str()) {
                continue;
            }
            let Some(declaration) = local.statement_span(tokens) else {
                continue;
            };
            self.items.push(ArrayAlias {
                declaration,
                name: SmolStr::new(local.name()),
                target: target.clone(),
                visible: TokenIndexRange::new(local.name_index() + 1, local.scope_end()),
            });
        }
    }
}

/// One erasable array alias declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ArrayAlias {
    /// Alias declaration statement span.
    declaration: SourceSpan,
    /// Alias identifier.
    name: SmolStr,
    /// Replacement fixed-array identifier.
    target: SmolStr,
    /// Token range where alias uses remain visible.
    visible: TokenIndexRange,
}

impl ArrayAlias {
    /// Emits declaration removal and alias-use replacements.
    pub(super) fn emit(&self, module: &ShaderModule<'_>, fixups: &mut Vec<Fixup>) {
        fixups.push(Fixup::replace(self.declaration, ""));
        for span in self.use_spans(module) {
            fixups.push(Fixup::replace(span, self.target.as_str()));
        }
    }

    /// Collects identifier spans that still refer to the alias.
    fn use_spans(&self, module: &ShaderModule<'_>) -> Vec<SourceSpan> {
        let tokens = module.token_stream().cursor();
        let shadows = self.shadowed_scopes(module);
        let mut spans = Vec::new();
        for index in self.visible.start()..self.visible.end() {
            if shadows
                .iter()
                .any(|shadow| index >= shadow.start() && index < shadow.end())
            {
                continue;
            }
            if matches!(tokens[index].kind(), TypedToken::Identifier(text) if text == &self.name)
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

    /// Returns token ranges where nested declarations shadow the alias.
    fn shadowed_scopes(&self, module: &ShaderModule<'_>) -> Vec<TokenIndexRange> {
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
            declaration.name() == self.name.as_str()
                && declaration.visible_start() > self.visible.start()
                && declaration.visible_start() < self.visible.end()
        })
        .map(|declaration| TokenIndexRange {
            start: declaration.visible_start().saturating_sub(1),
            end: declaration.scope_end().min(self.visible.end()),
        })
        .collect()
    }
}

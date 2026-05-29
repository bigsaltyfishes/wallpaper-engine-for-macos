use super::{
    BTreeSet, DeclarationDeclarators, Fixup, LocalDeclaration, LocalDeclarationStart,
    LocalTypeName, SourceSpan, StageInterfaceNames, SyntaxItem, Token, TokenKind,
};

/// Local identifier collisions collected from one shader module.
pub(super) struct LocalCollisionSet<'src> {
    /// Colliding local declarations in source order.
    pub(super) items: Vec<LocalIdentifierCollision<'src>>,
}
impl<'src> From<&mut crate::legalizer::LegalizationContext<'_, 'src>> for LocalCollisionSet<'src> {
    fn from(context: &mut crate::legalizer::LegalizationContext<'_, 'src>) -> Self {
        let stage_names = StageInterfaceNames::from(context.module).collect();
        let mut items = Vec::new();
        for function in context.module.items().iter().filter_map(|item| match item {
            SyntaxItem::Function(function) => Some(function),
            _ => None,
        }) {
            let tokens = FunctionBodyTokens::from(FunctionBodySource {
                tokens: context.module.tokens(),
                span: function.body_span(),
            });
            items.extend(
                FunctionLocalRenames {
                    tokens,
                    declared: stage_names.clone(),
                    _declared: std::marker::PhantomData,
                    scopes: vec![LocalScope::default()],
                    index: 0,
                    items: Vec::new(),
                }
                .collect(),
            );
        }
        Self { items }
    }
}
impl<'src> IntoIterator for LocalCollisionSet<'src> {
    type Item = LocalIdentifierCollision<'src>;
    type IntoIter = std::vec::IntoIter<LocalIdentifierCollision<'src>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
#[derive(Clone)]
/// Source data for one colliding local declaration.
pub(super) struct LocalCollisionSource<'module, 'src> {
    /// Local declaration that collides.
    pub(super) declaration: LocalDeclaration<'src>,
    /// Function body tokens containing the declaration and later uses.
    pub(super) tokens: FunctionBodyTokens<'module, 'src>,
    /// Replacement local name.
    pub(super) replacement: String,
}
/// A local declaration rename and its later identifier-use spans.
pub(super) struct LocalIdentifierCollision<'src> {
    /// Colliding declaration name.
    pub(super) declaration: LocalDeclaration<'src>,
    /// Replacement local name.
    pub(super) replacement: String,
    /// Identifier uses after the declaration.
    pub(super) uses: Vec<SourceSpan>,
}
impl<'src> From<LocalCollisionSource<'_, 'src>> for LocalIdentifierCollision<'src> {
    fn from(source: LocalCollisionSource<'_, 'src>) -> Self {
        let uses = LocalUseSpans {
            tokens: source.tokens.tokens(),
            declaration: source.declaration,
        }
        .collect();
        Self {
            declaration: source.declaration,
            replacement: source.replacement,
            uses,
        }
    }
}
impl LocalIdentifierCollision<'_> {
    /// Emits fixups for the declaration and subsequent local uses.
    pub(super) fn emit(self, context: &mut crate::legalizer::LegalizationContext<'_, '_>) {
        context.fixups.push(Fixup::replace(
            self.declaration.name_span(),
            self.replacement.as_str(),
        ));
        for span in self.uses {
            context
                .fixups
                .push(Fixup::replace(span, self.replacement.as_str()));
        }
    }
}
/// Identifier uses owned by one local declaration.
pub(super) struct LocalUseSpans<'tokens, 'src> {
    /// Function body tokens.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Declaration whose uses are collected.
    pub(super) declaration: LocalDeclaration<'src>,
}
impl LocalUseSpans<'_, '_> {
    /// Collects later uses, excluding nested declarations that shadow the same
    /// name.
    pub(super) fn collect(self) -> Vec<SourceSpan> {
        let mut spans = Vec::new();
        let mut index = self.declaration.declarator_end();
        while index < self.declaration.scope_end() {
            if let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                tokens: self.tokens,
                start: index,
            }) && declaration.name() == self.declaration.name()
            {
                index = declaration.scope_end();
                continue;
            }

            if matches!(
                self.tokens[index].kind,
                TokenKind::Identifier(text) if text == self.declaration.name()
            ) && !(IdentifierUse {
                tokens: self.tokens,
                index,
            })
            .member_field()
            {
                spans.push(self.tokens[index].span);
            }
            index += 1;
        }
        spans
    }
}
/// Candidate local identifier use.
pub(super) struct IdentifierUse<'tokens, 'src> {
    /// Function body tokens.
    pub(super) tokens: &'tokens [Token<'src>],
    /// Identifier token index.
    pub(super) index: usize,
}
impl IdentifierUse<'_, '_> {
    /// Returns whether this identifier is the field side of a member access.
    pub(super) fn member_field(self) -> bool {
        let Some(previous) =
            crate::legalizer::TokenSearch::new(self.tokens).previous_non_comment(self.index)
        else {
            return false;
        };
        matches!(self.tokens[previous].kind, TokenKind::Punctuation('.'))
    }
}
#[derive(Clone, Copy)]
/// Source for one function-body token range.
pub(super) struct FunctionBodySource<'module, 'src> {
    /// Full token stream.
    pub(super) tokens: &'module [Token<'src>],
    /// Function body source span.
    pub(super) span: SourceSpan,
}
/// Token range for a function body.
#[derive(Clone, Copy)]
pub(super) struct FunctionBodyTokens<'module, 'src> {
    /// Tokens contained by the body span.
    pub(super) tokens: &'module [Token<'src>],
}
impl<'module, 'src> From<FunctionBodySource<'module, 'src>> for FunctionBodyTokens<'module, 'src> {
    fn from(source: FunctionBodySource<'module, 'src>) -> Self {
        let start = source
            .tokens
            .iter()
            .position(|token| token.span.start() >= source.span.start())
            .unwrap_or(source.tokens.len());
        let end = source.tokens[start..]
            .iter()
            .position(|token| token.span.end() > source.span.end())
            .map_or(source.tokens.len(), |index| start + index);
        Self {
            tokens: &source.tokens[start..end],
        }
    }
}
impl<'module, 'src> FunctionBodyTokens<'module, 'src> {
    /// Returns function body tokens.
    pub(super) const fn tokens(self) -> &'module [Token<'src>] {
        self.tokens
    }
}
/// Scope-aware local rename collector for one function body.
pub(super) struct FunctionLocalRenames<'tokens, 'declared, 'src> {
    /// Function body tokens.
    pub(super) tokens: FunctionBodyTokens<'tokens, 'src>,
    /// Names already declared in surrounding shader scopes.
    pub(super) declared: BTreeSet<String>,
    /// Ties the collector to the source lifetime used by declaration facts.
    pub(super) _declared: std::marker::PhantomData<&'declared ()>,
    /// Lexical scope stack.
    pub(super) scopes: Vec<LocalScope<'src>>,
    /// Next token index to inspect.
    pub(super) index: usize,
    /// Collected collisions.
    pub(super) items: Vec<LocalIdentifierCollision<'src>>,
}
impl<'src> FunctionLocalRenames<'_, '_, 'src> {
    /// Collects scope-aware local rename collisions.
    pub(super) fn collect(mut self) -> Vec<LocalIdentifierCollision<'src>> {
        let tokens = self.tokens.tokens();
        while self.index < tokens.len() {
            match tokens[self.index].kind {
                TokenKind::LeftBrace => {
                    self.scopes.push(LocalScope::default());
                    self.index += 1;
                }
                TokenKind::RightBrace => {
                    if self.scopes.len() > 1 {
                        let _ = self.scopes.pop();
                    }
                    self.index += 1;
                }
                _ => {
                    if let Ok(declaration) = LocalDeclaration::try_from(LocalDeclarationStart {
                        tokens,
                        start: self.index,
                    }) {
                        let tail_start = declaration.tail_start();
                        for declaration in DeclarationDeclarators::new(tokens, declaration) {
                            self.collect_declaration(declaration);
                        }
                        self.index = tail_start;
                    } else {
                        self.index += 1;
                    }
                }
            }
        }
        self.items
    }

    /// Records a declaration and emits a collision when its visible name is
    /// reserved.
    pub(super) fn collect_declaration(&mut self, declaration: LocalDeclaration<'src>) {
        if !RenamableLocalType::from(declaration.ty()).renamed() {
            return;
        }
        let needs_rename = self.name_is_visible(declaration.name())
            || ReservedLocalName::from(declaration.name()).is_reserved();
        let replacement = needs_rename.then(|| self.replacement_for(declaration.name()));
        if let Some(replacement) = replacement {
            self.items
                .push(LocalIdentifierCollision::from(LocalCollisionSource {
                    declaration,
                    tokens: self.tokens,
                    replacement: replacement.clone(),
                }));
            self.current().bindings.push(LocalBinding {
                name: declaration.name(),
                visible_name: replacement.clone(),
            });
            let _ = self.declared.insert(replacement);
        } else {
            self.current().bindings.push(LocalBinding {
                name: declaration.name(),
                visible_name: declaration.name().to_owned(),
            });
            let _ = self.declared.insert(declaration.name().to_owned());
        }
    }

    /// Returns whether `name` is visible in an active local or stage scope.
    pub(super) fn name_is_visible(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| {
            scope
                .bindings
                .iter()
                .rev()
                .any(|binding| binding.visible_name == name || binding.name == name)
        }) || self.declared.contains(name)
    }

    /// Builds a deterministic replacement that avoids active visible names.
    pub(super) fn replacement_for(&self, name: &str) -> String {
        let base = format!("{name}_local");
        if !self.name_is_visible(&base) {
            return base;
        }
        for suffix in 1usize..=self.scopes.len() + self.declared.len() + 1 {
            let candidate = format!("{base}_{suffix}");
            if !self.name_is_visible(&candidate) {
                return candidate;
            }
        }
        format!("{base}_{}", self.scopes.len() + self.declared.len() + 2)
    }

    /// Returns current innermost scope.
    pub(super) fn current(&mut self) -> &mut LocalScope<'src> {
        self.scopes.last_mut().expect("root local scope exists")
    }
}
/// Local declarations for one lexical scope.
#[derive(Default)]
pub(super) struct LocalScope<'src> {
    /// Bindings declared in this scope.
    pub(super) bindings: Vec<LocalBinding<'src>>,
}
/// One local binding and the name visible after legalization.
pub(super) struct LocalBinding<'src> {
    /// Source declaration name.
    pub(super) name: &'src str,
    /// Replacement or original visible name.
    pub(super) visible_name: String,
}
#[derive(Clone, Copy)]
/// Local declaration type covered by the reserved local rename policy.
pub(super) struct RenamableLocalType<'src> {
    /// Source spelling.
    pub(super) name: &'src str,
}
impl<'src> From<&'src str> for RenamableLocalType<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}
impl RenamableLocalType<'_> {
    /// Returns whether this type participates in local renaming.
    pub(super) fn renamed(self) -> bool {
        LocalTypeName::from(self.name).is_local()
    }
}
#[derive(Clone, Copy)]
/// GLSL keyword-like local name.
pub(super) struct ReservedLocalName<'src> {
    /// Source spelling.
    pub(super) name: &'src str,
}
impl<'src> From<&'src str> for ReservedLocalName<'src> {
    fn from(name: &'src str) -> Self {
        Self { name }
    }
}
impl ReservedLocalName<'_> {
    /// Returns whether this local name collides with GLSL operator words.
    pub(super) fn is_reserved(self) -> bool {
        matches!(self.name, "and" | "or" | "sample" | "xor" | "not")
    }
}

use smol_str::SmolStr;

use super::{Fixup, LegacyTypeName, LocalDeclaration, SourceSpan, TypedToken};
use crate::tokenizer::{AccessOperator, OperatorType::Access, TokenCursor, TypedTokenFacts};

/// A local declaration rename and its later identifier-use spans.
pub(super) struct LocalIdentifierCollision {
    /// Colliding declaration name.
    pub declaration: LocalDeclaration,
    /// Replacement local name.
    pub replacement: String,
    /// Identifier uses after the declaration.
    pub uses: Vec<SourceSpan>,
}

impl LocalIdentifierCollision {
    /// Emits fixups for the declaration and subsequent local uses.
    pub(super) fn emit(self, context: &mut crate::codegen::CodegenContext<'_, '_>) {
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
/// Scope-aware local rename collector for one function body.
pub(super) struct FunctionLocalRenames<'declared> {
    /// Names already declared in surrounding shader scopes.
    pub declared: Vec<SmolStr>,
    /// Ties the collector to the source lifetime used by declaration facts.
    pub _declared: std::marker::PhantomData<&'declared ()>,
    /// Lexical scope stack.
    pub scopes: Vec<LocalScope>,
    /// Next token index to inspect.
    pub index: usize,
    /// First token outside this function body.
    pub end: usize,
    /// Collected collisions.
    pub items: Vec<LocalIdentifierCollision>,
}

impl FunctionLocalRenames<'_> {
    /// Collects scope-aware local rename collisions.
    pub(super) fn collect(
        mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
    ) -> Vec<LocalIdentifierCollision> {
        while self.index < self.end {
            match tokens[self.index].kind() {
                TypedToken::LeftBrace => {
                    self.scopes.push(LocalScope::default());
                    self.index += 1;
                }
                TypedToken::RightBrace => {
                    if self.scopes.len() > 1 {
                        let _ = self.scopes.pop();
                    }
                    self.index += 1;
                }
                _ => {
                    let declarations: Vec<_> = token_facts
                        .declarations_at_statement_start(self.index)
                        .iter()
                        .filter(|fact| LegacyTypeName::new(fact.ty().as_str()).is_local())
                        .filter_map(|fact| LocalDeclaration::from_declaration_fact(tokens, fact))
                        .collect();
                    if let Some(first) = declarations.first() {
                        let tail_start = first.tail_start();
                        for declaration in declarations {
                            self.collect_declaration(tokens, token_facts, declaration);
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
    pub(super) fn collect_declaration(
        &mut self,
        tokens: TokenCursor<'_>,
        token_facts: &TypedTokenFacts,
        declaration: LocalDeclaration,
    ) {
        if !LegacyTypeName::new(declaration.ty()).is_local() {
            return;
        }
        let needs_rename = self.name_is_visible(declaration.name())
            || ReservedLocalName::NAMES.contains(&declaration.name());
        let replacement = needs_rename.then(|| self.replacement_for(declaration.name()));
        if let Some(replacement) = replacement {
            let mut uses = Vec::new();
            let mut index = declaration.declarator_end();
            while index < declaration.scope_end() {
                if let Some(fact) = token_facts.declaration_at_name(index)
                    && LegacyTypeName::new(fact.ty().as_str()).is_local()
                    && let Some(shadow) = LocalDeclaration::from_declaration_fact(tokens, fact)
                    && shadow.name() == declaration.name()
                {
                    index = shadow.scope_end();
                    continue;
                }

                let member_field = tokens.previous_non_comment(index).is_some_and(|previous| {
                    matches!(
                        tokens[previous].kind(),
                        TypedToken::Operator(Access(AccessOperator::Member))
                    )
                });
                if matches!(
                    tokens[index].kind(),
                    TypedToken::Identifier(text) if text == declaration.name()
                ) && !member_field
                {
                    uses.push(tokens[index].span());
                }
                index += 1;
            }
            let declaration_name = SmolStr::new(declaration.name());
            self.items.push(LocalIdentifierCollision {
                declaration,
                replacement: replacement.clone(),
                uses,
            });
            self.current().bindings.push(LocalBinding {
                name: declaration_name,
                visible_name: SmolStr::new(replacement.as_str()),
            });
            self.record_declared_name(replacement.as_str());
        } else {
            self.current().bindings.push(LocalBinding {
                name: SmolStr::new(declaration.name()),
                visible_name: SmolStr::new(declaration.name()),
            });
            self.record_declared_name(declaration.name());
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
        }) || self.declared_name_is_visible(name)
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
    pub(super) fn current(&mut self) -> &mut LocalScope {
        self.scopes.last_mut().expect("root local scope exists")
    }

    /// Returns whether `name` is visible in a surrounding shader scope.
    pub(super) fn declared_name_is_visible(&self, name: &str) -> bool {
        self.declared.iter().any(|declared| declared == name)
    }

    /// Records a visible declaration name once, preserving set-like
    /// cardinality.
    pub(super) fn record_declared_name(&mut self, name: &str) {
        if !self.declared_name_is_visible(name) {
            self.declared.push(SmolStr::new(name));
        }
    }
}
/// Local declarations for one lexical scope.
#[derive(Default)]
pub(super) struct LocalScope {
    /// Bindings declared in this scope.
    pub bindings: Vec<LocalBinding>,
}
/// One local binding and the name visible after codegen.
pub(super) struct LocalBinding {
    /// Source declaration name.
    pub name: SmolStr,
    /// Replacement or original visible name.
    pub visible_name: SmolStr,
}
/// GLSL keyword-like local name predicates.
pub(super) struct ReservedLocalName;

impl ReservedLocalName {
    /// Names that collide with GLSL operator words.
    pub(super) const NAMES: &'static [&'static str] = &["and", "or", "sample", "xor", "not"];
}

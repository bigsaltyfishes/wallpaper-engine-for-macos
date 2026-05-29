use super::{
    BTreeSet, ClassifiedModCollision, Fixup, FunctionCall, FunctionCallIndex, ModCall,
    ModCollisionClass, PolicyContext, ScalarTypeFacts, ShaderModule, SyntaxItem,
};

/// Top-level stage interface names.
pub(super) struct StageInterfaceNames<'module, 'src> {
    /// Parsed shader module.
    pub(super) module: &'module ShaderModule<'src>,
}
impl<'module, 'src> From<&'module ShaderModule<'src>> for StageInterfaceNames<'module, 'src> {
    fn from(module: &'module ShaderModule<'src>) -> Self {
        Self { module }
    }
}
impl StageInterfaceNames<'_, '_> {
    /// Collects interface names.
    pub(super) fn collect(self) -> BTreeSet<String> {
        self.module
            .items()
            .iter()
            .filter_map(|item| match item {
                SyntaxItem::Declaration(declaration)
                    if declaration.qualifier().is_some()
                        && matches!(
                            declaration.qualifier(),
                            Some(
                                crate::syntax::TopLevelQualifier::Attribute
                                    | crate::syntax::TopLevelQualifier::In
                                    | crate::syntax::TopLevelQualifier::Out
                                    | crate::syntax::TopLevelQualifier::Uniform
                                    | crate::syntax::TopLevelQualifier::Varying
                            )
                        ) =>
                {
                    declaration.name()
                }
                _ => None,
            })
            .map(str::to_owned)
            .collect()
    }
}
/// User function collision rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct UserFunctionCollision {
    /// Source function name.
    pub(super) source: &'static str,
    /// Replacement function name.
    pub(super) replacement: &'static str,
}
impl UserFunctionCollision {
    /// Applies this collision rule when the source declares the function.
    pub(super) fn emit(self, context: &mut PolicyContext<'_, '_, '_>) {
        let context = context.context();
        if !context.declarations.has_user_function(self.source) {
            return;
        }

        let mod_class = (self.source == "mod")
            .then(|| {
                ModCollisionClass {
                    module: context.module,
                    fallback_functions: context
                        .declarations
                        .user_functions(self.source)
                        .copied()
                        .collect(),
                }
                .classify()
            })
            .map(|collision| ClassifiedModCollision {
                collision,
                scalar_facts: ScalarTypeFacts::from(context.module.tokens()),
            });

        let function_spans = mod_class
            .as_ref()
            .map(|mod_class| mod_class.collision.name_spans.clone());

        let calls = FunctionCallIndex::new(context.module.tokens());
        for call in calls.iter() {
            if self.renames_call(call, mod_class.as_ref()) {
                context
                    .fixups
                    .push(Fixup::replace(call.name_span(), self.replacement));
            }
        }

        let function_spans = if let Some(function_spans) = function_spans {
            function_spans
        } else {
            context
                .declarations
                .user_functions(self.source)
                .map(|function| function.name_span)
                .collect::<Vec<_>>()
        };
        for span in function_spans {
            context.fixups.push(Fixup::replace(span, self.replacement));
        }
    }

    /// Returns whether this syntactic call belongs to the user-defined
    /// collision class.
    pub(super) fn renames_call(
        self,
        call: FunctionCall<'_, '_>,
        mod_class: Option<&ClassifiedModCollision<'_>>,
    ) -> bool {
        if call.name() != self.source {
            return false;
        }
        if self.source != "mod" {
            return true;
        }
        let Some(mod_class) = mod_class else {
            return false;
        };
        match call.argument_count() {
            1 => mod_class.collision.has_unary,
            2 => {
                mod_class.collision.has_scalar_binary
                    && ModCall::from(call).scalar(&mod_class.scalar_facts)
            }
            _ => false,
        }
    }
}

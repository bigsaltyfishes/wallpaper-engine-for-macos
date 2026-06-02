use super::{
    ClassifiedModCollision, Fixup, FunctionCall, ModCollisionClass, ScalarTypeFacts,
    ScopedDeclarationFacts, ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
    StrategyContext,
    scalar::{ScalarBinding, ScalarValueType},
};
use crate::{
    codegen::expressions::analysis::{ScalarExpressionAnalyzer, ScalarExpressionFlavor},
    tokenizer::TokenCursor,
};

/// User function collision rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct UserFunctionCollision {
    /// Source function name.
    pub source: &'static str,
    /// Replacement function name.
    pub replacement: &'static str,
}

impl UserFunctionCollision {
    /// Applies this collision rule when the source declares the function.
    pub(super) fn emit(self, context: &mut StrategyContext<'_, '_, '_>) {
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
                scalar_facts: {
                    let facts = ScopedDeclarationFacts::collect(
                        context.module,
                        ScopedDeclarationFactsConfig {
                            parameter_types: ScopedDeclarationTypeMode::Any,
                            local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
                        },
                    );
                    ScalarTypeFacts {
                        bindings: facts
                            .declarations()
                            .iter()
                            .map(|declaration| ScalarBinding {
                                name: declaration.name().into(),
                                ty: match declaration.ty() {
                                    "bool" | "int" | "uint" | "float" | "float1" => {
                                        ScalarValueType::Scalar
                                    }
                                    "float2" | "float3" | "float4" | "vec2" | "vec3" | "vec4"
                                    | "ivec2" | "ivec3" | "ivec4" | "uvec2" | "uvec3" | "uvec4"
                                    | "bvec2" | "bvec3" | "bvec4" | "mat2" | "mat3" | "mat4"
                                    | "mat2x2" | "mat2x3" | "mat2x4" | "mat3x2" | "mat3x3"
                                    | "mat3x4" | "mat4x2" | "mat4x3" | "mat4x4" => {
                                        ScalarValueType::NonScalar
                                    }
                                    ty if facts.struct_names().iter().any(|name| name == ty) => {
                                        ScalarValueType::NonScalar
                                    }
                                    _ => ScalarValueType::Unknown,
                                },
                                visible_start: declaration.visible_start(),
                                scope_end: declaration.scope_end(),
                            })
                            .collect(),
                    }
                },
            });

        let function_spans = mod_class
            .as_ref()
            .map(|mod_class| mod_class.collision.name_spans.clone());

        let tokens = context.module.token_stream().cursor();
        let token_facts = context.module.token_facts();
        for call in context.module.function_calls() {
            if self.renames_call(tokens, token_facts, &call, mod_class.as_ref()) {
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
        tokens: TokenCursor<'_>,
        token_facts: &crate::tokenizer::TypedTokenFacts,
        call: &FunctionCall,
        mod_class: Option<&ClassifiedModCollision>,
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
                if !mod_class.collision.has_scalar_binary {
                    return false;
                }
                let Some(first) = call.arguments.get(0) else {
                    return false;
                };
                let Some(second) = call.arguments.get(1) else {
                    return false;
                };
                let scalar = ScalarExpressionAnalyzer {
                    facts: &mod_class.scalar_facts,
                    token_facts,
                    flavor: ScalarExpressionFlavor::ReservedModArgument,
                };
                scalar.is_scalar_range(tokens, first.start(), first.end() + 1)
                    && scalar.is_scalar_range(tokens, second.start(), second.end() + 1)
            }
            _ => false,
        }
    }
}

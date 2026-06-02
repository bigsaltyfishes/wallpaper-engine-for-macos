//! Reserved identifier codegen.

/// Function-name collisions with reserved identifiers.
mod functions;
/// Local identifier collisions with reserved identifiers.
mod locals;
/// User-defined `mod` compatibility classification.
mod mod_function;
/// Scalar facts used for reserved function compatibility.
mod scalar;

use linkme::distributed_slice;
use smol_str::SmolStr;

use self::{
    functions::UserFunctionCollision,
    locals::FunctionLocalRenames,
    mod_function::{ClassifiedModCollision, ModCollisionClass},
    scalar::ScalarTypeFacts,
};
use super::{
    COMPATIBILITY_FUNCTIONS, CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES,
    LEGACY_BUILTINS, LEGACY_TYPES, RESERVED_IDENTIFIERS, StrategyContext,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{
        Fixup, LegacyTypeName, LocalDeclaration, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
    },
    syntax::{FunctionCall, ShaderModule, SyntaxItem},
    tokenizer::TypedToken,
};

/// Renames user-defined functions that collide with GLSL builtins.
struct ReservedIdentifiersStrategy;
#[distributed_slice(GENERAL_POLICIES)]
static RESERVED_IDENTIFIERS_POLICY: CodegenStrategy = CodegenStrategy {
    name: RESERVED_IDENTIFIERS,
    stage: CodegenStage::SemanticRewrite,
    after: &[LEGACY_TYPES, LEGACY_BUILTINS, COMPATIBILITY_FUNCTIONS],
    emitter: &ReservedIdentifiersStrategy,
};

impl Emitable for ReservedIdentifiersStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let context_inner = context.context();
        let token_facts = context_inner.module.token_facts().clone();
        let mut stage_names = Vec::new();
        for name in context_inner
            .module
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
        {
            if !stage_names
                .iter()
                .any(|declared: &SmolStr| declared == name)
            {
                stage_names.push(SmolStr::new(name));
            }
        }
        let mut local_collisions = Vec::new();
        for function in context_inner
            .module
            .items()
            .iter()
            .filter_map(|item| match item {
                SyntaxItem::Function(function) => Some(function),
                _ => None,
            })
        {
            let range = context_inner
                .module
                .token_stream()
                .cursor()
                .contained_range(function.body_span());
            let tokens = context_inner.module.token_stream().cursor();
            local_collisions.extend(
                FunctionLocalRenames {
                    declared: stage_names.clone(),
                    _declared: std::marker::PhantomData,
                    scopes: vec![locals::LocalScope::default()],
                    index: range.start(),
                    end: range.end(),
                    items: Vec::new(),
                }
                .collect(tokens, &token_facts),
            );
        }
        for collision in local_collisions {
            collision.emit(context.context());
        }
        UserFunctionCollision {
            source: "mod",
            replacement: "_we_user_mod",
        }
        .emit(context);
        UserFunctionCollision {
            source: "sample",
            replacement: "_we_user_sample",
        }
        .emit(context);
        Ok(())
    }
}

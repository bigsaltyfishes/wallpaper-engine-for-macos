//! Focused type coercions for legacy `SceneShader` expressions.

/// Assignment-site vector width coercions.
mod assignment;
/// Binary expression vector width coercions.
mod binary;
/// Builtin function call argument coercions.
mod calls;
/// Declaration initializer vector coercions.
mod initializer;
/// Texture sample initializer coercions.
mod texture;
/// Shared vector type facts for type coercion policies.
mod types;

use linkme::distributed_slice;

use self::{
    assignment::NarrowVectorAssignments,
    binary::{Vec3Vec2BinaryExpressions, VectorBinaryExpressions},
    calls::{CoercionFunction, FunctionCoercion},
    initializer::{NarrowVectorInitializers, ScalarVectorInitializers, VectorScalarInitializers},
    texture::TextureVectorInitializers,
    types::VectorTypeBindings,
};
use super::{Emitable, GENERAL_POLICIES, PolicyContext, TextureSamplingCall};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{
        DeclarationDeclarators, DeclaratorInitializer, Fixup, FunctionCall, FunctionCallIndex,
        LocalDeclaration, LocalDeclarationStart, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode, TokenSearch,
        tokens::BalancedTokens,
    },
    lexer::{Token, TokenKind},
};

/// Applies small type-shape fixups required by strict GLSL frontends.
struct TypeCoercionPolicy;
#[distributed_slice(GENERAL_POLICIES)]
static TYPE_COERCION_POLICY: &dyn Emitable = &TypeCoercionPolicy;
impl Emitable for TypeCoercionPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.tokens();
        let vector_facts = VectorTypeBindings::from(tokens);
        for call in FunctionCallIndex::new(tokens).iter() {
            let Some(function) = (match call.name() {
                "mix" | "lerp" => Some(CoercionFunction::Mix),
                "smoothstep" => Some(CoercionFunction::Smoothstep),
                "step" => Some(CoercionFunction::Step),
                "pow" => Some(CoercionFunction::Pow),
                "clamp" => Some(CoercionFunction::Clamp),
                "min" => Some(CoercionFunction::Min),
                "max" => Some(CoercionFunction::Max),
                _ => None,
            }) else {
                continue;
            };
            FunctionCoercion {
                call,
                function,
                vector_facts: &vector_facts,
            }
            .emit(context);
        }

        let mut declarations = NarrowVectorInitializers {
            facts: &vector_facts,
            items: Vec::new(),
        };
        declarations.scan(tokens);
        for declaration in declarations.items {
            declaration.emit(context);
        }

        let scalar_vectors = ScalarVectorInitializers::from((tokens, &vector_facts));
        for initializer in scalar_vectors.items {
            initializer.emit(context);
        }

        let scalar_initializers = VectorScalarInitializers::from(tokens);
        for initializer in scalar_initializers.items {
            initializer.emit(context);
        }

        for initializer in TextureVectorInitializers::from(&mut *context) {
            initializer.emit(context);
        }

        let assignments = NarrowVectorAssignments::from(&mut *context);
        for assignment in assignments.items {
            assignment.emit(context);
        }

        let binary_vectors = Vec3Vec2BinaryExpressions::from((tokens, &vector_facts));
        for expression in binary_vectors.items {
            expression.emit(context);
        }

        let vector_binary_expressions = VectorBinaryExpressions::from((tokens, &vector_facts));
        for expression in vector_binary_expressions.items {
            expression.emit(context);
        }

        Ok(())
    }
}

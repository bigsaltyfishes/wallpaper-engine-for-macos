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
/// Shared vector type facts for type coercion strategies.
mod types;

use linkme::distributed_slice;

use self::{
    assignment::{NarrowVectorAssignments, ScalarVectorAssignments},
    binary::{Vec3Vec2BinaryExpressions, VectorBinaryExpressions},
    calls::{CoercionFunction, FunctionCoercion},
    initializer::{
        NarrowVectorInitializers, ScalarVectorInitializers, ScalarVectorReturnInitializers,
        VectorScalarInitializers,
    },
    texture::{TextureCoordinateArguments, TextureVectorInitializers},
    types::VectorTypeBindings,
};
use super::{
    ARRAY_PARAMETERS, CONTROL_FLOW_COERCION, CodegenStage, CodegenStrategy, Emitable,
    GENERAL_POLICIES, HLSL_MUL, LEGACY_BUILTINS, StrategyContext, TEXTURE_SAMPLING, TYPE_COERCION,
    TextureSamplingCall,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{
        DeclaratorInitializer, Fixup, LocalDeclaration, ScopedDeclarationFacts,
        ScopedDeclarationFactsConfig, ScopedDeclarationTypeMode,
    },
    tokenizer::TypedToken,
};

/// Applies small type-shape fixups required by strict GLSL frontends.
struct TypeCoercionStrategy;
#[distributed_slice(GENERAL_POLICIES)]
static TYPE_COERCION_POLICY: CodegenStrategy = CodegenStrategy {
    name: TYPE_COERCION,
    stage: CodegenStage::TypeCodegen,
    after: &[
        CONTROL_FLOW_COERCION,
        HLSL_MUL,
        TEXTURE_SAMPLING,
        LEGACY_BUILTINS,
        ARRAY_PARAMETERS,
    ],
    emitter: &TypeCoercionStrategy,
};

impl Emitable for TypeCoercionStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.token_stream().cursor();
        let token_facts = context.context().module.token_facts().clone();
        let scoped_facts = ScopedDeclarationFacts::collect(
            context.context().module,
            ScopedDeclarationFactsConfig {
                parameter_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
                local_types: ScopedDeclarationTypeMode::BuiltinsAndStructs,
            },
        );
        let mut functions = types::FunctionTypeBindings::default();
        functions.collect(token_facts.function_signatures());
        let mut vector_facts =
            VectorTypeBindings::new(&scoped_facts, &context.context().declarations);
        vector_facts.functions = functions.items;
        let function_calls = context
            .context()
            .module
            .function_calls()
            .collect::<Vec<_>>();
        for call in function_calls.iter().cloned() {
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
                token_facts: &token_facts,
            }
            .emit(context, tokens);
        }

        let mut declarations = NarrowVectorInitializers {
            facts: &vector_facts,
            items: Vec::new(),
        };
        declarations.scan(tokens, &token_facts);
        for declaration in declarations.items {
            declaration.emit(context);
        }

        let mut scalar_vectors = ScalarVectorInitializers::default();
        scalar_vectors.collect(tokens, &token_facts, &vector_facts);
        for initializer in scalar_vectors.items {
            initializer.emit(context);
        }

        let mut scalar_vector_returns = ScalarVectorReturnInitializers::default();
        scalar_vector_returns.collect(tokens, &token_facts, &function_calls, &vector_facts);
        for initializer in scalar_vector_returns.items {
            initializer.emit(context);
        }

        let mut scalar_initializers = VectorScalarInitializers::default();
        scalar_initializers.collect(tokens, &token_facts);
        for initializer in scalar_initializers.items {
            initializer.emit(context);
        }

        let mut texture_initializers = TextureVectorInitializers { items: Vec::new() };
        texture_initializers.collect_from_context(&mut *context);
        for initializer in texture_initializers {
            initializer.emit(context);
        }

        let mut texture_coordinates = TextureCoordinateArguments { items: Vec::new() };
        texture_coordinates.collect_from_context(&mut *context, &vector_facts);
        for coordinate in texture_coordinates.items {
            coordinate.emit(context);
        }

        let mut assignments = NarrowVectorAssignments { items: Vec::new() };
        assignments.collect_from_context(&mut *context, &token_facts, &vector_facts);
        for assignment in assignments.items {
            assignment.emit(context);
        }

        let mut scalar_assignments = ScalarVectorAssignments { items: Vec::new() };
        scalar_assignments.collect(tokens, &token_facts, &vector_facts);
        for assignment in scalar_assignments.items {
            assignment.emit(context);
        }

        let mut binary_vectors = Vec3Vec2BinaryExpressions::default();
        binary_vectors.collect(tokens, &token_facts, &vector_facts);
        for expression in binary_vectors.items {
            expression.emit(context);
        }

        let mut vector_binary_expressions = VectorBinaryExpressions::default();
        vector_binary_expressions.collect(tokens, &token_facts, &vector_facts);
        for expression in vector_binary_expressions.items {
            expression.emit(context);
        }
        Ok(())
    }
}

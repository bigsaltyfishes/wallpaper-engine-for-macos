//! Compatibility helper function emission requests.

use linkme::distributed_slice;

use super::{
    COMPATIBILITY_FUNCTIONS, CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES,
    LEGACY_BUILTINS, StrategyContext,
};
use crate::{ShaderResult, ShaderStageKind};

/// Requests generated helper functions only when source references them.
struct CompatibilityFunctionsStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static COMPATIBILITY_FUNCTIONS_POLICY: CodegenStrategy = CodegenStrategy {
    name: COMPATIBILITY_FUNCTIONS,
    stage: CodegenStage::CompatibilityExpansion,
    after: &[LEGACY_BUILTINS],
    emitter: &CompatibilityFunctionsStrategy,
};

impl Emitable for CompatibilityFunctionsStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let fragment_stage = context.context().module.stage() == ShaderStageKind::Fragment;

        for call in context.context().module.function_calls() {
            match call.name() {
                "clip"
                    if fragment_stage
                        && !context.context().declarations.has_user_function("clip") =>
                {
                    context.context().declarations.require_clip_functions();
                }
                "PerformLighting_V1"
                    if !context
                        .context()
                        .declarations
                        .has_user_function("PerformLighting_V1") =>
                {
                    context
                        .context()
                        .declarations
                        .require_perform_lighting_functions();
                }
                _ => {}
            }
        }
        Ok(())
    }
}

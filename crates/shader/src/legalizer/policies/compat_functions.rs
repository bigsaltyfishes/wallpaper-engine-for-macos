//! Compatibility helper function emission requests.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{ShaderResult, ShaderStageKind, legalizer::FunctionCallIndex};

/// Requests generated helper functions only when source references them.
struct CompatibilityFunctionsPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static COMPATIBILITY_FUNCTIONS_POLICY: &dyn Emitable = &CompatibilityFunctionsPolicy;

impl Emitable for CompatibilityFunctionsPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let fragment_stage = context.context().module.stage() == ShaderStageKind::Fragment;

        let calls = FunctionCallIndex::new(context.context().module.tokens());
        for call in calls.iter() {
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

//! Mutable stage input legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{ShaderResult, ShaderStageKind, legalizer::StageInputWrite};

/// Marks written stage inputs for generated local mutable copies.
struct MutableInputsPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static MUTABLE_INPUTS_POLICY: &dyn Emitable = &MutableInputsPolicy;

impl Emitable for MutableInputsPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        if !matches!(
            context.context().module.stage(),
            ShaderStageKind::Vertex | ShaderStageKind::Fragment
        ) {
            return Ok(());
        }

        let tokens = context.context().module.tokens();
        for interface in context.context().declarations.stage_inputs_mut() {
            if (StageInputWrite {
                tokens,
                name: interface.name.as_ref(),
            })
            .exists()
            {
                interface.use_local_copy();
            }
        }
        Ok(())
    }
}

//! Mutable stage input codegen.

use linkme::distributed_slice;

use super::{
    ARRAY_PARAMETERS, CONTROL_FLOW_COERCION, CodegenStage, CodegenStrategy, Emitable,
    GENERAL_POLICIES, MUTABLE_INPUTS, StrategyContext, TYPE_COERCION,
};
use crate::{ShaderResult, ShaderStageKind};

/// Marks written stage inputs for generated local mutable copies.
struct MutableInputsStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static MUTABLE_INPUTS_POLICY: CodegenStrategy = CodegenStrategy {
    name: MUTABLE_INPUTS,
    stage: CodegenStage::OutputPreparation,
    after: &[TYPE_COERCION, CONTROL_FLOW_COERCION, ARRAY_PARAMETERS],
    emitter: &MutableInputsStrategy,
};

impl Emitable for MutableInputsStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        if !matches!(
            context.context().module.stage(),
            ShaderStageKind::Vertex | ShaderStageKind::Fragment
        ) {
            return Ok(());
        }

        let module = context.context().module;
        for interface in context.context().declarations.stage_inputs_mut() {
            if module.writes_stage_input(interface.name.as_ref()) {
                interface.use_local_copy();
            }
        }
        Ok(())
    }
}

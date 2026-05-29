//! Fragment output codegen.

use linkme::distributed_slice;

use super::{
    ALPHA_TO_COVERAGE, CONTROL_FLOW_COERCION, CodegenStage, CodegenStrategy, Emitable,
    FRAGMENT_OUTPUT, GENERAL_POLICIES, MUTABLE_INPUTS, SCALAR_TEXTURE, StrategyContext,
    TYPE_COERCION,
};
use crate::{
    ShaderResult, ShaderStageKind,
    codegen::{Fixup, FragmentOutput},
};

/// Replaces `gl_FragColor` with a generated explicit fragment output.
struct FragmentOutputStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static FRAGMENT_OUTPUT_POLICY: CodegenStrategy = CodegenStrategy {
    name: FRAGMENT_OUTPUT,
    stage: CodegenStage::OutputPreparation,
    after: &[
        ALPHA_TO_COVERAGE,
        TYPE_COERCION,
        SCALAR_TEXTURE,
        CONTROL_FLOW_COERCION,
        MUTABLE_INPUTS,
    ],
    emitter: &FragmentOutputStrategy,
};

impl Emitable for FragmentOutputStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let context = context.context();
        if context.module.stage() != ShaderStageKind::Fragment {
            return Ok(());
        }

        for token in context.module.token_stream().identifiers() {
            if token.text() == "gl_FragColor" {
                context.declarations.require_fragment_output();
                context
                    .fixups
                    .push(Fixup::replace(token.span(), FragmentOutput::NAME));
            }
        }
        Ok(())
    }
}

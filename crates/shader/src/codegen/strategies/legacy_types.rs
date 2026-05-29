//! Legacy type name codegen.

use linkme::distributed_slice;

use super::{
    CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES, LEGACY_TYPES, StrategyContext,
};
use crate::{
    ShaderResult,
    codegen::{Fixup, declarations::LegacyTypeName},
};

/// Rewrites HLSL/Wallpaper Engine vector aliases to GLSL type names.
struct LegacyTypesStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static LEGACY_TYPES_POLICY: CodegenStrategy = CodegenStrategy {
    name: LEGACY_TYPES,
    stage: CodegenStage::CompatibilityExpansion,
    after: &[],
    emitter: &LegacyTypesStrategy,
};

impl Emitable for LegacyTypesStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let context = context.context();
        for token in context.module.token_stream().identifiers() {
            let replacement = LegacyTypeName::new(token.text()).glsl();
            if replacement != token.text() {
                context
                    .fixups
                    .push(Fixup::replace(token.span(), replacement));
            }
        }
        Ok(())
    }
}

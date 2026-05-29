//! Fragment output legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult, ShaderStageKind,
    legalizer::{Fixup, FragmentOutput},
};

/// Replaces `gl_FragColor` with a generated explicit fragment output.
struct FragmentOutputPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static FRAGMENT_OUTPUT_POLICY: &dyn Emitable = &FragmentOutputPolicy;

impl Emitable for FragmentOutputPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        if context.context().module.stage() != ShaderStageKind::Fragment {
            return Ok(());
        }

        for token in context.context().tokens.identifiers() {
            if token.text() == "gl_FragColor" {
                context.context().declarations.require_fragment_output();
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(token.span(), FragmentOutput::NAME));
            }
        }
        Ok(())
    }
}

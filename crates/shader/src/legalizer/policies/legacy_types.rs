//! Legacy type name legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{ShaderResult, legalizer::Fixup};

/// Rewrites HLSL/Wallpaper Engine vector aliases to GLSL type names.
struct LegacyTypesPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static LEGACY_TYPES_POLICY: &dyn Emitable = &LegacyTypesPolicy;

impl Emitable for LegacyTypesPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        for token in context.context().tokens.identifiers() {
            let replacement = match token.text() {
                "float1" => Some("float"),
                "float2" => Some("vec2"),
                "float3" => Some("vec3"),
                "float4" => Some("vec4"),
                _ => None,
            };
            if let Some(replacement) = replacement {
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(token.span(), replacement));
            }
        }
        Ok(())
    }
}

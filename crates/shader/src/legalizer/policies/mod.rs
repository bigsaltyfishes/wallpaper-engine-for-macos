//! Internal legalizer policies.

mod alpha_to_coverage;
mod array_parameters;
mod compat_functions;
mod control_flow_coercion;
mod fragment_output;
mod hlsl_mul;
mod legacy_builtins;
mod legacy_types;
mod mutable_inputs;
mod reserved_identifiers;
mod scalar_texture;
mod texture_sampling;
mod type_coercion;

use linkme::distributed_slice;
use texture_sampling::{TextureSamplingCall, TextureSamplingFunction};

use super::LegalizationContext;
use crate::ShaderResult;

/// Behavior implemented by one legalization policy.
pub trait Emitable: Sync {
    /// Marks source fixups or generated declarations for this policy.
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()>;
}

/// Registered order-independent legalizer policies.
#[distributed_slice]
pub(crate) static GENERAL_POLICIES: [&'static dyn Emitable] = [..];

/// Execution order for the general policy slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyOrder {
    /// Production linkme iteration order.
    Natural,
    /// Test-only dependency check that runs policies in reverse order.
    #[cfg(test)]
    Reverse,
}

/// Typed policy access to the shared legalizer context.
pub struct PolicyContext<'ctx, 'module, 'src> {
    /// Shared legalizer context.
    pub(crate) context: &'ctx mut LegalizationContext<'module, 'src>,
}

impl<'module, 'src> PolicyContext<'_, 'module, 'src> {
    /// Returns the shared legalizer context.
    pub(crate) fn context(&mut self) -> &mut LegalizationContext<'module, 'src> {
        self.context
    }
}

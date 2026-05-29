//! Internal legalizer strategies.

mod alpha_to_coverage;
mod array_parameters;
mod compat_functions;
mod control_flow_coercion;
mod fragment_output;
mod hlsl_mul;
mod legacy_builtins;
mod legacy_types;
mod mutable_inputs;
mod pipeline;
mod reserved_identifiers;
mod scalar_texture;
mod texture_sampling;
mod type_coercion;

use linkme::distributed_slice;
use texture_sampling::{TextureSamplingCall, TextureSamplingFunction};

use super::CodegenContext;
use crate::ShaderResult;

/// Stable legalizer strategy identifier used by dependency metadata.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CodegenStrategyName(&'static str);

impl CodegenStrategyName {
    /// Creates a stable strategy name.
    #[must_use]
    pub(super) const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Returns the strategy name as a string.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Strategy name for legacy type alias rewriting.
pub(super) const LEGACY_TYPES: CodegenStrategyName = CodegenStrategyName::new("legacy_types");
/// Strategy name for texture sampling call rewriting.
pub(super) const TEXTURE_SAMPLING: CodegenStrategyName =
    CodegenStrategyName::new("texture_sampling");
/// Strategy name for legacy builtin call rewriting.
pub(super) const LEGACY_BUILTINS: CodegenStrategyName = CodegenStrategyName::new("legacy_builtins");
/// Strategy name for HLSL `mul` call rewriting.
pub(super) const HLSL_MUL: CodegenStrategyName = CodegenStrategyName::new("hlsl_mul");
/// Strategy name for compatibility helper function requests.
pub(super) const COMPATIBILITY_FUNCTIONS: CodegenStrategyName =
    CodegenStrategyName::new("compatibility_functions");
/// Strategy name for reserved identifier rewriting.
pub(super) const RESERVED_IDENTIFIERS: CodegenStrategyName =
    CodegenStrategyName::new("reserved_identifiers");
/// Strategy name for fixed-array parameter specialization.
pub(super) const ARRAY_PARAMETERS: CodegenStrategyName =
    CodegenStrategyName::new("array_parameters");
/// Strategy name for alpha-to-coverage derivative rewriting.
pub(super) const ALPHA_TO_COVERAGE: CodegenStrategyName =
    CodegenStrategyName::new("alpha_to_coverage");
/// Strategy name for control-flow scalar coercions.
pub(super) const CONTROL_FLOW_COERCION: CodegenStrategyName =
    CodegenStrategyName::new("control_flow_coercion");
/// Strategy name for strict GLSL type-shape coercions.
pub(super) const TYPE_COERCION: CodegenStrategyName = CodegenStrategyName::new("type_coercion");
/// Strategy name for scalar texture assignment rewriting.
pub(super) const SCALAR_TEXTURE: CodegenStrategyName = CodegenStrategyName::new("scalar_texture");
/// Strategy name for mutable stage input preparation.
pub(super) const MUTABLE_INPUTS: CodegenStrategyName = CodegenStrategyName::new("mutable_inputs");
/// Strategy name for generated fragment output preparation.
pub(super) const FRAGMENT_OUTPUT: CodegenStrategyName = CodegenStrategyName::new("fragment_output");

/// Coarse legalizer execution stage.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum CodegenStage {
    /// Legacy spelling and compatibility call expansion.
    CompatibilityExpansion,
    /// Source-structure rewrites before type repair.
    SemanticRewrite,
    /// Strict-GLSL type and expression shape repairs.
    TypeCodegen,
    /// Generated input/output preparation before final source emission.
    OutputPreparation,
}

/// Metadata-rich legalizer strategy registration.
#[derive(Clone, Copy)]
pub(super) struct CodegenStrategy {
    /// Stable strategy identity.
    pub name: CodegenStrategyName,
    /// Coarse execution stage.
    pub stage: CodegenStage,
    /// Strategies that must run before this strategy.
    pub after: &'static [CodegenStrategyName],
    /// Strategy implementation.
    pub emitter: &'static dyn Emitable,
}

/// Behavior implemented by one codegen strategy.
pub(super) trait Emitable: Sync {
    /// Marks source fixups or generated declarations for this strategy.
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()>;
}

/// Registered order-independent legalizer strategies.
#[distributed_slice]
pub static GENERAL_POLICIES: [CodegenStrategy] = [..];

/// Typed strategy access to the shared legalizer context.
pub(super) struct StrategyContext<'ctx, 'module, 'src> {
    /// Shared legalizer context.
    pub(super) context: &'ctx mut CodegenContext<'module, 'src>,
}

impl<'module, 'src> StrategyContext<'_, 'module, 'src> {
    /// Runs the deterministic legalizer pipeline.
    pub(super) fn emit_pipeline(&mut self) -> ShaderResult<()> {
        pipeline::LEGALIZER_PIPELINE.emit(self)
    }

    /// Returns the shared legalizer context.
    pub(super) fn context(&mut self) -> &mut CodegenContext<'module, 'src> {
        self.context
    }
}

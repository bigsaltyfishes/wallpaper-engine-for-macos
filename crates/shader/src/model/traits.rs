use super::{CompiledStageArtifact, ShaderReflection, ShaderStageKind};
use crate::{ShaderResult, legalize::LegalizedStageSource};

/// Trait for shader compiler backends.
pub trait ShaderCompiler {
    /// Backend module type retained internally for reflection.
    type Module;

    /// Compiles one shader stage.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot compile the provided source.
    fn compile_stage(
        &self,
        stage: ShaderStageKind,
        source: &LegalizedStageSource,
    ) -> ShaderResult<CompiledStageArtifact<Self::Module>>;
}

/// Trait for shader reflection backends.
pub trait ShaderReflector<M> {
    /// Reflects a compiled module.
    ///
    /// # Errors
    ///
    /// Returns an error when reflected bindings cannot be represented by the
    /// core model.
    fn reflect_stage(&self, stage: ShaderStageKind, module: &M) -> ShaderResult<ShaderReflection>;
}

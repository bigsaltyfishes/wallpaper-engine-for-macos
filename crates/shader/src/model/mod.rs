//! Strongly typed shader request and output model.

/// Compiled shader output model.
mod compiled;
/// Validated identifier and index newtypes.
mod identifiers;
/// Extracted shader metadata model.
mod metadata;
/// Renderer-neutral reflection model.
mod reflection;
/// Shader compilation request model.
mod request;
/// Shader stage and target model.
mod stage;
/// Shader texture metadata model.
mod texture;
/// Compiler and reflector backend traits.
mod traits;

pub use compiled::{
    CompiledShaderProgram, CompiledShaderStage, CompiledStageArtifact, ShaderCacheKey,
};
pub use identifiers::{
    BindingIndex, BindingSet, ComboName, IncludePath, LocationIndex, ShaderName, ShaderSymbolName,
};
pub use metadata::{
    DefaultTextureValue, DefaultUniformValue, MaterialAlias, ShaderComboValue, ShaderMetadata,
};
pub use reflection::{
    ShaderDescriptorBinding, ShaderDescriptorKind, ShaderReflection, ShaderStageMask,
    ShaderUniformBlock, ShaderUniformMember, ShaderVertexInput, VertexFormat,
};
pub use request::{ShaderProgramRequest, ShaderProgramRequestBuilder};
pub use stage::{ShaderCachePolicy, ShaderStageKind, ShaderStageSource, ShaderTarget};
pub use texture::{ShaderTextureInfo, TextureComponentState, TextureFormatHint, TextureSlot};
pub use traits::{ShaderCompiler, ShaderReflector};

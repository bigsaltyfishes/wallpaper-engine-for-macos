//! Typed Rust shader pipeline contracts and core data model.
#![deny(clippy::correctness)]
#![deny(clippy::single_call_fn)]
#![deny(clippy::complexity)]
#![warn(clippy::pedantic)]
#![warn(clippy::useless_attribute)]
#![warn(clippy::excessive_precision)]
#![warn(clippy::missing_docs_in_private_items)]

pub mod backend;
pub mod error;
pub mod layout;
pub mod legalizer;
pub mod lexer;
pub mod metadata;
pub mod model;
pub mod pipeline;
pub mod preprocess;
pub mod property;
pub mod source;
pub mod syntax;

#[cfg(feature = "ffi")]
pub mod compat;

#[cfg(feature = "ffi")]
pub mod ffi {
    //! Compatibility re-export for the C ABI bridge.

    pub use crate::compat::ffi::*;
}

pub mod compile {
    //! Compatibility re-export for the Naga compiler backend.

    pub use crate::backend::naga::NagaCompiler;
}

pub mod reflect {
    //! Compatibility re-export for the Naga reflection backend.

    pub use crate::backend::naga::NagaReflector;
}

pub use error::{ShaderDiagnostic, ShaderError, ShaderResult, SourceSpan};
pub use legalizer as legalize;
pub use model::{
    BindingIndex, BindingSet, ComboName, CompiledShaderProgram, CompiledShaderStage,
    CompiledStageArtifact, DefaultTextureValue, DefaultUniformValue, IncludePath, LocationIndex,
    MaterialAlias, ShaderCacheKey, ShaderCachePolicy, ShaderComboValue, ShaderCompiler,
    ShaderDescriptorBinding, ShaderDescriptorKind, ShaderMetadata, ShaderName,
    ShaderProgramRequest, ShaderProgramRequestBuilder, ShaderReflection, ShaderReflector,
    ShaderStageKind, ShaderStageMask, ShaderStageSource, ShaderSymbolName, ShaderTarget,
    ShaderTextureInfo, ShaderUniformBlock, ShaderUniformMember, ShaderVertexInput,
    TextureComponentState, TextureFormatHint, TextureSlot, VertexFormat,
};
pub use property::{ProjectPropertyBinding, PropertyName, PropertyValue};
pub use source::{InMemoryShaderSourceProvider, ShaderSourceProvider};

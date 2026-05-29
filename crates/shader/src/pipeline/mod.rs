//! Typed shader program pipeline orchestration.

/// Stable shader cache-key construction.
mod cache;
/// Request-level pipeline orchestration.
mod context;
/// Preprocessed and parsed stage inputs.
mod inputs;
/// Cross-stage interface analysis and layout.
mod interface;
/// Metadata merge and request combo augmentation.
mod metadata;
/// Reflection merge across compiled stages.
mod reflection;
/// Program resource layout and descriptor allocation.
mod resources;
/// Pipeline revision identity.
mod revision;
/// Stage-local compilation pipeline.
mod stage;

pub use context::{DefaultShaderPipeline, ShaderPipeline};
pub use revision::ShaderPipelineRevision;

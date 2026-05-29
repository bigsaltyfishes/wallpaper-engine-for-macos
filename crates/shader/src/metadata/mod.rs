//! Wallpaper Engine shader metadata extraction from parsed shader syntax.

/// Wallpaper Engine annotation JSON cursor and slice helpers.
mod annotation_json;
/// Metadata accumulation from parsed annotation facts.
mod builder;
/// Metadata traversal over parsed shader modules.
mod extractor;

pub use extractor::ShaderModuleMetadataExt;

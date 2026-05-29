//! Generated resource declarations.

use std::{borrow::Cow, fmt::Write as _};

use super::{super::emission::SourceEmitter, types::LegacyTypeName};
use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, ShaderStageKind, layout::DescriptorBinding,
};

/// GLSL sampler uniform type classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SamplerType<'src> {
    /// Source type spelling.
    name: &'src str,
}

impl<'src> SamplerType<'src> {
    /// Returns a sampler classification for GLSL sampler type names.
    pub(crate) fn new(name: &'src str) -> Option<Self> {
        const PREFIXES: [&str; 3] = ["sampler", "isampler", "usampler"];

        PREFIXES
            .iter()
            .any(|prefix| {
                name.strip_prefix(prefix).is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.chars().next().is_some_and(|first| {
                            first.is_ascii_digit() || first.is_ascii_uppercase()
                        })
                })
            })
            .then_some(Self { name })
    }

    /// Returns whether the legalizer can split this source sampler into
    /// backend-compatible texture and sampler descriptors.
    pub(crate) fn supports_texture_split(self) -> bool {
        self.name == "sampler2D"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Scalar or vector uniform moved into the generated global block.
pub struct UniformMember<'src> {
    /// Source type name.
    pub(crate) ty: &'src str,
    /// Source variable name.
    pub(crate) name: &'src str,
    /// Optional array suffix following the declaration name.
    pub(crate) array_suffix: Option<Cow<'src, str>>,
    /// Explicit layout binding parsed from source, when present.
    pub(crate) explicit_binding: Option<u32>,
    /// Descriptor binding assigned to the generated block.
    pub(crate) binding: Option<DescriptorBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Generated std140 block containing scalar/vector uniforms.
pub struct UniformBlock<'src> {
    /// Members emitted inside the block.
    pub(super) members: Vec<UniformMember<'src>>,
    /// Descriptor binding shared by all members.
    pub(super) binding: DescriptorBinding,
}

impl UniformBlock<'_> {
    /// Emits the generated uniform block declaration, resolving member array
    /// suffixes through a caller-supplied macro resolver when available.
    pub(crate) fn emit_with_array_suffix_resolver(
        &self,
        output: &mut String,
        mut resolve_array_suffix: impl FnMut(&str) -> Option<String>,
    ) -> ShaderResult<()> {
        writeln!(
            output,
            "layout(std140, set = {}, binding = {}) uniform GlobalUniforms {{",
            self.binding.set(),
            self.binding.binding()
        )
        .map_err(SourceEmitter::write_error)?;
        for member in &self.members {
            writeln!(
                output,
                "    {} {}{};",
                LegacyTypeName::new(member.ty).glsl(),
                member.name,
                member
                    .array_suffix
                    .as_deref()
                    .and_then(&mut resolve_array_suffix)
                    .as_deref()
                    .or(member.array_suffix.as_deref())
                    .unwrap_or_default()
            )
            .map_err(SourceEmitter::write_error)?;
        }
        writeln!(output, "}};").map_err(SourceEmitter::write_error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Separated texture declaration with an assigned descriptor binding.
pub struct TextureDeclaration<'src> {
    /// Emitted texture type name.
    pub(super) ty: &'src str,
    /// Source texture variable name.
    pub(super) name: &'src str,
    /// Descriptor binding assigned to the texture.
    pub(super) binding: Option<DescriptorBinding>,
    /// Descriptor binding assigned to this texture's paired sampler.
    pub(super) sampler_binding: Option<DescriptorBinding>,
}

impl TextureDeclaration<'_> {
    /// Prefix for generated sampler descriptors paired to texture declarations.
    pub(crate) const SAMPLER_PREFIX: &'static str = "_we_Sampler_";

    /// Parses `g_TextureN` texture names into fixed binding indices.
    pub(super) fn texture_binding(self, stage: ShaderStageKind) -> ShaderResult<Option<u32>> {
        let Some(suffix) = self.name.strip_prefix("g_Texture") else {
            return Ok(None);
        };
        if suffix.is_empty() || !suffix.chars().all(|character| character.is_ascii_digit()) {
            return Ok(None);
        }
        if suffix.len() > 1 && suffix.starts_with('0') {
            return Err(ShaderError::Legalize {
                diagnostics: Box::new([self.non_canonical_binding_diagnostic(stage)]),
            });
        }

        Ok(suffix.parse::<u32>().ok())
    }

    /// Builds a diagnostic for non-canonical `g_TextureN` encoded bindings.
    fn non_canonical_binding_diagnostic(self, stage: ShaderStageKind) -> ShaderDiagnostic {
        ShaderDiagnostic::new(format!(
            "source texture `{}` is not a canonical g_TextureN descriptor binding name",
            self.name
        ))
        .with_stage(stage)
        .with_pass("Legalizer")
    }

    /// Builds a diagnostic for duplicate `g_TextureN` encoded bindings.
    pub(super) fn duplicate_binding_diagnostic(
        self,
        stage: ShaderStageKind,
        previous_name: &str,
        binding: u32,
    ) -> ShaderDiagnostic {
        ShaderDiagnostic::new(format!(
            "source textures `{previous_name}` and `{}` both encode descriptor binding {binding}",
            self.name
        ))
        .with_stage(stage)
        .with_pass("Legalizer")
    }

    /// Emits the generated texture declaration.
    pub(crate) fn emit(self, output: &mut String) -> ShaderResult<()> {
        let binding = self.binding.ok_or_else(|| {
            ShaderError::invalid_request("texture binding was not assigned before emission")
        })?;
        writeln!(
            output,
            "layout(set = {}, binding = {}) uniform {} {};",
            binding.set(),
            binding.binding(),
            self.ty,
            self.name
        )
        .map_err(SourceEmitter::write_error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Generated sampler paired to a separated texture handle.
pub struct TextureSampler<'src> {
    /// Source texture variable name.
    pub(super) texture_name: &'src str,
    /// Generated sampler descriptor binding.
    pub(super) binding: DescriptorBinding,
}

impl TextureSampler<'_> {
    /// Emits the generated sampler declaration.
    pub(crate) fn emit(self, output: &mut String) -> ShaderResult<()> {
        writeln!(
            output,
            "layout(set = {}, binding = {}) uniform sampler {};",
            self.binding.set(),
            self.binding.binding(),
            TextureDeclaration::SAMPLER_PREFIX.to_owned() + self.texture_name
        )
        .map_err(SourceEmitter::write_error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Generated fragment color output declaration.
pub struct FragmentOutput;

impl FragmentOutput {
    /// Generated output variable name used to replace `gl_FragColor`.
    pub(crate) const NAME: &'static str = "_we_FragColor";
}

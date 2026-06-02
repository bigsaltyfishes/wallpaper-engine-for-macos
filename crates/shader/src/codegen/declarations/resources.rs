//! Generated resource declarations.

use std::fmt::Write as _;

use smol_str::SmolStr;

use super::{super::emission::SourceEmitter, types::LegacyTypeName};
use crate::{
    ShaderDiagnostic, ShaderError, ShaderResult, ShaderStageKind, layout::DescriptorBinding,
};

/// GLSL sampler uniform type classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SamplerType<'src> {
    /// Source type spelling.
    name: &'src str,
}

impl<'src> SamplerType<'src> {
    /// Returns a sampler classification for GLSL sampler type names.
    #[must_use]
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
    #[must_use]
    pub(crate) fn supports_texture_split(self) -> bool {
        self.name == "sampler2D"
    }
}

/// Scalar or vector uniform moved into the generated global block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UniformMember {
    /// Source type name.
    pub ty: SmolStr,
    /// Source variable name.
    pub name: SmolStr,
    /// Optional array suffix following the declaration name.
    pub array_suffix: Option<SmolStr>,
    /// Explicit layout binding parsed from source, when present.
    pub explicit_binding: Option<u32>,
    /// Descriptor binding assigned to the generated block.
    pub binding: Option<DescriptorBinding>,
}

/// Generated std140 block containing scalar/vector uniforms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UniformBlock {
    /// Members emitted inside the block.
    pub members: Vec<UniformMember>,
    /// Descriptor binding shared by all members.
    pub binding: DescriptorBinding,
}

impl UniformBlock {
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
                LegacyTypeName::new(member.ty.as_str()).glsl(),
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

/// Separated texture declaration with an assigned descriptor binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextureDeclaration<'src> {
    /// Emitted texture type name.
    pub ty: &'src str,
    /// Source texture variable name.
    pub name: &'src str,
    /// Descriptor binding assigned to the texture.
    pub binding: Option<DescriptorBinding>,
    /// Descriptor binding assigned to this texture's paired sampler.
    pub sampler_binding: Option<DescriptorBinding>,
}

impl TextureDeclaration<'_> {
    /// Prefix for generated sampler descriptors paired to texture declarations.
    pub(crate) const SAMPLER_PREFIX: &'static str = "_we_Sampler_";

    /// Parses `g_TextureN` texture names into fixed binding indices.
    pub(crate) fn texture_binding(self, stage: ShaderStageKind) -> ShaderResult<Option<u32>> {
        let Some(suffix) = self.name.strip_prefix("g_Texture") else {
            return Ok(None);
        };
        if suffix.is_empty() || !suffix.chars().all(|character| character.is_ascii_digit()) {
            return Ok(None);
        }
        if suffix.len() > 1 && suffix.starts_with('0') {
            return Err(ShaderError::Codegen {
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
        .with_pass("Codegen")
    }

    /// Builds a diagnostic for duplicate `g_TextureN` encoded bindings.
    pub(crate) fn duplicate_binding_diagnostic(
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
        .with_pass("Codegen")
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

/// Generated sampler paired to a separated texture handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextureSampler<'src> {
    /// Source texture variable name.
    pub texture_name: &'src str,
    /// Generated sampler descriptor binding.
    pub binding: DescriptorBinding,
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

/// Generated fragment color output declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FragmentOutput;

impl FragmentOutput {
    /// Generated output variable name used to replace `gl_FragColor`.
    pub(crate) const NAME: &'static str = "_we_FragColor";
}

//! Bridge response JSON DTOs.

use crate::{
    DefaultTextureValue, DefaultUniformValue, MaterialAlias, PropertyValue, ShaderComboValue,
    ShaderDescriptorBinding, ShaderMetadata, ShaderReflection, ShaderStageMask, ShaderUniformBlock,
    ShaderUniformMember, ShaderVertexInput, VertexFormat,
};

/// Metadata response JSON.
#[derive(Debug, serde::Serialize)]
pub(in crate::compat::ffi) struct MetadataJson<'program> {
    /// Combo values.
    combos: Vec<ComboJson<'program>>,
    /// Material aliases.
    aliases: Vec<AliasJson<'program>>,
    /// Default uniform values.
    default_uniforms: Vec<DefaultUniformJson<'program>>,
    /// Default texture values.
    default_textures: Vec<DefaultTextureJson<'program>>,
    /// Active texture slots.
    active_texture_slots: Vec<u8>,
}

impl<'program> From<&'program ShaderMetadata> for MetadataJson<'program> {
    fn from(metadata: &'program ShaderMetadata) -> Self {
        Self {
            combos: metadata.combos().iter().map(ComboJson::from).collect(),
            aliases: metadata.aliases().iter().map(AliasJson::from).collect(),
            default_uniforms: metadata
                .default_uniforms()
                .iter()
                .map(DefaultUniformJson::from)
                .collect(),
            default_textures: metadata
                .default_textures()
                .iter()
                .map(DefaultTextureJson::from)
                .collect(),
            active_texture_slots: metadata
                .active_texture_slots()
                .iter()
                .map(|slot| slot.index())
                .collect(),
        }
    }
}

/// Combo response JSON.
#[derive(Debug, serde::Serialize)]
struct ComboJson<'program> {
    /// Combo name.
    name: &'program str,
    /// Combo value.
    value: &'program str,
}

impl<'program> From<&'program ShaderComboValue> for ComboJson<'program> {
    fn from(combo: &'program ShaderComboValue) -> Self {
        Self {
            name: combo.name().as_str(),
            value: combo.value(),
        }
    }
}

/// Alias response JSON.
#[derive(Debug, serde::Serialize)]
struct AliasJson<'program> {
    /// Material property name.
    material: &'program str,
    /// Uniform name.
    uniform: &'program str,
}

impl<'program> From<&'program MaterialAlias> for AliasJson<'program> {
    fn from(alias: &'program MaterialAlias) -> Self {
        Self {
            material: alias.material(),
            uniform: alias.uniform(),
        }
    }
}

/// Default uniform response JSON.
#[derive(Debug, serde::Serialize)]
struct DefaultUniformJson<'program> {
    /// Uniform name.
    name: &'program str,
    /// Default value.
    value: PropertyValueJson<'program>,
}

impl<'program> From<&'program DefaultUniformValue> for DefaultUniformJson<'program> {
    fn from(uniform: &'program DefaultUniformValue) -> Self {
        Self {
            name: uniform.uniform(),
            value: PropertyValueJson::from(uniform.value()),
        }
    }
}

/// Default texture response JSON.
#[derive(Debug, serde::Serialize)]
struct DefaultTextureJson<'program> {
    /// Texture slot.
    slot: u8,
    /// Texture path.
    path: &'program str,
}

impl<'program> From<&'program DefaultTextureValue> for DefaultTextureJson<'program> {
    fn from(texture: &'program DefaultTextureValue) -> Self {
        Self {
            slot: texture.slot().index(),
            path: texture.path(),
        }
    }
}

/// Property value response JSON.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum PropertyValueJson<'program> {
    /// String value.
    String(&'program str),
    /// Number value.
    Number(f32),
    /// Boolean value.
    Bool(bool),
    /// Three-component vector value.
    Vec3([f32; 3]),
    /// Missing value.
    None,
}

impl<'program> From<&'program PropertyValue> for PropertyValueJson<'program> {
    fn from(value: &'program PropertyValue) -> Self {
        match value {
            PropertyValue::String(value) => Self::String(value),
            PropertyValue::Number(value) => Self::Number(*value),
            PropertyValue::Bool(value) => Self::Bool(*value),
            PropertyValue::Vec3(value) => Self::Vec3(*value),
            PropertyValue::None => Self::None,
        }
    }
}

/// Reflection response JSON.
#[derive(Debug, serde::Serialize)]
pub(in crate::compat::ffi) struct ReflectionJson<'program> {
    /// Descriptor bindings.
    descriptor_bindings: Vec<DescriptorBindingJson<'program>>,
    /// Uniform blocks.
    uniform_blocks: Vec<UniformBlockJson<'program>>,
    /// Vertex inputs.
    vertex_inputs: Vec<VertexInputJson<'program>>,
    /// Active texture slots.
    active_texture_slots: Vec<u8>,
}

impl<'program> From<&'program ShaderReflection> for ReflectionJson<'program> {
    fn from(reflection: &'program ShaderReflection) -> Self {
        Self {
            descriptor_bindings: reflection
                .descriptor_bindings()
                .iter()
                .map(DescriptorBindingJson::from)
                .collect(),
            uniform_blocks: reflection
                .uniform_blocks()
                .iter()
                .map(UniformBlockJson::from)
                .collect(),
            vertex_inputs: reflection
                .vertex_inputs()
                .iter()
                .map(VertexInputJson::from)
                .collect(),
            active_texture_slots: reflection
                .active_texture_slots()
                .iter()
                .map(|slot| slot.index())
                .collect(),
        }
    }
}

/// Descriptor binding response JSON.
#[derive(Debug, serde::Serialize)]
struct DescriptorBindingJson<'program> {
    /// Descriptor name.
    name: &'program str,
    /// Descriptor set.
    set: u32,
    /// Descriptor binding.
    binding: u32,
    /// Descriptor kind.
    descriptor: DescriptorKindJson,
    /// Stage mask.
    stages: StageMaskJson,
    /// Descriptor count.
    count: u32,
}

impl<'program> From<&'program ShaderDescriptorBinding> for DescriptorBindingJson<'program> {
    fn from(binding: &'program ShaderDescriptorBinding) -> Self {
        Self {
            name: binding.name(),
            set: binding.set().set(),
            binding: binding.binding().binding(),
            descriptor: DescriptorKindJson::from(binding.kind()),
            stages: StageMaskJson::from(binding.stages()),
            count: binding.count(),
        }
    }
}

/// Descriptor kind response JSON.
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DescriptorKindJson {
    /// Uniform buffer.
    UniformBuffer,
    /// Sampled image.
    SampledImage,
    /// Combined image sampler.
    CombinedImageSampler,
    /// Standalone sampler.
    Sampler,
}

impl From<crate::ShaderDescriptorKind> for DescriptorKindJson {
    fn from(kind: crate::ShaderDescriptorKind) -> Self {
        match kind {
            crate::ShaderDescriptorKind::UniformBuffer => Self::UniformBuffer,
            crate::ShaderDescriptorKind::SampledImage => Self::SampledImage,
            crate::ShaderDescriptorKind::CombinedImageSampler => Self::CombinedImageSampler,
            crate::ShaderDescriptorKind::Sampler => Self::Sampler,
        }
    }
}

/// Stage mask response JSON.
#[derive(Clone, Copy, Debug)]
struct StageMaskJson {
    /// Stage mask.
    mask: ShaderStageMask,
}

impl From<ShaderStageMask> for StageMaskJson {
    fn from(mask: ShaderStageMask) -> Self {
        Self { mask }
    }
}

impl serde::Serialize for StageMaskJson {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let count = usize::from(self.mask.vertex()) + usize::from(self.mask.fragment());
        let mut sequence = serializer.serialize_seq(Some(count))?;
        if self.mask.vertex() {
            sequence.serialize_element("vertex")?;
        }
        if self.mask.fragment() {
            sequence.serialize_element("fragment")?;
        }
        sequence.end()
    }
}

/// Uniform block response JSON.
#[derive(Debug, serde::Serialize)]
struct UniformBlockJson<'program> {
    /// Block name.
    name: &'program str,
    /// Descriptor set.
    set: u32,
    /// Descriptor binding.
    binding: u32,
    /// Block byte size.
    size: u32,
    /// Uniform members.
    members: Vec<UniformMemberJson<'program>>,
}

impl<'program> From<&'program ShaderUniformBlock> for UniformBlockJson<'program> {
    fn from(block: &'program ShaderUniformBlock) -> Self {
        Self {
            name: block.name(),
            set: block.set().set(),
            binding: block.binding().binding(),
            size: block.byte_size(),
            members: block
                .members()
                .iter()
                .map(UniformMemberJson::from)
                .collect(),
        }
    }
}

/// Uniform member response JSON.
#[derive(Debug, serde::Serialize)]
struct UniformMemberJson<'program> {
    /// Member name.
    name: &'program str,
    /// Member byte offset.
    offset: u32,
    /// Member byte size.
    size: u32,
    /// Scalar element count.
    element_count: u32,
    /// Array count.
    array_count: u32,
    /// Array stride.
    array_stride: u32,
}

impl<'program> From<&'program ShaderUniformMember> for UniformMemberJson<'program> {
    fn from(member: &'program ShaderUniformMember) -> Self {
        Self {
            name: member.name(),
            offset: member.offset(),
            size: member.byte_size(),
            element_count: member.element_count(),
            array_count: member.array_count(),
            array_stride: member.array_stride(),
        }
    }
}

/// Vertex input response JSON.
#[derive(Debug, serde::Serialize)]
struct VertexInputJson<'program> {
    /// Input name.
    name: &'program str,
    /// Input location.
    location: u32,
    /// Input format.
    format: &'static str,
}

impl<'program> From<&'program ShaderVertexInput> for VertexInputJson<'program> {
    fn from(input: &'program ShaderVertexInput) -> Self {
        Self {
            name: input.name(),
            location: input.location().index(),
            format: match input.format() {
                VertexFormat::R32Sfloat => "r32_sfloat",
                VertexFormat::R32G32Sfloat => "r32g32_sfloat",
                VertexFormat::R32G32B32Sfloat => "r32g32b32_sfloat",
                VertexFormat::R32G32B32A32Sfloat => "r32g32b32a32_sfloat",
                VertexFormat::R32Uint => "r32_uint",
                VertexFormat::R32G32Uint => "r32g32_uint",
                VertexFormat::R32G32B32Uint => "r32g32b32_uint",
                VertexFormat::R32G32B32A32Uint => "r32g32b32a32_uint",
                VertexFormat::R32Sint => "r32_sint",
                VertexFormat::R32G32Sint => "r32g32_sint",
                VertexFormat::R32G32B32Sint => "r32g32b32_sint",
                VertexFormat::R32G32B32A32Sint => "r32g32b32a32_sint",
            },
        }
    }
}

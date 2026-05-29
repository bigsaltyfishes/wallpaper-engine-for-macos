#[cfg(feature = "serde")]
use std::fmt;

use super::{
    BindingIndex, BindingSet, LocationIndex, ShaderStageKind, ShaderSymbolName, TextureSlot,
};
use crate::ShaderResult;

/// Renderer-neutral reflection data for a shader stage or merged program.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderReflection {
    /// Reflected descriptor bindings.
    descriptor_bindings: Box<[ShaderDescriptorBinding]>,
    /// Reflected uniform blocks.
    uniform_blocks: Box<[ShaderUniformBlock]>,
    /// Reflected vertex input declarations.
    vertex_inputs: Box<[ShaderVertexInput]>,
    /// Texture slots proven active by reflection.
    active_texture_slots: Box<[TextureSlot]>,
}

impl ShaderReflection {
    /// Creates an empty reflection result.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates reflection data from active texture slots.
    #[must_use]
    pub fn from_active_texture_slots(active_texture_slots: Box<[TextureSlot]>) -> Self {
        Self {
            active_texture_slots,
            ..Self::default()
        }
    }

    /// Creates reflection data from all reflected fields.
    #[must_use]
    pub fn new(
        descriptor_bindings: Box<[ShaderDescriptorBinding]>,
        uniform_blocks: Box<[ShaderUniformBlock]>,
        vertex_inputs: Box<[ShaderVertexInput]>,
        active_texture_slots: Box<[TextureSlot]>,
    ) -> Self {
        Self {
            descriptor_bindings,
            uniform_blocks,
            vertex_inputs,
            active_texture_slots,
        }
    }

    /// Returns reflected descriptor bindings.
    #[must_use]
    pub fn descriptor_bindings(&self) -> &[ShaderDescriptorBinding] {
        &self.descriptor_bindings
    }

    /// Returns reflected uniform blocks.
    #[must_use]
    pub fn uniform_blocks(&self) -> &[ShaderUniformBlock] {
        &self.uniform_blocks
    }

    /// Returns reflected vertex inputs.
    #[must_use]
    pub fn vertex_inputs(&self) -> &[ShaderVertexInput] {
        &self.vertex_inputs
    }

    /// Returns active texture slots.
    #[must_use]
    pub fn active_texture_slots(&self) -> &[TextureSlot] {
        &self.active_texture_slots
    }
}

/// Kind of a reflected descriptor binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ShaderDescriptorKind {
    /// Uniform buffer resource.
    UniformBuffer,
    /// Sampled image resource used with a separate sampler.
    SampledImage,
    /// Combined image sampler resource.
    CombinedImageSampler,
    /// Standalone sampler resource.
    Sampler,
}

/// Shader stages that use a reflected descriptor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShaderStageMask {
    /// Whether the vertex shader stage uses the descriptor.
    vertex: bool,
    /// Whether the fragment shader stage uses the descriptor.
    fragment: bool,
}

impl ShaderStageMask {
    /// Creates a stage mask from explicit stage usage.
    #[must_use]
    pub const fn new(vertex: bool, fragment: bool) -> Self {
        Self { vertex, fragment }
    }

    /// Creates a mask containing one shader stage.
    #[must_use]
    pub const fn single(stage: ShaderStageKind) -> Self {
        match stage {
            ShaderStageKind::Vertex => Self::new(true, false),
            ShaderStageKind::Fragment => Self::new(false, true),
        }
    }

    /// Returns whether the vertex stage uses the descriptor.
    #[must_use]
    pub const fn vertex(self) -> bool {
        self.vertex
    }

    /// Returns whether the fragment stage uses the descriptor.
    #[must_use]
    pub const fn fragment(self) -> bool {
        self.fragment
    }

    /// Returns a mask containing stages from both operands.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self::new(self.vertex || other.vertex, self.fragment || other.fragment)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ShaderStageMask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut stages = serializer
            .serialize_seq(Some(usize::from(self.vertex) + usize::from(self.fragment)))?;
        if self.vertex {
            stages.serialize_element("vertex")?;
        }
        if self.fragment {
            stages.serialize_element("fragment")?;
        }
        stages.end()
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ShaderStageMask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(ShaderStageMaskVisitor)
    }
}

/// Serde visitor for reflection stage masks encoded as stage name arrays.
#[cfg(feature = "serde")]
struct ShaderStageMaskVisitor;

#[cfg(feature = "serde")]
impl<'de> serde::de::Visitor<'de> for ShaderStageMaskVisitor {
    type Value = ShaderStageMask;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a sequence containing vertex and/or fragment stage names")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut mask = ShaderStageMask::default();

        while let Some(stage) = sequence.next_element::<String>()? {
            match stage.as_str() {
                "vertex" => mask.vertex = true,
                "fragment" => mask.fragment = true,
                _ => {
                    return Err(serde::de::Error::unknown_variant(
                        &stage,
                        &["vertex", "fragment"],
                    ));
                }
            }
        }

        Ok(mask)
    }
}

/// Reflected descriptor binding.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderDescriptorBinding {
    /// Resource name from the shader module.
    name: ShaderSymbolName,
    /// Descriptor set.
    set: BindingSet,
    /// Descriptor binding index.
    binding: BindingIndex,
    /// Descriptor resource kind.
    #[cfg_attr(feature = "serde", serde(rename = "descriptor"))]
    kind: ShaderDescriptorKind,
    /// Shader stages using this binding.
    stages: ShaderStageMask,
    /// Descriptor array count.
    count: u32,
}

impl ShaderDescriptorBinding {
    /// Creates a reflected descriptor binding.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a valid shader symbol name.
    pub fn new(
        name: impl Into<String>,
        set: BindingSet,
        binding: BindingIndex,
        kind: ShaderDescriptorKind,
        stages: ShaderStageMask,
        count: u32,
    ) -> ShaderResult<Self> {
        Ok(Self {
            name: ShaderSymbolName::new(name)?,
            set,
            binding,
            kind,
            stages,
            count,
        })
    }

    /// Returns the resource name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the resource symbol name.
    #[must_use]
    pub const fn symbol_name(&self) -> &ShaderSymbolName {
        &self.name
    }

    /// Returns the descriptor set.
    #[must_use]
    pub const fn set(&self) -> BindingSet {
        self.set
    }

    /// Returns the descriptor binding index.
    #[must_use]
    pub const fn binding(&self) -> BindingIndex {
        self.binding
    }

    /// Returns the descriptor kind.
    #[must_use]
    pub const fn kind(&self) -> ShaderDescriptorKind {
        self.kind
    }

    /// Returns the shader stages using this binding.
    #[must_use]
    pub const fn stages(&self) -> ShaderStageMask {
        self.stages
    }

    /// Returns the descriptor array count.
    #[must_use]
    pub const fn count(&self) -> u32 {
        self.count
    }
}

/// Reflected uniform block.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderUniformBlock {
    /// Uniform block name.
    name: ShaderSymbolName,
    /// Descriptor set.
    set: BindingSet,
    /// Descriptor binding index.
    binding: BindingIndex,
    /// Byte size of the block.
    #[cfg_attr(feature = "serde", serde(rename = "size"))]
    byte_size: u32,
    /// Reflected struct members.
    members: Box<[ShaderUniformMember]>,
}

impl ShaderUniformBlock {
    /// Creates a reflected uniform block.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a valid shader symbol name.
    pub fn new(
        name: impl Into<String>,
        set: BindingSet,
        binding: BindingIndex,
        byte_size: u32,
        members: Box<[ShaderUniformMember]>,
    ) -> ShaderResult<Self> {
        Ok(Self {
            name: ShaderSymbolName::new(name)?,
            set,
            binding,
            byte_size,
            members,
        })
    }

    /// Returns the block name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the block symbol name.
    #[must_use]
    pub const fn symbol_name(&self) -> &ShaderSymbolName {
        &self.name
    }

    /// Returns the descriptor set.
    #[must_use]
    pub const fn set(&self) -> BindingSet {
        self.set
    }

    /// Returns the descriptor binding index.
    #[must_use]
    pub const fn binding(&self) -> BindingIndex {
        self.binding
    }

    /// Returns the byte size.
    #[must_use]
    pub const fn byte_size(&self) -> u32 {
        self.byte_size
    }

    /// Returns reflected members.
    #[must_use]
    pub fn members(&self) -> &[ShaderUniformMember] {
        &self.members
    }
}

/// Reflected uniform block member.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderUniformMember {
    /// Member name.
    name: ShaderSymbolName,
    /// Byte offset from the start of the block.
    offset: u32,
    /// Byte size of the member type when known.
    #[cfg_attr(feature = "serde", serde(rename = "size"))]
    byte_size: u32,
    /// Scalar element count for vectors and matrices.
    element_count: u32,
    /// Constant array element count.
    array_count: u32,
    /// Byte stride between array elements.
    array_stride: u32,
}

impl ShaderUniformMember {
    /// Creates a reflected uniform block member.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a valid shader symbol name.
    pub fn new(
        name: impl Into<String>,
        offset: u32,
        byte_size: u32,
        element_count: u32,
        array_count: u32,
        array_stride: u32,
    ) -> ShaderResult<Self> {
        Ok(Self {
            name: ShaderSymbolName::new(name)?,
            offset,
            byte_size,
            element_count,
            array_count,
            array_stride,
        })
    }

    /// Returns the member name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the member symbol name.
    #[must_use]
    pub const fn symbol_name(&self) -> &ShaderSymbolName {
        &self.name
    }

    /// Returns the byte offset.
    #[must_use]
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the byte size.
    #[must_use]
    pub const fn byte_size(&self) -> u32 {
        self.byte_size
    }

    /// Returns the scalar element count for vectors and matrices.
    #[must_use]
    pub const fn element_count(&self) -> u32 {
        self.element_count
    }

    /// Returns the constant array element count.
    #[must_use]
    pub const fn array_count(&self) -> u32 {
        self.array_count
    }

    /// Returns the byte stride between array elements.
    #[must_use]
    pub const fn array_stride(&self) -> u32 {
        self.array_stride
    }
}

/// Vertex attribute format inferred from a reflected entry-point input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VertexFormat {
    /// 32-bit float scalar.
    #[cfg_attr(feature = "serde", serde(rename = "r32_sfloat"))]
    R32Sfloat,
    /// Two 32-bit float components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32_sfloat"))]
    R32G32Sfloat,
    /// Three 32-bit float components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32_sfloat"))]
    R32G32B32Sfloat,
    /// Four 32-bit float components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32a32_sfloat"))]
    R32G32B32A32Sfloat,
    /// 32-bit unsigned integer scalar.
    #[cfg_attr(feature = "serde", serde(rename = "r32_uint"))]
    R32Uint,
    /// Two 32-bit unsigned integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32_uint"))]
    R32G32Uint,
    /// Three 32-bit unsigned integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32_uint"))]
    R32G32B32Uint,
    /// Four 32-bit unsigned integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32a32_uint"))]
    R32G32B32A32Uint,
    /// 32-bit signed integer scalar.
    #[cfg_attr(feature = "serde", serde(rename = "r32_sint"))]
    R32Sint,
    /// Two 32-bit signed integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32_sint"))]
    R32G32Sint,
    /// Three 32-bit signed integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32_sint"))]
    R32G32B32Sint,
    /// Four 32-bit signed integer components.
    #[cfg_attr(feature = "serde", serde(rename = "r32g32b32a32_sint"))]
    R32G32B32A32Sint,
}

/// Reflected vertex input.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderVertexInput {
    /// Input name.
    name: ShaderSymbolName,
    /// Input location.
    location: LocationIndex,
    /// Inferred renderer-neutral input format.
    format: VertexFormat,
}

impl ShaderVertexInput {
    /// Creates a reflected vertex input.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a valid shader symbol name.
    pub fn new(
        name: impl Into<String>,
        location: LocationIndex,
        format: VertexFormat,
    ) -> ShaderResult<Self> {
        Ok(Self {
            name: ShaderSymbolName::new(name)?,
            location,
            format,
        })
    }

    /// Returns the input name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the input symbol name.
    #[must_use]
    pub const fn symbol_name(&self) -> &ShaderSymbolName {
        &self.name
    }

    /// Returns the input location.
    #[must_use]
    pub const fn location(&self) -> LocationIndex {
        self.location
    }

    /// Returns the inferred vertex input format.
    #[must_use]
    pub const fn format(&self) -> VertexFormat {
        self.format
    }
}

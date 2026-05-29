//! Bridge request JSON DTOs.

use crate::{
    ComboName, ProjectPropertyBinding, PropertyName, PropertyValue, ShaderCachePolicy,
    ShaderComboValue, ShaderName, ShaderProgramRequest, ShaderStageKind, ShaderStageSource,
    ShaderTarget, ShaderTextureInfo, TextureComponentState, TextureFormatHint, TextureSlot,
};

/// Bridge request DTO.
#[derive(Debug, serde::Deserialize)]
pub(in crate::compat::ffi) struct RequestDto {
    /// Shader program name.
    shader_name: String,
    /// Requested target backend.
    target: Option<TargetDto>,
    /// Cache behavior.
    cache_policy: Option<CachePolicyDto>,
    /// Stage source DTOs.
    stages: Vec<StageSourceDto>,
    /// Combo DTOs.
    #[serde(default)]
    combos: Vec<ComboDto>,
    /// Texture DTOs.
    #[serde(default)]
    textures: Vec<TextureDto>,
    /// Project property DTOs.
    #[serde(default)]
    properties: Vec<PropertyDto>,
}

impl RequestDto {
    /// Converts the DTO into a typed shader request.
    pub(in crate::compat::ffi) fn into_request(self) -> crate::ShaderResult<ShaderProgramRequest> {
        let mut builder = ShaderProgramRequest::builder(ShaderName::new(self.shader_name)?)
            .target(self.target.unwrap_or_default().into())
            .cache_policy(self.cache_policy.unwrap_or_default().into());

        for stage in self.stages {
            builder = builder.stage(stage.into_stage_source());
        }
        for combo in self.combos {
            builder = builder.combo(combo.into_combo()?);
        }
        for texture in self.textures {
            builder = builder.texture(texture.into_texture()?);
        }
        for property in self.properties {
            builder = builder.property(property.into_binding()?);
        }

        builder.build()
    }
}

/// Shader target DTO.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum TargetDto {
    /// Vulkan SPIR-V target.
    #[default]
    VulkanSpirv,
}

impl From<TargetDto> for ShaderTarget {
    fn from(target: TargetDto) -> Self {
        match target {
            TargetDto::VulkanSpirv => Self::VulkanSpirv,
        }
    }
}

/// Cache policy DTO.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum CachePolicyDto {
    /// Disabled shader cache.
    #[default]
    Disabled,
    /// Enabled shader cache.
    Enabled {
        /// Scene id used in cache keys.
        scene_id: String,
    },
}

impl From<CachePolicyDto> for ShaderCachePolicy {
    fn from(policy: CachePolicyDto) -> Self {
        match policy {
            CachePolicyDto::Disabled => Self::Disabled,
            CachePolicyDto::Enabled { scene_id } => Self::Enabled { scene_id },
        }
    }
}

/// Stage source DTO.
#[derive(Debug, serde::Deserialize)]
struct StageSourceDto {
    /// Stage kind.
    kind: StageKindDto,
    /// Raw source text.
    source: String,
}

impl StageSourceDto {
    /// Converts this DTO into a stage source.
    fn into_stage_source(self) -> ShaderStageSource {
        ShaderStageSource::new(self.kind.into(), self.source)
    }
}

/// Stage kind DTO.
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum StageKindDto {
    /// Vertex stage.
    Vertex,
    /// Fragment stage.
    Fragment,
}

impl From<StageKindDto> for ShaderStageKind {
    fn from(stage: StageKindDto) -> Self {
        match stage {
            StageKindDto::Vertex => Self::Vertex,
            StageKindDto::Fragment => Self::Fragment,
        }
    }
}

/// Combo DTO.
#[derive(Debug, serde::Deserialize)]
struct ComboDto {
    /// Combo name.
    name: String,
    /// Combo value.
    value: String,
}

impl ComboDto {
    /// Converts this DTO into a typed combo.
    fn into_combo(self) -> crate::ShaderResult<ShaderComboValue> {
        Ok(ShaderComboValue::new(
            ComboName::new(self.name)?,
            self.value,
        ))
    }
}

/// Texture DTO.
#[derive(Debug, serde::Deserialize)]
struct TextureDto {
    /// Texture slot.
    slot: u8,
    /// Whether this slot has a material texture resource.
    #[serde(default = "default_texture_present")]
    present: bool,
    /// Whether this texture is enabled.
    #[serde(alias = "enabled")]
    is_enabled: bool,
    /// Texture format.
    #[serde(default)]
    format: TextureFormatDto,
    /// Component state.
    #[serde(default)]
    components: TextureComponentsDto,
}

impl TextureDto {
    /// Converts this DTO into typed texture info.
    fn into_texture(self) -> crate::ShaderResult<ShaderTextureInfo> {
        Ok(ShaderTextureInfo::with_presence(
            TextureSlot::new(self.slot)?,
            self.present,
            self.is_enabled,
            self.format.into(),
            self.components.into(),
        ))
    }
}

/// Default material texture presence for older bridge JSON.
const fn default_texture_present() -> bool {
    true
}

/// Texture format DTO.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum TextureFormatDto {
    /// Unknown format.
    #[default]
    Unknown,
    /// R8 format.
    R8,
    /// RG8 format.
    Rg8,
    /// RGBA8 format.
    Rgba8,
}

impl From<TextureFormatDto> for TextureFormatHint {
    fn from(format: TextureFormatDto) -> Self {
        match format {
            TextureFormatDto::Unknown => Self::Unknown,
            TextureFormatDto::R8 => Self::R8,
            TextureFormatDto::Rg8 => Self::Rg8,
            TextureFormatDto::Rgba8 => Self::Rgba8,
        }
    }
}

/// Texture component-state DTO.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
struct TextureComponentsDto {
    /// First component.
    #[serde(default)]
    compo1: bool,
    /// Second component.
    #[serde(default)]
    compo2: bool,
    /// Third component.
    #[serde(default)]
    compo3: bool,
}

impl From<TextureComponentsDto> for [TextureComponentState; 3] {
    fn from(components: TextureComponentsDto) -> Self {
        [
            TextureComponentState::new(components.compo1),
            TextureComponentState::new(components.compo2),
            TextureComponentState::new(components.compo3),
        ]
    }
}

/// Project property DTO.
#[derive(Debug, serde::Deserialize)]
struct PropertyDto {
    /// Project property name.
    name: String,
    /// Project property value.
    value: PropertyValueDto,
}

impl PropertyDto {
    /// Converts this DTO into a typed property binding.
    fn into_binding(self) -> crate::ShaderResult<ProjectPropertyBinding> {
        Ok(ProjectPropertyBinding::new(
            PropertyName::new(self.name)?,
            self.value.into(),
        ))
    }
}

/// Project property value DTO.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum PropertyValueDto {
    /// String value.
    String(String),
    /// Number value.
    Number(f32),
    /// Boolean value.
    Bool(bool),
    /// Three-component vector value.
    Vec3([f32; 3]),
    /// Missing value.
    None,
}

impl From<PropertyValueDto> for PropertyValue {
    fn from(value: PropertyValueDto) -> Self {
        match value {
            PropertyValueDto::String(value) => Self::String(value),
            PropertyValueDto::Number(value) => Self::Number(value),
            PropertyValueDto::Bool(value) => Self::Bool(value),
            PropertyValueDto::Vec3(value) => Self::Vec3(value),
            PropertyValueDto::None => Self::None,
        }
    }
}

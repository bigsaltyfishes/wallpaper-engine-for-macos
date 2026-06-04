use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Display,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
    DisplayDesc, EngineError,
    display::state::{DisplayKey, DisplayStateModel},
    media::audio::AudioVolume,
    project::{SceneSourceResolution, validate_relative_normal_path},
    render::{ShaderCacheDecision, ShaderCacheInputs},
};

const DEFAULT_RENDER_TARGET: &str = "_rt_default";

#[derive(Clone, Debug, PartialEq)]
pub struct SceneFile {
    objects: Vec<Value>,
    scene: SceneIr,
}

impl SceneFile {
    /// Parses a scene JSON document without loading referenced assets.
    ///
    /// # Errors
    ///
    /// Returns an error if the document is malformed or references invalid
    /// relative asset paths.
    pub fn parse<T: AsRef<str>>(json: T) -> Result<Self, crate::EngineError> {
        Self::parse_with_assets(json, |_| Ok(None))
    }

    /// Parses a scene JSON document and validates loadable asset references.
    ///
    /// # Errors
    ///
    /// Returns an error if the document is malformed, an asset path is invalid,
    /// the loader fails, or a loaded asset is not valid JSON.
    pub fn parse_with_assets<T, F>(json: T, mut loader: F) -> Result<Self, crate::EngineError>
    where
        T: AsRef<str>,
        F: FnMut(&str) -> Result<Option<String>, crate::EngineError>,
    {
        let raw: Value = serde_json::from_str(json.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to parse scene file: {error}"))
        })?;
        let object = raw.as_object().ok_or_else(|| {
            crate::EngineError::InvalidInput("scene file root must be an object".to_string())
        })?;

        let objects = object
            .get("objects")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let asset_paths = objects.iter().flat_map(|object| {
            ["image", "particle", "sound", "light"]
                .into_iter()
                .filter_map(move |key| object.get(key).and_then(Value::as_str))
        });
        for asset_path in asset_paths {
            validate_relative_normal_path(Path::new(asset_path), "scene asset path")?;
            if let Some(asset_json) = loader(asset_path)? {
                serde_json::from_str::<Value>(&asset_json).map_err(|error| {
                    crate::EngineError::InvalidInput(format!(
                        "failed to parse scene asset `{asset_path}`: {error}"
                    ))
                })?;
            }
        }

        let scene = SceneIr::from(&raw);
        Ok(Self { objects, scene })
    }

    /// Loads a scene file from a project or direct scene path.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be resolved, files cannot be
    /// read, or the scene/asset JSON is invalid.
    pub fn load_project<P, A>(project_path: P, assets_path: A) -> Result<Self, crate::EngineError>
    where
        P: AsRef<Path>,
        A: AsRef<Path>,
    {
        let project_path = project_path.as_ref();
        let assets_path = assets_path.as_ref();

        if project_path.file_name().and_then(|name| name.to_str()) != Some("project.json") {
            let content = fs::read_to_string(project_path).map_err(|error| {
                crate::EngineError::InvalidInput(format!("failed to read scene file: {error}"))
            })?;
            return Self::parse_with_assets(content, |asset| {
                read_optional_asset(
                    project_path.parent().unwrap_or_else(|| Path::new("")),
                    assets_path,
                    asset,
                )
            });
        }

        let resolution = SceneSourceResolution::load(project_path)?;
        let Some(scene_source) = resolution.scene_source() else {
            return Err(crate::EngineError::InvalidInput(
                "project is not a scene wallpaper".to_string(),
            ));
        };
        let scene_entry = Path::new(&scene_source.pkg_entry);
        validate_relative_normal_path(scene_entry, "scene file")?;
        let scene_path = path_under_root(&scene_source.pkg_dir, scene_entry, "scene file")?;
        let content = fs::read_to_string(&scene_path).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to read scene file: {error}"))
        })?;

        Self::parse_with_assets(content, |asset| {
            read_optional_asset(&scene_source.pkg_dir, assets_path, asset)
        })
    }

    #[must_use]
    pub fn objects(&self) -> &[Value] {
        &self.objects
    }

    #[must_use]
    pub fn scene(&self) -> &SceneIr {
        &self.scene
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneIr {
    render_targets: BTreeMap<String, SceneRenderTargetIr>,
}

impl SceneIr {
    #[must_use]
    pub fn render_targets(&self) -> &BTreeMap<String, SceneRenderTargetIr> {
        &self.render_targets
    }
}

impl From<&Value> for SceneIr {
    fn from(value: &Value) -> Self {
        let (width, height) = value
            .get("general")
            .and_then(|general| general.get("orthogonalprojection"))
            .map_or((1920, 1080), |value| {
                if value.is_null() {
                    return (1920, 1080);
                }

                let width = value
                    .get("width")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(1920);
                let height = value
                    .get("height")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(1080);

                (width, height)
            });

        let mut render_targets = BTreeMap::new();
        render_targets.insert(
            DEFAULT_RENDER_TARGET.to_string(),
            SceneRenderTargetIr { width, height },
        );

        Self { render_targets }
    }
}

/// Opaque identifier returned by the renderer for an open scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneHandle(u64);

impl SceneHandle {
    /// Wraps a raw renderer handle.
    #[must_use]
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw renderer handle value.
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// How wallpaper content is mapped into the target display rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScalingMode {
    /// Do not scale; render at source size.
    None,
    /// Stretch content to exactly match the output rectangle.
    Stretch,
    /// Preserve aspect ratio and fit all content inside the output rectangle.
    #[default]
    Fit,
    /// Preserve aspect ratio and fill the output rectangle, cropping overflow.
    Fill,
}

impl Display for ScalingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ScalingMode::None => "none",
            ScalingMode::Stretch => "stretch",
            ScalingMode::Fit => "fit",
            ScalingMode::Fill => "fill",
        };
        write!(f, "{name}")
    }
}

/// Desired wallpaper scene configuration for one display.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDesc {
    /// Display where the scene should be shown.
    pub display: DisplayDesc,
    /// Path to the workshop `project.json` or scene source file.
    pub scene_path: String,
    /// Path to Wallpaper Engine's shared assets directory.
    pub assets_path: String,
    /// Target frame rate requested from the renderer.
    pub fps: u32,
    /// Initial wallpaper scaling mode.
    pub scaling_mode: ScalingMode,
    /// Initial positive wallpaper scaling factor.
    pub scaling_factor: f64,
    /// Whether the final presentation should be mirrored left-to-right.
    pub horizontal_flip: bool,
    /// Whether the renderer should start in a paused state.
    pub paused: bool,
    /// Whether scene audio-response properties should be enabled.
    pub audio_response_enabled: bool,
    /// Scene-wide audio volume multiplier.
    pub audio_volume: AudioVolume,
    /// Scene-wide audio mute flag.
    pub audio_muted: bool,
    /// Optional project property override JSON before flattening.
    pub property_override_json: Option<String>,
    /// Optional shader cache root for this scene.
    pub shader_cache_path: Option<String>,
    /// Whether shader cache entries should be regenerated.
    pub force_shader_refresh: bool,
}

impl SceneDesc {
    /// Starts building a scene descriptor with conservative defaults.
    ///
    /// # Panics
    ///
    /// Panics only if the built-in default audio volume falls outside the valid
    /// [`AudioVolume`] range.
    #[must_use]
    pub fn builder(display: DisplayDesc, scene_path: impl Into<String>) -> SceneDescBuilder {
        SceneDescBuilder {
            display,
            scene_path: scene_path.into(),
            assets_path: String::new(),
            fps: 60,
            scaling_mode: ScalingMode::default(),
            scaling_factor: 1.0,
            horizontal_flip: false,
            paused: false,
            audio_response_enabled: false,
            audio_volume: AudioVolume::try_from(1.0).unwrap(),
            audio_muted: false,
            property_override_json: None,
            shader_cache_path: None,
            force_shader_refresh: false,
        }
    }

    /// Constructs a scene descriptor without validation.
    ///
    /// Prefer [`SceneDesc::builder`] for new call sites that should validate
    /// required fields before handing the descriptor to the renderer.
    ///
    /// # Panics
    ///
    /// Panics only if the built-in default audio volume falls outside the valid
    /// [`AudioVolume`] range.
    #[must_use]
    pub fn new(
        display: DisplayDesc,
        scene_path: impl Into<String>,
        assets_path: impl Into<String>,
        fps: u32,
        paused: bool,
    ) -> Self {
        Self {
            display,
            scene_path: scene_path.into(),
            assets_path: assets_path.into(),
            fps,
            scaling_mode: ScalingMode::default(),
            scaling_factor: 1.0,
            horizontal_flip: false,
            paused,
            audio_response_enabled: false,
            audio_volume: AudioVolume::try_from(1.0).unwrap(),
            audio_muted: false,
            property_override_json: None,
            shader_cache_path: None,
            force_shader_refresh: false,
        }
    }

    /// Validates that the descriptor can be handed to the renderer.
    ///
    /// # Errors
    ///
    /// Returns an error if required display, path, FPS, or scaling fields are
    /// invalid.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.display.display_id == 0 {
            return Err(EngineError::InvalidInput(
                "display_id must be non-zero".to_string(),
            ));
        }
        if self.display.width == 0 || self.display.height == 0 {
            return Err(EngineError::InvalidInput(
                "display dimensions must be non-zero".to_string(),
            ));
        }
        if self.scene_path.is_empty() {
            return Err(EngineError::InvalidInput(
                "scene_path must not be empty".to_string(),
            ));
        }
        if self.fps == 0 {
            return Err(EngineError::InvalidInput(
                "fps must be greater than zero".to_string(),
            ));
        }
        if !valid_scaling_factor(self.scaling_factor) {
            return Err(EngineError::InvalidInput(
                "scaling factor must be finite and greater than zero".to_string(),
            ));
        }
        Ok(())
    }

    /// Sets the initial audio-response state.
    #[must_use]
    pub fn with_audio_response_enabled(mut self, enabled: bool) -> Self {
        self.audio_response_enabled = enabled;
        self
    }

    /// Sets the initial scaling mode.
    #[must_use]
    pub fn with_scaling_mode(mut self, scaling_mode: ScalingMode) -> Self {
        self.scaling_mode = scaling_mode;
        self
    }

    /// Sets the initial scaling factor.
    #[must_use]
    pub fn with_scaling_factor(mut self, scaling_factor: f64) -> Self {
        self.scaling_factor = scaling_factor;
        self
    }

    /// Sets the scene-wide audio volume multiplier.
    ///
    /// # Panics
    ///
    /// Panics if `audio_volume` is outside the valid [`AudioVolume`] range.
    #[must_use]
    pub fn with_audio_volume(mut self, audio_volume: f32) -> Self {
        self.audio_volume = AudioVolume::try_from(audio_volume).unwrap();
        self
    }

    /// Sets the scene-wide audio mute flag.
    #[must_use]
    pub fn with_audio_muted(mut self, audio_muted: bool) -> Self {
        self.audio_muted = audio_muted;
        self
    }

    /// Sets project property override JSON for the scene.
    #[must_use]
    pub fn with_property_override_json(
        mut self,
        property_override_json: impl Into<String>,
    ) -> Self {
        self.property_override_json = Some(property_override_json.into());
        self
    }

    /// Sets the shader cache root for the scene.
    #[must_use]
    pub fn with_shader_cache_path(mut self, shader_cache_path: impl Into<String>) -> Self {
        self.shader_cache_path = Some(shader_cache_path.into());
        self
    }

    /// Controls whether shader cache entries should be regenerated.
    #[must_use]
    pub fn with_force_shader_refresh(mut self, force_shader_refresh: bool) -> Self {
        self.force_shader_refresh = force_shader_refresh;
        self
    }

    /// Resolves the effective shader cache directory for this scene.
    ///
    /// # Errors
    ///
    /// Returns an error if the project source cannot be resolved or shader
    /// cache inputs cannot be validated.
    pub fn shader_cache_path(&self) -> Result<Option<String>, EngineError> {
        let Some(cache_root) = self.shader_cache_path.as_deref() else {
            return Ok(None);
        };
        let scene_path = Path::new(&self.scene_path);
        let resolution = SceneSourceResolution::load(scene_path)?;
        let Some(scene_source) = resolution.scene_source() else {
            return Ok(None);
        };
        let inputs = ShaderCacheInputs::builder(&scene_source.scene_id, cache_root)
            .project_json_path(scene_path)
            .scene_pkg_path(&scene_source.pkg_path)
            .property_override_json(self.property_override_json.clone())
            .force_refresh(self.force_shader_refresh)
            .build()?;
        let decision = ShaderCacheDecision::prepare(inputs)?;
        Ok(Some(
            decision.scene_cache_path().to_string_lossy().into_owned(),
        ))
    }

    /// Returns true iff all wallpaper-defining fields are equal, intentionally
    /// ignoring `display` and the initial `paused` seed. Used to detect whether
    /// the scene descriptor changed in a way that requires rebuilding the
    /// renderer rather than just moving the window to a new display.
    #[must_use]
    pub fn same_wallpaper(&self, other: &SceneDesc) -> bool {
        self.scene_path == other.scene_path
            && self.assets_path == other.assets_path
            && self.fps == other.fps
            && self.scaling_mode == other.scaling_mode
            && (self.scaling_factor - other.scaling_factor).abs() <= f64::EPSILON
            && self.horizontal_flip == other.horizontal_flip
            && self.audio_response_enabled == other.audio_response_enabled
            && self.audio_volume == other.audio_volume
            && self.audio_muted == other.audio_muted
            && self.property_override_json == other.property_override_json
            && self.shader_cache_path == other.shader_cache_path
            && self.force_shader_refresh == other.force_shader_refresh
    }

    /// Marks a one-shot shader refresh request as consumed after the renderer
    /// has been opened or rebuilt with that descriptor.
    pub fn mark_shader_refresh_complete(&mut self) {
        self.force_shader_refresh = false;
    }
}

/// Display-independent wallpaper scene configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTemplate {
    /// Path to the workshop `project.json` or scene source file.
    pub scene_path: String,
    /// Path to Wallpaper Engine's shared assets directory.
    pub assets_path: String,
    /// Target frame rate requested from the renderer.
    pub fps: u32,
    /// Initial wallpaper scaling mode.
    pub scaling_mode: ScalingMode,
    /// Initial positive wallpaper scaling factor.
    pub scaling_factor: f64,
    /// Whether the final presentation should be mirrored left-to-right.
    pub horizontal_flip: bool,
    /// Whether the renderer should start in a paused state.
    pub paused: bool,
    /// Whether scene audio-response properties should be enabled.
    pub audio_response_enabled: bool,
    /// Scene-wide audio volume multiplier.
    pub audio_volume: AudioVolume,
    /// Scene-wide audio mute flag.
    pub audio_muted: bool,
    /// Optional project property override JSON before flattening.
    pub property_override_json: Option<String>,
    /// Optional shader cache root for this scene.
    pub shader_cache_path: Option<String>,
    /// Whether shader cache entries should be regenerated.
    pub force_shader_refresh: bool,
}

impl SceneTemplate {
    /// Starts building a display-independent scene template.
    ///
    /// # Panics
    ///
    /// Panics only if the built-in default audio volume falls outside the valid
    /// [`AudioVolume`] range.
    #[must_use]
    pub fn builder(scene_path: impl Into<String>) -> SceneTemplateBuilder {
        SceneTemplateBuilder {
            scene_path: scene_path.into(),
            assets_path: String::new(),
            fps: 60,
            scaling_mode: ScalingMode::default(),
            scaling_factor: 1.0,
            horizontal_flip: false,
            paused: false,
            audio_response_enabled: false,
            audio_volume: AudioVolume::try_from(1.0).unwrap(),
            audio_muted: false,
            property_override_json: None,
            shader_cache_path: None,
            force_shader_refresh: false,
        }
    }

    /// Copies display-independent scene settings from a scene descriptor.
    #[must_use]
    pub fn from_scene_desc(scene: &SceneDesc) -> Self {
        Self {
            scene_path: scene.scene_path.clone(),
            assets_path: scene.assets_path.clone(),
            fps: scene.fps,
            scaling_mode: scene.scaling_mode,
            scaling_factor: scene.scaling_factor,
            horizontal_flip: scene.horizontal_flip,
            paused: scene.paused,
            audio_response_enabled: scene.audio_response_enabled,
            audio_volume: scene.audio_volume,
            audio_muted: scene.audio_muted,
            property_override_json: scene.property_override_json.clone(),
            shader_cache_path: scene.shader_cache_path.clone(),
            force_shader_refresh: scene.force_shader_refresh,
        }
    }

    /// Creates a concrete scene descriptor for one display.
    #[must_use]
    pub fn for_display(&self, display: DisplayDesc) -> SceneDesc {
        SceneDesc {
            display,
            scene_path: self.scene_path.clone(),
            assets_path: self.assets_path.clone(),
            fps: self.fps,
            scaling_mode: self.scaling_mode,
            scaling_factor: self.scaling_factor,
            horizontal_flip: self.horizontal_flip,
            paused: self.paused,
            audio_response_enabled: self.audio_response_enabled,
            audio_volume: self.audio_volume,
            audio_muted: self.audio_muted,
            property_override_json: self.property_override_json.clone(),
            shader_cache_path: self.shader_cache_path.clone(),
            force_shader_refresh: self.force_shader_refresh,
        }
    }

    /// Validates that the template can be applied to a display.
    ///
    /// # Errors
    ///
    /// Returns an error if required path, FPS, or scaling fields are invalid.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.scene_path.is_empty() {
            return Err(EngineError::InvalidInput(
                "scene_path must not be empty".to_string(),
            ));
        }
        if self.fps == 0 {
            return Err(EngineError::InvalidInput(
                "fps must be greater than zero".to_string(),
            ));
        }
        if !valid_scaling_factor(self.scaling_factor) {
            return Err(EngineError::InvalidInput(
                "scaling factor must be finite and greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Builder for [`SceneTemplate`].
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTemplateBuilder {
    scene_path: String,
    assets_path: String,
    fps: u32,
    scaling_mode: ScalingMode,
    scaling_factor: f64,
    horizontal_flip: bool,
    paused: bool,
    audio_response_enabled: bool,
    audio_volume: AudioVolume,
    audio_muted: bool,
    property_override_json: Option<String>,
    shader_cache_path: Option<String>,
    force_shader_refresh: bool,
}

impl SceneTemplateBuilder {
    /// Sets the shared assets path.
    #[must_use]
    pub fn assets_path(mut self, assets_path: impl Into<String>) -> Self {
        self.assets_path = assets_path.into();
        self
    }

    /// Sets the target frame rate.
    #[must_use]
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }

    /// Sets the initial scaling mode.
    #[must_use]
    pub fn scaling_mode(mut self, scaling_mode: ScalingMode) -> Self {
        self.scaling_mode = scaling_mode;
        self
    }

    /// Sets the initial scaling factor.
    #[must_use]
    pub fn scaling_factor(mut self, scaling_factor: f64) -> Self {
        self.scaling_factor = scaling_factor;
        self
    }

    /// Sets whether final presentation should be mirrored left-to-right.
    #[must_use]
    pub fn horizontal_flip(mut self, enabled: bool) -> Self {
        self.horizontal_flip = enabled;
        self
    }

    /// Sets the initial pause state.
    #[must_use]
    pub fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    /// Sets the initial audio-response state.
    #[must_use]
    pub fn audio_response_enabled(mut self, enabled: bool) -> Self {
        self.audio_response_enabled = enabled;
        self
    }

    /// Sets the scene-wide audio volume multiplier.
    ///
    /// # Panics
    ///
    /// Panics if `audio_volume` is outside the valid [`AudioVolume`] range.
    #[must_use]
    pub fn audio_volume(mut self, audio_volume: f32) -> Self {
        self.audio_volume = AudioVolume::try_from(audio_volume).unwrap();
        self
    }

    /// Sets the scene-wide audio mute flag.
    #[must_use]
    pub fn audio_muted(mut self, audio_muted: bool) -> Self {
        self.audio_muted = audio_muted;
        self
    }

    /// Sets project property override JSON.
    #[must_use]
    pub fn property_override_json(mut self, json: impl Into<String>) -> Self {
        self.property_override_json = Some(json.into());
        self
    }

    /// Sets the shader cache root.
    #[must_use]
    pub fn shader_cache_path(mut self, path: impl Into<String>) -> Self {
        self.shader_cache_path = Some(path.into());
        self
    }

    /// Controls whether shader cache entries should be regenerated.
    #[must_use]
    pub fn force_shader_refresh(mut self, refresh: bool) -> Self {
        self.force_shader_refresh = refresh;
        self
    }

    /// Validates and returns the scene template.
    pub fn build(self) -> Result<SceneTemplate, EngineError> {
        let template = SceneTemplate {
            scene_path: self.scene_path,
            assets_path: self.assets_path,
            fps: self.fps,
            scaling_mode: self.scaling_mode,
            scaling_factor: self.scaling_factor,
            horizontal_flip: self.horizontal_flip,
            paused: self.paused,
            audio_response_enabled: self.audio_response_enabled,
            audio_volume: self.audio_volume,
            audio_muted: self.audio_muted,
            property_override_json: self.property_override_json,
            shader_cache_path: self.shader_cache_path,
            force_shader_refresh: self.force_shader_refresh,
        };
        template.validate()?;
        Ok(template)
    }
}

pub trait SceneDescSliceExt {
    /// Asserts that the slice contains at most one scene per display.
    ///
    /// # Errors
    ///
    /// Returns an error if multiple scenes target the same display.
    fn assert_unique(&self) -> Result<(), EngineError>;

    /// Produces a [`SceneResult`] for each scene by looking up its handle in
    /// the provided map.
    ///
    /// # Errors
    ///
    /// Returns an error if a scene cannot be mapped to a display key or handle.
    fn reconcile_results(
        &self,
        display_state: &DisplayStateModel,
        handles_by_display: &HashMap<DisplayKey, SceneHandle>,
    ) -> Result<Vec<SceneResult>, EngineError>;
}

impl SceneDescSliceExt for [SceneDesc] {
    fn assert_unique(&self) -> Result<(), EngineError> {
        let mut display_ids = HashSet::with_capacity(self.len());
        for scene in self {
            if !display_ids.insert(scene.display.display_id) {
                return Err(EngineError::InvalidInput(format!(
                    "desired scene list contains duplicate display_id {}",
                    scene.display.display_id
                )));
            }
        }
        Ok(())
    }

    fn reconcile_results(
        &self,
        display_state: &DisplayStateModel,
        handles_by_display: &HashMap<DisplayKey, SceneHandle>,
    ) -> Result<Vec<SceneResult>, EngineError> {
        let mut results = Vec::with_capacity(self.len());
        for scene in self {
            let key = display_state.reconcile_key(scene)?;
            let handle = handles_by_display.get(&key).copied().ok_or_else(|| {
                EngineError::Platform(format!(
                    "missing scene handle for requested display {}",
                    scene.display.display_id
                ))
            })?;
            results.push(SceneResult::new(scene.display.display_id, handle, 0));
        }
        Ok(results)
    }
}

/// Builder for [`SceneDesc`].
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDescBuilder {
    display: DisplayDesc,
    scene_path: String,
    assets_path: String,
    fps: u32,
    scaling_mode: ScalingMode,
    scaling_factor: f64,
    horizontal_flip: bool,
    paused: bool,
    audio_response_enabled: bool,
    audio_volume: AudioVolume,
    audio_muted: bool,
    property_override_json: Option<String>,
    shader_cache_path: Option<String>,
    force_shader_refresh: bool,
}

impl SceneDescBuilder {
    /// Sets the shared assets path.
    #[must_use]
    pub fn assets_path(mut self, assets_path: impl Into<String>) -> Self {
        self.assets_path = assets_path.into();
        self
    }

    /// Sets the target frame rate.
    #[must_use]
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }

    /// Sets the initial scaling mode.
    #[must_use]
    pub fn scaling_mode(mut self, scaling_mode: ScalingMode) -> Self {
        self.scaling_mode = scaling_mode;
        self
    }

    /// Sets the initial scaling factor.
    #[must_use]
    pub fn scaling_factor(mut self, scaling_factor: f64) -> Self {
        self.scaling_factor = scaling_factor;
        self
    }

    /// Sets whether final presentation should be mirrored left-to-right.
    #[must_use]
    pub fn horizontal_flip(mut self, enabled: bool) -> Self {
        self.horizontal_flip = enabled;
        self
    }

    /// Sets the initial pause state.
    #[must_use]
    pub fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    /// Sets the initial audio-response state.
    #[must_use]
    pub fn audio_response_enabled(mut self, enabled: bool) -> Self {
        self.audio_response_enabled = enabled;
        self
    }

    /// Sets the scene-wide audio volume multiplier.
    ///
    /// # Panics
    ///
    /// Panics if `audio_volume` is outside the valid [`AudioVolume`] range.
    #[must_use]
    pub fn audio_volume(mut self, audio_volume: f32) -> Self {
        self.audio_volume = AudioVolume::try_from(audio_volume).unwrap();
        self
    }

    /// Sets the scene-wide audio mute flag.
    #[must_use]
    pub fn audio_muted(mut self, audio_muted: bool) -> Self {
        self.audio_muted = audio_muted;
        self
    }

    /// Sets project property override JSON.
    #[must_use]
    pub fn property_override_json(mut self, json: impl Into<String>) -> Self {
        self.property_override_json = Some(json.into());
        self
    }

    /// Sets the shader cache root.
    #[must_use]
    pub fn shader_cache_path(mut self, path: impl Into<String>) -> Self {
        self.shader_cache_path = Some(path.into());
        self
    }

    /// Controls whether shader cache entries should be regenerated.
    #[must_use]
    pub fn force_shader_refresh(mut self, refresh: bool) -> Self {
        self.force_shader_refresh = refresh;
        self
    }

    /// Validates and returns the scene descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error if required path, FPS, or scaling fields are invalid.
    pub fn build(self) -> Result<SceneDesc, EngineError> {
        if self.scene_path.is_empty() {
            return Err(EngineError::InvalidInput(
                "scene_path must not be empty".to_string(),
            ));
        }

        if self.fps == 0 {
            return Err(EngineError::InvalidInput(
                "fps must be greater than zero".to_string(),
            ));
        }
        if !valid_scaling_factor(self.scaling_factor) {
            return Err(EngineError::InvalidInput(
                "scaling factor must be finite and greater than zero".to_string(),
            ));
        }

        Ok(SceneDesc {
            display: self.display,
            scene_path: self.scene_path,
            assets_path: self.assets_path,
            fps: self.fps,
            scaling_mode: self.scaling_mode,
            scaling_factor: self.scaling_factor,
            horizontal_flip: self.horizontal_flip,
            paused: self.paused,
            audio_response_enabled: self.audio_response_enabled,
            audio_volume: self.audio_volume,
            audio_muted: self.audio_muted,
            property_override_json: self.property_override_json,
            shader_cache_path: self.shader_cache_path,
            force_shader_refresh: self.force_shader_refresh,
        })
    }
}

fn valid_scaling_factor(factor: f64) -> bool {
    factor.is_finite() && factor > 0.0
}

/// Result for one scene reconciliation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneResult {
    /// Display associated with the reconciled scene.
    pub display_id: u32,
    /// Renderer handle for the scene.
    pub handle: SceneHandle,
    /// Renderer status code. Zero means success.
    pub status: i32,
}

impl SceneResult {
    /// Constructs a reconciliation result from renderer values.
    #[must_use]
    pub fn new(display_id: u32, handle: SceneHandle, status: i32) -> Self {
        Self {
            display_id,
            handle,
            status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScalingMode, SceneDesc, SceneHandle, SceneResult, SceneTemplate};
    use crate::{DisplayDesc, EngineError};

    #[test]
    fn scene_descriptor_keeps_display_assignment() {
        let display = DisplayDesc::new(42, -1920, 0, 1920, 1080, 2.0);
        let scene = SceneDesc::new(
            display.clone(),
            "/scene/project.json",
            "/scene/assets",
            60,
            false,
        );

        assert_eq!(scene.display, display);
        assert_eq!(scene.display.display_id, 42);
        assert_eq!(scene.scene_path, "/scene/project.json");
        assert_eq!(scene.assets_path, "/scene/assets");
        assert_eq!(scene.fps, 60);
        assert!(!scene.paused);
        assert!(!scene.audio_response_enabled);
        assert!(!scene.audio_muted);
        assert_eq!(scene.property_override_json, None);
        assert_eq!(scene.shader_cache_path, None);
        assert!(!scene.force_shader_refresh);
    }

    #[test]
    fn scene_descriptor_can_enable_audio_response() {
        let display = DisplayDesc::new(42, -1920, 0, 1920, 1080, 2.0);
        let scene = SceneDesc::new(display, "/scene/project.json", "/scene/assets", 60, false)
            .with_audio_response_enabled(true);

        assert!(scene.audio_response_enabled);
    }

    #[test]
    fn scene_result_keeps_handle_and_status() {
        let result = SceneResult::new(7, SceneHandle::new(99), 0);

        assert_eq!(result.display_id, 7);
        assert_eq!(result.handle.raw(), 99);
        assert_eq!(result.status, 0);
    }

    #[test]
    fn scaling_mode_defaults_to_fit() {
        assert_eq!(ScalingMode::default(), ScalingMode::Fit);
    }

    #[test]
    fn scene_template_builds_scene_for_display() {
        let display = DisplayDesc::new(42, -1920, 0, 1920, 1080, 2.0);
        let template = SceneTemplate::builder("/scene/project.json")
            .assets_path("/scene/assets")
            .fps(75)
            .scaling_mode(ScalingMode::Fill)
            .scaling_factor(1.25)
            .paused(true)
            .audio_response_enabled(true)
            .audio_volume(0.5)
            .audio_muted(true)
            .property_override_json("{\"general\":{\"brightness\":1}}")
            .shader_cache_path("/tmp/cache")
            .force_shader_refresh(true)
            .build()
            .expect("template should build");

        let scene = template.for_display(display.clone());

        assert_eq!(scene.display, display);
        assert_eq!(scene.scene_path, "/scene/project.json");
        assert_eq!(scene.assets_path, "/scene/assets");
        assert_eq!(scene.fps, 75);
        assert_eq!(scene.scaling_mode, ScalingMode::Fill);
        assert!(
            (scene.scaling_factor - 1.25).abs() <= f64::EPSILON,
            "expected scaling factor {} to be within f64::EPSILON of 1.25",
            scene.scaling_factor
        );
        assert!(scene.paused);
        assert!(scene.audio_response_enabled);
        assert_eq!(
            scene.audio_volume,
            crate::media::audio::AudioVolume::try_from(0.5).unwrap()
        );
        assert!(scene.audio_muted);
        assert_eq!(
            scene.property_override_json.as_deref(),
            Some("{\"general\":{\"brightness\":1}}")
        );
        assert_eq!(scene.shader_cache_path.as_deref(), Some("/tmp/cache"));
        assert!(scene.force_shader_refresh);
    }

    #[test]
    fn scene_template_can_be_copied_from_scene_desc() {
        let display = DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0);
        let scene = SceneDesc::builder(display, "/tmp/project.json")
            .assets_path("/tmp/assets")
            .fps(30)
            .build()
            .expect("scene should build");

        let template = SceneTemplate::from_scene_desc(&scene);

        assert_eq!(template.scene_path, scene.scene_path);
        assert_eq!(template.assets_path, scene.assets_path);
        assert_eq!(template.fps, scene.fps);
        assert_eq!(template.scaling_mode, scene.scaling_mode);
        assert!(
            (template.scaling_factor - scene.scaling_factor).abs() <= f64::EPSILON,
            "expected template scaling factor {} to be within f64::EPSILON of scene scaling \
             factor {}",
            template.scaling_factor,
            scene.scaling_factor
        );
    }

    #[test]
    fn scene_template_rejects_empty_scene_path() {
        let error = SceneTemplate::builder("")
            .build()
            .expect_err("empty path should fail");

        match error {
            EngineError::InvalidInput(message) => {
                assert_eq!(message, "scene_path must not be empty");
            }
            other => panic!("expected invalid input, got {other:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneRenderTargetIr {
    pub width: u32,
    pub height: u32,
}

fn read_optional_asset(
    project_root: &Path,
    assets_path: &Path,
    asset: &str,
) -> Result<Option<String>, crate::EngineError> {
    let asset_path = Path::new(asset);
    validate_relative_normal_path(asset_path, "scene asset path")?;

    for root in [project_root, assets_path] {
        if root.as_os_str().is_empty() {
            continue;
        }
        let path = root.join(asset_path);
        if !path.exists() {
            continue;
        }

        let path = path_under_root(root, asset_path, "scene asset")?;
        let content = fs::read_to_string(&path).map_err(|error| {
            crate::EngineError::InvalidInput(format!(
                "failed to read scene asset `{}`: {error}",
                path.display()
            ))
        })?;
        return Ok(Some(content));
    }

    Ok(None)
}

fn path_under_root(
    root: &Path,
    relative: &Path,
    label: &str,
) -> Result<PathBuf, crate::EngineError> {
    validate_relative_normal_path(relative, label)?;

    let root = if root.as_os_str().is_empty() {
        std::env::current_dir().map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to resolve {label} root: {error}"))
        })?
    } else {
        root.to_path_buf()
    };
    let path = root.join(relative);
    let canonical_root = fs::canonicalize(&root).map_err(|error| {
        crate::EngineError::InvalidInput(format!("failed to resolve {label} root: {error}"))
    })?;
    let canonical_path = fs::canonicalize(&path).map_err(|error| {
        crate::EngineError::InvalidInput(format!("failed to resolve {label}: {error}"))
    })?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(crate::EngineError::InvalidInput(format!(
            "{label} must stay under its root"
        )));
    }

    Ok(canonical_path)
}

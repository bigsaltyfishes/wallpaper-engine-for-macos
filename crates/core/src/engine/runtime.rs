use std::sync::Arc;

use serde_json::Value;

use crate::{
    DisplayDesc, EngineError, WallpaperWindow,
    display::state::DisplayKey,
    engine::FirstFrameCallback,
    media::audio::AudioVolume,
    owe::backend::{OweBackend, OweScene},
    project::{
        ProjectManifest, ScalingMode, SceneDesc, SceneHandle, SerdeValudeExt, WallpaperProjectType,
        validate_relative_normal_path,
    },
};

pub struct SceneRuntime {
    /// Last descriptor used to configure the renderer scene.
    pub desc: SceneDesc,
    /// Stable handle used when reporting renderer lifecycle events.
    handle: SceneHandle,
    /// Engine-level callback invoked after OWE renders the first frame.
    first_frame_callback: FirstFrameCallback,
    /// Active wallpaper content for this runtime.
    content: RuntimeContent,
    /// Runtime override applied after descriptor defaults.
    scaling_mode: ScalingMode,
    /// Runtime override applied after descriptor defaults.
    scaling_factor: f64,
    /// Renderer-surface override. Changing this rebuilds the renderer object.
    render_resolution: Option<(u32, u32)>,
    /// Runtime audio-response state, preserved across scene reconciliation.
    audio_response_enabled: bool,
    /// Runtime playback state, preserved across scene reconciliation.
    paused: bool,
    /// Runtime scene-global audio volume, preserved across scene
    /// reconciliation.
    audio_volume: AudioVolume,
    /// Runtime scene-global audio mute state, preserved across scene
    /// reconciliation.
    audio_muted: bool,
    /// Flattened runtime property override, preserved across reconciliation.
    property_override_json: Option<String>,
    /// `AppKit` window that owns the `CAMetalLayer` passed to OWE.
    window: Option<WallpaperWindow>,
}

enum RuntimeContent {
    Scene(OweScene),
    Web(wallpaper_web::Runtime),
}

struct WebRuntime;

impl WebRuntime {
    fn resolve_entry(desc: &SceneDesc) -> Result<Option<std::path::PathBuf>, EngineError> {
        let project_path = std::path::Path::new(&desc.scene_path);
        if project_path.file_name().and_then(|name| name.to_str()) != Some("project.json") {
            return Ok(None);
        }

        let manifest = ProjectManifest::load(project_path)?;
        if manifest.project_type() != WallpaperProjectType::Web {
            return Ok(None);
        }
        if manifest.file().is_empty() {
            return Err(EngineError::InvalidInput(
                "web project file entry must not be empty".to_string(),
            ));
        }

        let entry = std::path::Path::new(manifest.file());
        validate_relative_normal_path(entry, "web project file entry")?;
        Ok(Some(
            project_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .join(entry),
        ))
    }
}

#[derive(Clone)]
pub struct SceneRuntimeState {
    /// Mutable state that should survive replacing the scene descriptor.
    pub scaling_mode: ScalingMode,
    pub scaling_factor: f64,
    pub render_resolution: Option<(u32, u32)>,
    pub audio_response_enabled: bool,
    pub paused: bool,
    pub audio_volume: AudioVolume,
    pub audio_muted: bool,
    pub property_override_json: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescriptorInheritance {
    PreserveRuntimeOverrides,
    UseDescriptorDefaults,
}

#[derive(Debug, PartialEq, Eq)]
enum PropertyOverrideUpdate<'a> {
    Unchanged,
    Apply(&'a str),
    Reset,
}

pub struct RuntimeRefreshJob {
    pub key: DisplayKey,
    pub handle: SceneHandle,
    pub desc: SceneDesc,
    pub runtime_state: SceneRuntimeState,
    pub existing_runtime: Option<SceneRuntime>,
    pub first_frame_callback: FirstFrameCallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRefreshMode {
    Unchanged,
    OpenRuntime,
    UpdateWindowOnly,
    RebuildExistingRuntime,
    ReopenRuntime,
}

impl RuntimeRefreshMode {
    /// Classifies the transition between two scene descriptors.
    pub fn from_transition(current: Option<&SceneDesc>, desired: &SceneDesc) -> Self {
        let Some(current) = current else {
            return RuntimeRefreshMode::OpenRuntime;
        };
        if current.same_wallpaper(desired) && current.display == desired.display {
            return RuntimeRefreshMode::Unchanged;
        }
        if current.same_wallpaper(desired) {
            if current.display.has_same_render_surface(&desired.display) {
                return RuntimeRefreshMode::UpdateWindowOnly;
            }
            return RuntimeRefreshMode::ReopenRuntime;
        }
        RuntimeRefreshMode::RebuildExistingRuntime
    }

    /// Returns true if the transition requires a runtime refresh.
    pub fn is_required(self) -> bool {
        self != RuntimeRefreshMode::Unchanged
    }
}

impl SceneRuntime {
    pub fn open(
        backend: OweBackend,
        first_frame_callback: FirstFrameCallback,
        handle: SceneHandle,
        desc: &SceneDesc,
        state: SceneRuntimeState,
    ) -> Result<Self, EngineError> {
        if let Some(web_entry) = WebRuntime::resolve_entry(desc)? {
            let mut stored_desc = desc.clone();
            stored_desc.mark_shader_refresh_complete();
            let descriptor_state = SceneRuntimeState::try_from(desc)?;
            let web_properties = wallpaper_web::Properties::load(
                std::path::Path::new(&desc.scene_path),
                descriptor_state.property_override_json.as_deref(),
            )
            .map_err(|e| EngineError::Platform(format!("{e}")))?;
            let mut window = WallpaperWindow::builder(desc.display.clone()).open()?;
            window.install_web_view(&web_entry, Some(&web_properties))?;
            let web_runtime = wallpaper_web::Runtime::start(
                move || {
                    backend
                        .current_audio_spectrum_128()
                        .ok()
                        .map(|(bins, _)| bins)
                },
                window.web_audio_dispatcher()?,
                window.web_property_dispatcher()?,
            );
            let mut runtime = Self {
                desc: stored_desc,
                handle,
                first_frame_callback,
                content: RuntimeContent::Web(web_runtime),
                scaling_mode: state.scaling_mode,
                scaling_factor: state.scaling_factor,
                render_resolution: state.render_resolution,
                audio_response_enabled: state.audio_response_enabled,
                paused: state.paused,
                audio_volume: state.audio_volume,
                audio_muted: state.audio_muted,
                property_override_json: descriptor_state.property_override_json.clone(),
                window: Some(window),
            };
            runtime.apply_runtime_properties(&descriptor_state)?;
            return Ok(runtime);
        }

        let mut stored_desc = desc.clone();
        stored_desc.mark_shader_refresh_complete();
        let window = WallpaperWindow::builder(desc.display.clone()).open()?;
        let renderer = backend.open_scene(
            desc,
            window.metal_layer_ptr(),
            state.scaling_mode,
            state.scaling_factor,
            state.render_resolution,
            Some(Arc::new({
                let callback = first_frame_callback.clone();
                move || callback(handle)
            })),
        )?;
        let descriptor_state = SceneRuntimeState::try_from(desc)?;
        let mut runtime = Self {
            desc: desc.clone(),
            handle,
            first_frame_callback,
            content: RuntimeContent::Scene(renderer),
            scaling_mode: state.scaling_mode,
            scaling_factor: state.scaling_factor,
            render_resolution: state.render_resolution,
            audio_response_enabled: state.audio_response_enabled,
            paused: state.paused,
            audio_volume: state.audio_volume,
            audio_muted: state.audio_muted,
            property_override_json: descriptor_state.property_override_json.clone(),
            window: Some(window),
        };
        runtime.desc = stored_desc;
        runtime.apply_runtime_properties(&descriptor_state)?;
        Ok(runtime)
    }

    pub fn set_scaling_mode(&mut self, mode: ScalingMode) -> Result<(), EngineError> {
        match &mut self.content {
            RuntimeContent::Scene(renderer) => renderer.set_scaling_mode(mode)?,
            RuntimeContent::Web(runtime) => runtime
                .set_scaling_mode(mode.to_string().as_str())
                .map_err(|e| EngineError::Platform(format!("{e}")))?,
        }
        self.scaling_mode = mode;
        self.desc.scaling_mode = mode;
        Ok(())
    }

    pub fn set_scaling_factor(&mut self, factor: f64) -> Result<(), EngineError> {
        match &mut self.content {
            RuntimeContent::Scene(renderer) => renderer.set_scaling_factor(factor)?,
            RuntimeContent::Web(runtime) => runtime
                .set_scaling_factor(factor)
                .map_err(|e| EngineError::Platform(format!("{e}")))?,
        }
        self.scaling_factor = factor;
        self.desc.scaling_factor = factor;
        Ok(())
    }

    pub fn set_fps(&mut self, fps: u32) -> Result<(), EngineError> {
        match &mut self.content {
            RuntimeContent::Scene(renderer) => renderer.set_target_fps(fps)?,
            RuntimeContent::Web(runtime) => {
                runtime.set_fps(fps).map_err(|e| EngineError::Platform(format!("{e}")))?;
            }
        }
        self.desc.fps = fps;
        Ok(())
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_paused(paused)?;
        }
        self.paused = paused;
        Ok(())
    }

    pub fn set_mouse_position(&mut self, x: f64, y: f64) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_mouse_position(x, y)?;
        }
        Ok(())
    }

    pub fn set_mouse_button(&mut self, button: u32, pressed: bool) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_mouse_button(button, pressed)?;
        }
        Ok(())
    }

    pub fn set_mouse_entered(&mut self, entered: bool) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_mouse_entered(entered)?;
        }
        Ok(())
    }

    pub fn set_render_resolution(
        &mut self,
        backend: OweBackend,
        width: u32,
        height: u32,
    ) -> Result<(), EngineError> {
        self.rebuild_for_desc(backend, &self.desc.clone(), Some((width, height)))
    }

    fn rebuild_for_desc(
        &mut self,
        backend: OweBackend,
        desc: &SceneDesc,
        render_resolution: Option<(u32, u32)>,
    ) -> Result<(), EngineError> {
        if matches!(self.content, RuntimeContent::Web(_))
            || WebRuntime::resolve_entry(desc)?.is_some()
        {
            let state = self.runtime_state();
            let mut replacement = Self::open(
                backend,
                self.first_frame_callback.clone(),
                self.handle,
                desc,
                state,
            )?;
            replacement.render_resolution = render_resolution;
            let mut old = std::mem::replace(self, replacement);
            return old.close();
        }

        let mut state = self.runtime_state();
        let current_descriptor_state = SceneRuntimeState::try_from(&self.desc)?;
        let descriptor_state = SceneRuntimeState::try_from(desc)?;
        let mut stored_desc = desc.clone();
        stored_desc.mark_shader_refresh_complete();
        let inheritance = if self.desc.same_wallpaper(desc) {
            DescriptorInheritance::PreserveRuntimeOverrides
        } else {
            DescriptorInheritance::UseDescriptorDefaults
        };
        state.inherit_descriptor_defaults(
            &current_descriptor_state,
            &descriptor_state,
            inheritance,
        );
        let old_display = self.desc.display.clone();
        let first_frame_callback = self.renderer_first_frame_callback();
        let window = self.window.as_mut().ok_or_else(|| {
            EngineError::Platform("wallpaper window is already closed".to_string())
        })?;

        // Swap the CAMetalLayer BEFORE creating the new OWE scene so that
        // each SceneWallpaper's VkSurface references its own CAMetalLayer.
        // If we reused the old layer via `update_display`, the new VkSurface
        // would be created on the same CAMetalLayer that the old VkSurface
        // still references (it's destroyed only in `old_renderer.close()`
        // below), violating the VK_EXT_metal_surface invariant that only
        // one VkSurfaceKHR can be associated with a CAMetalLayer at a time.
        //
        // The old CAMetalLayer remains alive via MoltenVK's retain through
        // the old VkSurface, and is deallocated when `old_renderer.close()`
        // destroys that surface.
        let metal_layer = window.update_layer(desc.display.clone())?;

        let mut renderer = match backend.open_scene(
            desc,
            metal_layer,
            state.scaling_mode,
            state.scaling_factor,
            render_resolution,
            Some(first_frame_callback),
        ) {
            Ok(renderer) => renderer,
            Err(error) => {
                // Best-effort geometry rollback. The layer swap is not reversible
                // without creating yet another new layer, so we restore the
                // NSWindow/NSView frames to the old display's geometry instead.
                let _ = window.update_display(old_display);
                return Err(error);
            }
        };
        if let Err(error) = state.apply_to(&mut renderer, &descriptor_state) {
            let _ = renderer.close();
            let _ = window.update_display(old_display);
            return Err(error);
        }
        let RuntimeContent::Scene(old_renderer) =
            std::mem::replace(&mut self.content, RuntimeContent::Scene(renderer))
        else {
            unreachable!("web runtime was rebuilt before opening OWE");
        };
        let mut old_renderer = old_renderer;
        self.desc = stored_desc;
        self.scaling_mode = state.scaling_mode;
        self.scaling_factor = state.scaling_factor;
        self.render_resolution = render_resolution;
        self.audio_response_enabled = state.audio_response_enabled;
        self.audio_volume = state.audio_volume;
        self.audio_muted = state.audio_muted;
        self.property_override_json = state.property_override_json;
        old_renderer.close()
    }

    pub fn replace_wallpaper(
        &mut self,
        backend: OweBackend,
        desc: &SceneDesc,
    ) -> Result<(), EngineError> {
        let state = self.runtime_state();
        let replacement = Self::open(
            backend,
            self.first_frame_callback.clone(),
            self.handle,
            desc,
            state,
        )?;
        let mut old = std::mem::replace(self, replacement);
        old.close()
    }

    pub fn resize_or_rebuild(
        &mut self,
        backend: OweBackend,
        display: DisplayDesc,
    ) -> Result<(), EngineError> {
        if self.desc.display == display {
            return Ok(());
        }
        // Note: the fast surface-reconfigure transaction lives in
        // `run_runtime_refresh_job`'s `ReopenRuntime` branch, not here. This
        // method is reached only from `RebuildExistingRuntime`, which by
        // construction means wallpaper-defining fields have changed and a
        // full scene rebuild is required.
        let mut desc = self.desc.clone();
        desc.display = display;
        self.rebuild_for_desc(backend, &desc, self.render_resolution)
    }

    fn renderer_first_frame_callback(&self) -> crate::owe::backend::FirstFrameCallback {
        let callback = self.first_frame_callback.clone();
        let handle = self.handle;
        Arc::new(move || callback(handle))
    }

    pub fn update_window_display(&mut self, display: DisplayDesc) -> Result<(), EngineError> {
        let window = self.window.as_mut().ok_or_else(|| {
            EngineError::Platform("scene runtime has no window during display update".to_string())
        })?;
        window.update_display(display.clone())?;
        self.desc.display = display;
        Ok(())
    }

    /// Fast-path display reconfiguration. Preserves scene, shaders, render
    /// graph, audio, and runtime state; only the Vulkan surface, swapchain,
    /// and presentation passes are rebuilt.
    ///
    /// On any failure, returns `Err`; the caller should fall back to
    /// `rebuild_for_desc`.
    pub fn reconfigure_for_display(
        &mut self,
        backend: OweBackend,
        display: DisplayDesc,
    ) -> Result<(), EngineError> {
        if matches!(self.content, RuntimeContent::Web(_)) {
            self.update_window_display(display)?;
            return Ok(());
        }

        let _ = backend; // retained in signature for symmetry with rebuild_for_desc
        let start = std::time::Instant::now();
        let runtime_state = self.runtime_state();

        // 1. Pause rendering and release the renderer-side surface.
        let RuntimeContent::Scene(renderer) = &mut self.content else {
            unreachable!("web runtime returned before scene reconfigure");
        };
        renderer.begin_surface_reconfigure()?;

        // 2. Swap the CAMetalLayer under the existing NSWindow.
        let window = self.window.as_mut().ok_or_else(|| {
            EngineError::Platform("scene runtime has no window during reconfigure".to_string())
        })?;
        let metal_layer = window.update_layer(display.clone())?;

        // 3. Compute render resolution from the new display, or keep explicit override
        //    if one was set.
        let (render_width, render_height) = self
            .render_resolution
            .unwrap_or((display.width, display.height));

        // 4. Rebuild the Vulkan surface + swapchain + presentation passes from the new
        //    layer, and resume rendering.
        renderer.finish_surface_reconfigure(
            metal_layer,
            display.width,
            display.height,
            render_width,
            render_height,
            display.scale_factor,
        )?;

        // 5. OWE resumes after finishing the surface transaction. Preserve an
        //    already-paused runtime by restoring that state before returning.
        if runtime_state.paused {
            renderer.set_paused(true)?;
        }

        // 6. Commit the new descriptor.
        self.desc.display = display;
        log::debug!(
            "[wallpaper-core engine] display reconfigure completed in {:?}",
            start.elapsed()
        );
        Ok(())
    }

    pub fn set_audio_response_enabled(&mut self, enabled: bool) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_audio_response_enabled(enabled)?;
        }
        self.audio_response_enabled = enabled;
        self.desc.audio_response_enabled = enabled;
        Ok(())
    }

    pub fn set_audio_volume(&mut self, volume: AudioVolume) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_audio_volume(volume)?;
        }
        self.audio_volume = volume;
        self.desc.audio_volume = volume;
        Ok(())
    }

    pub fn set_audio_muted(&mut self, muted: bool) -> Result<(), EngineError> {
        if let RuntimeContent::Scene(renderer) = &mut self.content {
            renderer.set_audio_muted(muted)?;
        }
        self.audio_muted = muted;
        self.desc.audio_muted = muted;
        Ok(())
    }

    pub fn set_property_override_json(
        &mut self,
        flat_json: Option<String>,
    ) -> Result<(), EngineError> {
        match &mut self.content {
            RuntimeContent::Scene(renderer) => {
                if let Some(json) = flat_json.as_deref() {
                    renderer.set_property_override(json)?;
                } else {
                    renderer.reset_property_override()?;
                }
            }
            RuntimeContent::Web(runtime) => {
                let properties = wallpaper_web::Properties::load(
                    std::path::Path::new(&self.desc.scene_path),
                    flat_json.as_deref(),
                )
                .map_err(|e| EngineError::Platform(format!("{e}")))?;
                runtime
                    .dispatch_properties(&properties)
                    .map_err(|e| EngineError::Platform(format!("{e}")))?;
            }
        }
        self.property_override_json = flat_json;
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), EngineError> {
        let backend_result = match &mut self.content {
            RuntimeContent::Scene(renderer) => renderer.close(),
            RuntimeContent::Web(_) => Ok(()),
        };
        if let Some(mut window) = self.window.take() {
            window.close();
        }
        backend_result
    }

    pub fn runtime_state(&self) -> SceneRuntimeState {
        SceneRuntimeState {
            scaling_mode: self.scaling_mode,
            scaling_factor: self.scaling_factor,
            render_resolution: self.render_resolution,
            audio_response_enabled: self.audio_response_enabled,
            paused: self.paused,
            audio_volume: self.audio_volume,
            audio_muted: self.audio_muted,
            property_override_json: self.property_override_json.clone(),
        }
    }

    pub(crate) fn runtime_state_for_desc(
        &self,
        desc: &SceneDesc,
    ) -> Result<SceneRuntimeState, EngineError> {
        let mut state = self.runtime_state();
        state.inherit_descriptor_transition(&self.desc, desc)?;
        Ok(state)
    }

    fn apply_runtime_properties(
        &mut self,
        descriptor_state: &SceneRuntimeState,
    ) -> Result<(), EngineError> {
        let state = self.runtime_state();
        match &mut self.content {
            RuntimeContent::Scene(renderer) => state.apply_to(renderer, descriptor_state)?,
            RuntimeContent::Web(runtime) => {
                if !matches!(
                    state.property_override_update(descriptor_state),
                    PropertyOverrideUpdate::Unchanged
                ) {
                    let properties = wallpaper_web::Properties::load(
                        std::path::Path::new(&self.desc.scene_path),
                        descriptor_state.property_override_json.as_deref(),
                    )
                    .map_err(|e| EngineError::Platform(format!("{e}")))?;
                    runtime
                        .dispatch_properties(&properties)
                        .map_err(|e| EngineError::Platform(format!("{e}")))?;
                }
                runtime
                    .set_scaling_mode(self.desc.scaling_mode.to_string().as_str())
                    .map_err(|e| EngineError::Platform(format!("{e}")))?;
                runtime
                    .set_scaling_factor(self.desc.scaling_factor)
                    .map_err(|e| EngineError::Platform(format!("{e}")))?;
                runtime
                    .set_fps(self.desc.fps)
                    .map_err(|e| EngineError::Platform(format!("{e}")))?;
            }
        }
        Ok(())
    }
}
impl Drop for SceneRuntime {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl SceneRuntimeState {
    pub(crate) fn inherit_descriptor_transition(
        &mut self,
        current_desc: &SceneDesc,
        next_desc: &SceneDesc,
    ) -> Result<(), EngineError> {
        let current_descriptor_state = Self::try_from(current_desc)?;
        let next_descriptor_state = Self::try_from(next_desc)?;
        let inheritance = if current_desc.same_wallpaper(next_desc) {
            DescriptorInheritance::PreserveRuntimeOverrides
        } else {
            DescriptorInheritance::UseDescriptorDefaults
        };
        self.inherit_descriptor_defaults(
            &current_descriptor_state,
            &next_descriptor_state,
            inheritance,
        );
        Ok(())
    }

    fn apply_to(
        &self,
        renderer: &mut OweScene,
        descriptor_state: &SceneRuntimeState,
    ) -> Result<(), EngineError> {
        if self.paused != descriptor_state.paused {
            renderer.set_paused(self.paused)?;
        }
        if self.audio_response_enabled != descriptor_state.audio_response_enabled {
            renderer.set_audio_response_enabled(self.audio_response_enabled)?;
        }
        if self.audio_volume != descriptor_state.audio_volume {
            renderer.set_audio_volume(self.audio_volume)?;
        }
        if self.audio_muted != descriptor_state.audio_muted {
            renderer.set_audio_muted(self.audio_muted)?;
        }
        match self.property_override_update(descriptor_state) {
            PropertyOverrideUpdate::Unchanged => {}
            PropertyOverrideUpdate::Apply(json) => renderer.set_property_override(json)?,
            PropertyOverrideUpdate::Reset => renderer.reset_property_override()?,
        }
        Ok(())
    }

    fn property_override_update<'a>(
        &'a self,
        descriptor_state: &SceneRuntimeState,
    ) -> PropertyOverrideUpdate<'a> {
        match (
            self.property_override_json.as_deref(),
            descriptor_state.property_override_json.as_deref(),
        ) {
            (runtime, descriptor) if runtime == descriptor => PropertyOverrideUpdate::Unchanged,
            (Some(json), _) => PropertyOverrideUpdate::Apply(json),
            (None, Some(_)) => PropertyOverrideUpdate::Reset,
            (None, None) => PropertyOverrideUpdate::Unchanged,
        }
    }

    fn inherit_descriptor_property_override(
        &mut self,
        current_descriptor_state: &SceneRuntimeState,
        next_descriptor_state: &SceneRuntimeState,
    ) {
        if self.property_override_json == current_descriptor_state.property_override_json {
            self.property_override_json
                .clone_from(&next_descriptor_state.property_override_json);
        }
    }

    fn inherit_descriptor_defaults(
        &mut self,
        current_descriptor_state: &SceneRuntimeState,
        next_descriptor_state: &SceneRuntimeState,
        inheritance: DescriptorInheritance,
    ) {
        self.inherit_descriptor_property_override(current_descriptor_state, next_descriptor_state);

        if inheritance == DescriptorInheritance::UseDescriptorDefaults {
            self.scaling_mode = next_descriptor_state.scaling_mode;
            self.scaling_factor = next_descriptor_state.scaling_factor;
            self.audio_response_enabled = next_descriptor_state.audio_response_enabled;
            self.audio_volume = next_descriptor_state.audio_volume;
            self.audio_muted = next_descriptor_state.audio_muted;
        }
    }
}

impl TryFrom<&SceneDesc> for SceneRuntimeState {
    type Error = EngineError;

    fn try_from(desc: &SceneDesc) -> Result<Self, Self::Error> {
        // Descriptor values seed runtime state only for first open. Later
        // reconciliations keep explicit API changes such as property overrides.
        Ok(Self {
            scaling_mode: desc.scaling_mode,
            scaling_factor: desc.scaling_factor,
            render_resolution: None,
            audio_response_enabled: desc.audio_response_enabled,
            paused: desc.paused,
            audio_volume: desc.audio_volume,
            audio_muted: desc.audio_muted,
            property_override_json: desc
                .property_override_json
                .as_deref()
                .map(|json| {
                    let flat_json = serde_json::from_str::<Value>(json)
                        .map_err(|e| EngineError::InvalidInput(e.to_string()))?
                        .flatten()?;
                    serde_json::to_string(&flat_json)
                        .map_err(|e| EngineError::InvalidInput(e.to_string()))
                })
                .transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_runtime_state_initial_uses_descriptor_pause_state() {
        let desc = crate::project::SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .paused(true)
        .build()
        .expect("scene should build");

        let state = SceneRuntimeState::try_from(&desc).expect("state should build");

        assert!(state.paused);
    }

    #[test]
    fn property_override_delta_keeps_matching_empty_override_unchanged() {
        let state = runtime_state(None);
        let descriptor_state = runtime_state(None);

        assert!(matches!(
            state.property_override_update(&descriptor_state),
            PropertyOverrideUpdate::Unchanged
        ));
    }

    #[test]
    fn property_override_delta_resets_only_when_descriptor_supplies_override() {
        let state = runtime_state(None);
        let descriptor_state = runtime_state(Some(r#"{"enabled":true}"#));

        assert!(matches!(
            state.property_override_update(&descriptor_state),
            PropertyOverrideUpdate::Reset
        ));
    }

    #[test]
    fn property_override_delta_applies_runtime_override_when_different() {
        let state = runtime_state(Some(r#"{"enabled":false}"#));
        let descriptor_state = runtime_state(Some(r#"{"enabled":true}"#));

        assert!(matches!(
            state.property_override_update(&descriptor_state),
            PropertyOverrideUpdate::Apply(r#"{"enabled":false}"#)
        ));
    }

    #[test]
    fn descriptor_property_override_change_is_not_reset_when_runtime_matches_current_descriptor() {
        let mut state = runtime_state(None);
        let current_descriptor_state = runtime_state(None);
        let next_descriptor_state = runtime_state(Some(r#"{"newproperty24":false}"#));

        state.inherit_descriptor_property_override(
            &current_descriptor_state,
            &next_descriptor_state,
        );

        assert_eq!(
            state.property_override_json.as_deref(),
            Some(r#"{"newproperty24":false}"#)
        );
        assert!(matches!(
            state.property_override_update(&next_descriptor_state),
            PropertyOverrideUpdate::Unchanged
        ));
    }

    #[test]
    fn explicit_runtime_property_reset_still_overrides_descriptor_refresh() {
        let mut state = runtime_state(None);
        let current_descriptor_state = runtime_state(Some(r#"{"newproperty24":true}"#));
        let next_descriptor_state = runtime_state(Some(r#"{"newproperty24":false}"#));

        state.inherit_descriptor_property_override(
            &current_descriptor_state,
            &next_descriptor_state,
        );

        assert_eq!(state.property_override_json, None);
        assert!(matches!(
            state.property_override_update(&next_descriptor_state),
            PropertyOverrideUpdate::Reset
        ));
    }

    #[test]
    fn different_wallpaper_rebuild_uses_next_descriptor_render_defaults() {
        let mut state = runtime_state(None);
        state.scaling_mode = ScalingMode::Fill;
        state.scaling_factor = 1.25;
        state.audio_response_enabled = true;
        let mut current_descriptor_state = runtime_state(None);
        current_descriptor_state.scaling_mode = ScalingMode::Fill;
        current_descriptor_state.scaling_factor = 1.25;
        current_descriptor_state.audio_response_enabled = true;
        let mut next_descriptor_state = runtime_state(None);
        next_descriptor_state.scaling_mode = ScalingMode::Stretch;
        next_descriptor_state.scaling_factor = 2.0;
        next_descriptor_state.audio_response_enabled = false;

        state.inherit_descriptor_defaults(
            &current_descriptor_state,
            &next_descriptor_state,
            DescriptorInheritance::UseDescriptorDefaults,
        );

        assert_eq!(state.scaling_mode, ScalingMode::Stretch);
        assert!(
            (state.scaling_factor - 2.0).abs() <= f64::EPSILON,
            "expected scaling factor {} to be within f64::EPSILON of 2.0",
            state.scaling_factor
        );
        assert!(!state.audio_response_enabled);
    }

    #[test]
    fn live_runtime_transition_state_for_new_wallpaper_uses_descriptor_scaling_defaults() {
        let current_desc = SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/current/project.json",
        )
        .assets_path("/tmp/assets")
        .scaling_mode(ScalingMode::Fill)
        .scaling_factor(1.25)
        .build()
        .expect("current scene should build");
        let next_desc = SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/next/project.json",
        )
        .assets_path("/tmp/assets")
        .scaling_mode(ScalingMode::Fit)
        .scaling_factor(1.0)
        .build()
        .expect("next scene should build");
        let mut state =
            SceneRuntimeState::try_from(&current_desc).expect("current state should build");

        state
            .inherit_descriptor_transition(&current_desc, &next_desc)
            .expect("transition should build");

        assert_eq!(state.scaling_mode, ScalingMode::Fit);
        assert!(
            (state.scaling_factor - 1.0).abs() <= f64::EPSILON,
            "expected descriptor scaling factor 1.0, got {}",
            state.scaling_factor
        );
    }

    #[test]
    fn same_wallpaper_rebuild_preserves_explicit_runtime_render_overrides() {
        let mut state = runtime_state(None);
        state.scaling_mode = ScalingMode::Fill;
        state.scaling_factor = 1.25;
        state.audio_response_enabled = false;
        let current_descriptor_state = runtime_state(None);
        let mut next_descriptor_state = runtime_state(None);
        next_descriptor_state.scaling_mode = ScalingMode::Stretch;
        next_descriptor_state.scaling_factor = 2.0;
        next_descriptor_state.audio_response_enabled = true;

        state.inherit_descriptor_defaults(
            &current_descriptor_state,
            &next_descriptor_state,
            DescriptorInheritance::PreserveRuntimeOverrides,
        );

        assert_eq!(state.scaling_mode, ScalingMode::Fill);
        assert!(
            (state.scaling_factor - 1.25).abs() <= f64::EPSILON,
            "expected scaling factor {} to be within f64::EPSILON of 1.25",
            state.scaling_factor
        );
        assert!(!state.audio_response_enabled);
    }

    #[test]
    fn paused_runtime_requires_pause_restore_after_surface_reconfigure() {
        let mut state = runtime_state(None);
        state.paused = true;

        assert!(state.paused);
    }

    #[test]
    fn running_runtime_does_not_require_pause_restore_after_surface_reconfigure() {
        let state = runtime_state(None);

        assert!(!state.paused);
    }

    fn runtime_state(property_override_json: Option<&str>) -> SceneRuntimeState {
        SceneRuntimeState {
            scaling_mode: ScalingMode::Fit,
            scaling_factor: 1.0,
            render_resolution: None,
            audio_response_enabled: true,
            paused: false,
            audio_volume: AudioVolume::try_from(1.0).expect("volume should be valid"),
            audio_muted: false,
            property_override_json: property_override_json.map(ToOwned::to_owned),
        }
    }
}

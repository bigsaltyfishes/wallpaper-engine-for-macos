mod actor;
mod config;
mod messages;
mod runtime;
mod snapshot;
mod state;

use std::sync::Arc;

use actor::{EngineActor, EngineActorHandle};
pub use config::{DisplayConfig, DisplaySelector, WallpaperAssignment, WallpaperEngineConfig};
use serde_json::Value;
pub use snapshot::EngineSnapshotPublisher;

#[cfg(test)]
use crate::engine::runtime::{RuntimeRefreshMode, SceneRuntimeState};
#[cfg(test)]
use crate::project::SceneDescSliceExt;
use crate::{
    DisplayDesc, DisplayIdentity, EngineError,
    display::{
        callback::{DisplayChangeRegistration, DisplayRefreshTarget},
        state::DisplayStateModel,
    },
    engine::state::EngineState,
    media::audio::{
        AudioCaptureError, AudioFrameConsumer, AudioResponseResampler, AudioVolume,
        InterleavedStereoF32, MonoPcmF32,
    },
    owe::backend::OweBackend,
    project::{ScalingMode, SceneDesc, SceneHandle, SceneResult, SerdeValudeExt},
};

/// Public snapshot of one display's engine-side state. Returned by
/// [`WallpaperEngine::display_snapshot`] for UI consumers that mirror the
/// engine's view of attached monitors.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySnapshotEntry {
    /// Stable identity metadata for matching this display across sessions.
    pub identity: DisplayIdentity,
    /// Physical display descriptor.
    pub desc: DisplayDesc,
    /// Live scene handle for this display, if the engine has opened one.
    pub handle: Option<SceneHandle>,
    /// Whether this display currently has a wallpaper window.
    pub window_active: bool,
    /// The wallpaper assigned to this display, if any.
    pub assignment: Option<WallpaperAssignment>,
}

/// Rust entry point for controlling wallpaper scenes.
///
/// The type is cloneable so UI, audio, and smoke-test code can share the same
/// engine handle. On macOS it owns the Rust-managed scene/window state
/// directly; on unsupported platforms construction returns
/// [`EngineError::UnsupportedPlatform`].
#[derive(Clone)]
pub struct WallpaperEngine {
    backend: OweBackend,
    snapshots: Arc<EngineSnapshotPublisher>,
    audio_response_resampler: Arc<std::sync::Mutex<AudioResponseResampler>>,
    #[allow(dead_code)]
    actor: EngineActorHandle,
    /// Owns callback registration and its target for the engine lifetime.
    #[allow(dead_code)]
    lifecycle: Arc<EngineLifecycle>,
}

struct EngineLifecycle {
    #[allow(dead_code)]
    refresh_target: Arc<EngineRefreshTarget>,
    #[allow(dead_code)]
    display_callback: Option<DisplayChangeRegistration<EngineRefreshTarget>>,
}

struct EngineRefreshTarget {
    actor: kameo::actor::ActorRef<EngineActor>,
}

impl DisplayRefreshTarget for EngineRefreshTarget {
    fn schedule(&self) {
        if let Err(error) = self.actor.tell(messages::RefreshDisplays).blocking_send() {
            log::warn!(
                "[wallpaper-core display] skipped display refresh because actor mailbox failed: \
                 {error}"
            );
        }
    }
}

impl EngineLifecycle {
    #[allow(clippy::single_call_fn)]
    fn new(actor: &EngineActorHandle) -> Result<Self, EngineError> {
        let refresh_target = Arc::new(EngineRefreshTarget {
            actor: actor.actor().clone(),
        });
        #[cfg(not(test))]
        let display_callback = Some(DisplayChangeRegistration::register(&refresh_target)?);
        #[cfg(test)]
        let display_callback = match tokio::runtime::Handle::try_current() {
            Ok(handle)
                if matches!(
                    handle.runtime_flavor(),
                    tokio::runtime::RuntimeFlavor::CurrentThread
                ) =>
            {
                None
            }
            _ => Some(DisplayChangeRegistration::register(&refresh_target)?),
        };

        Ok(Self {
            refresh_target,
            display_callback,
        })
    }
}

impl WallpaperEngine {
    /// Creates a wallpaper engine instance for the current platform.
    ///
    /// # Errors
    ///
    /// Returns an error if the native renderer backend, display model, actor,
    /// or platform display callback cannot be initialized.
    pub fn new() -> Result<Self, EngineError> {
        Self::with_config(WallpaperEngineConfig::default())
    }

    /// Creates a wallpaper engine instance using an explicit startup
    /// configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the native renderer backend, display model, actor,
    /// or platform display callback cannot be initialized.
    pub fn with_config(config: WallpaperEngineConfig) -> Result<Self, EngineError> {
        let backend = OweBackend::initialize()?;
        let model = DisplayStateModel::from_config(config)?;
        let actor_state = EngineState::with_display_model(model);
        let initial_snapshot = actor_state.snapshot();
        let snapshots = Arc::new(EngineSnapshotPublisher::new(initial_snapshot));
        let actor = EngineActorHandle::spawn(backend, actor_state, Arc::clone(&snapshots))?;
        let lifecycle = Arc::new(EngineLifecycle::new(&actor)?);
        let engine = Self {
            backend,
            snapshots,
            audio_response_resampler: Arc::new(
                std::sync::Mutex::new(AudioResponseResampler::new()),
            ),
            actor,
            lifecycle,
        };
        Ok(engine)
    }

    /// Alias for [`WallpaperEngine::new`] retained for call sites that model an
    /// explicit startup phase.
    ///
    /// # Errors
    ///
    /// Returns an error if [`WallpaperEngine::new`] cannot initialize the
    /// engine.
    pub fn initialize() -> Result<Self, EngineError> {
        Self::new()
    }

    async fn ask_actor<M, T>(&self, message: M) -> Result<T, EngineError>
    where
        EngineActor: kameo::message::Message<M>,
        <EngineActor as kameo::message::Message<M>>::Reply:
            kameo::reply::Reply<Ok = T, Error = EngineError>,
        M: Send + 'static,
        T: Send + 'static,
    {
        self.actor
            .actor()
            .ask(message)
            .await
            .map_err(EngineError::from)
    }

    /// Refreshes the platform display list.
    ///
    /// # Errors
    ///
    /// Returns an error if the platform display query or actor request fails.
    pub async fn refresh_displays(&self) -> Result<(), EngineError> {
        self.ask_actor(messages::RefreshDisplays).await
    }

    #[must_use]
    pub fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry> {
        self.snapshots.load().displays.clone()
    }

    #[cfg(test)]
    /// # Errors
    ///
    /// Returns an error if actor communication fails.
    pub async fn actor_ping_for_test(&self) -> Result<(), EngineError> {
        self.actor
            .actor()
            .ask(messages::Ping)
            .await
            .map_err(EngineError::from)?;
        Ok(())
    }

    #[cfg(test)]
    /// # Errors
    ///
    /// Returns an error if actor communication fails.
    pub async fn actor_display_record_count_for_test(&self) -> Result<usize, EngineError> {
        self.actor
            .actor()
            .ask(messages::DisplayRecordCountForTest)
            .await
            .map_err(EngineError::from)
    }

    #[cfg(test)]
    /// # Errors
    ///
    /// Returns an error if actor communication fails.
    pub async fn actor_sequence_for_test(&self, expected: u64) -> Result<u64, EngineError> {
        self.ask_actor(messages::SequenceForTest { expected }).await
    }

    #[cfg(test)]
    /// # Panics
    ///
    /// Panics if the test-only actor shell cannot be started.
    pub async fn actor_closed_error_for_test() -> EngineError {
        let snapshots = Arc::new(EngineSnapshotPublisher::new(
            EngineState::default().snapshot(),
        ));
        let actor_handle = EngineActorHandle::spawn(OweBackend, EngineState::default(), snapshots)
            .expect("test actor shell should start");
        let actor = actor_handle.actor().clone();
        let _ = actor.stop_gracefully().await;
        actor.wait_for_shutdown().await;

        let error = actor
            .ask(messages::Ping)
            .await
            .expect_err("stopped actor ask should fail");
        EngineError::from(error)
    }

    /// Sets or clears the wallpaper assignment for one display selector.
    ///
    /// # Errors
    ///
    /// Returns an error if the selector is invalid, actor communication fails,
    /// or the renderer rejects the requested assignment.
    pub async fn set_wallpaper_for_display(
        &self,
        selector: DisplaySelector,
        wallpaper: WallpaperAssignment,
    ) -> Result<Option<SceneHandle>, EngineError> {
        self.ask_actor(messages::SetWallpaperForDisplay {
            selector,
            wallpaper,
        })
        .await
    }

    /// Creates a window for one display selector.
    ///
    /// # Errors
    ///
    /// Returns an error if the selector is invalid, actor communication fails,
    /// or the platform window cannot be created.
    pub async fn create_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> Result<Option<SceneHandle>, EngineError> {
        self.ask_actor(messages::CreateWindowForDisplay { selector })
            .await
    }

    /// Destroys the window for one display selector.
    ///
    /// # Errors
    ///
    /// Returns an error if the selector is invalid, actor communication fails,
    /// or the platform window cannot be closed.
    pub async fn destroy_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> Result<(), EngineError> {
        self.ask_actor(messages::DestroyWindowForDisplay { selector })
            .await
    }

    /// Makes the active renderer scene set match `scenes`.
    ///
    /// Each input item describes the desired wallpaper for one display. The
    /// returned handles are stable identifiers for later per-scene operations
    /// such as scaling or audio-response toggles.
    ///
    /// # Errors
    ///
    /// Returns an error if scene descriptors are invalid, actor communication
    /// fails, or the renderer cannot open/reconfigure the requested scenes.
    pub async fn reconcile_scenes(
        &self,
        scenes: Vec<SceneDesc>,
    ) -> Result<Vec<SceneResult>, EngineError> {
        self.ask_actor(messages::ReconcileScenes { scenes }).await
    }

    /// Closes all scenes currently owned by this engine instance.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer cannot
    /// close one of the open scenes.
    pub async fn close_all_scenes(&self) -> Result<(), EngineError> {
        self.ask_actor(messages::CloseAllScenes).await
    }

    /// Sets how a scene is fit into its display/output rectangle.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_scaling_mode(
        &self,
        handle: SceneHandle,
        mode: ScalingMode,
    ) -> Result<(), EngineError> {
        self.ask_actor(messages::SetScalingMode { handle, mode })
            .await
    }

    /// Sets an additional positive scale factor for a scene.
    ///
    /// # Errors
    ///
    /// Returns an error if `factor` is not finite/positive, actor communication
    /// fails, or the renderer rejects the update.
    pub async fn set_scaling_factor(
        &self,
        handle: SceneHandle,
        factor: f64,
    ) -> Result<(), EngineError> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(EngineError::InvalidInput(
                "scaling factor must be finite and greater than zero".to_string(),
            ));
        }

        self.ask_actor(messages::SetScalingFactor { handle, factor })
            .await
    }

    /// Live-updates the target FPS for one scene.
    ///
    /// # Errors
    ///
    /// Returns an error if `fps` is zero, actor communication fails, or the
    /// renderer rejects the update.
    pub async fn set_fps(&self, handle: SceneHandle, fps: u32) -> Result<(), EngineError> {
        if fps == 0 {
            return Err(EngineError::InvalidInput(
                "fps must be greater than zero".to_string(),
            ));
        }

        self.ask_actor(messages::SetFps { handle, fps }).await
    }

    /// Live-updates the paused state for one open scene.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_paused(&self, handle: SceneHandle, paused: bool) -> Result<(), EngineError> {
        self.ask_actor(messages::SetPaused { handle, paused }).await
    }

    /// Live-updates the paused state for all open scenes.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_all_paused(&self, paused: bool) -> Result<(), EngineError> {
        self.ask_actor(messages::SetAllPaused { paused }).await
    }

    /// Overrides the scene render resolution.
    ///
    /// The dimensions must be non-zero. Use the scene/display default instead
    /// of calling this method when no explicit override is wanted.
    ///
    /// # Errors
    ///
    /// Returns an error if either dimension is zero, actor communication fails,
    /// or the renderer cannot apply the new resolution.
    pub async fn set_render_resolution(
        &self,
        handle: SceneHandle,
        width: u32,
        height: u32,
    ) -> Result<(), EngineError> {
        if width == 0 || height == 0 {
            return Err(EngineError::InvalidInput(
                "render resolution dimensions must be non-zero".to_string(),
            ));
        }

        self.ask_actor(messages::SetRenderResolution {
            handle,
            width,
            height,
        })
        .await
    }

    /// Enables or disables audio-responsive scene behavior.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_audio_response_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), EngineError> {
        self.ask_actor(messages::SetAudioResponseEnabled { handle, enabled })
            .await
    }

    /// Sets the scene-wide audio volume multiplier.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_audio_volume(
        &self,
        handle: SceneHandle,
        volume: AudioVolume,
    ) -> Result<(), EngineError> {
        self.ask_actor(messages::SetAudioVolume { handle, volume })
            .await
    }

    /// Sets the scene-wide audio mute flag.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the update.
    pub async fn set_audio_muted(
        &self,
        handle: SceneHandle,
        muted: bool,
    ) -> Result<(), EngineError> {
        self.ask_actor(messages::SetAudioMuted { handle, muted })
            .await
    }

    /// Applies a project property override JSON document to an open scene.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON cannot be parsed/flattened, actor
    /// communication fails, or the renderer rejects the override.
    pub async fn set_property_override<T: Into<String> + Send>(
        &self,
        handle: SceneHandle,
        json: T,
    ) -> Result<(), EngineError> {
        let json = json.into();
        let flat_json = serde_json::from_str::<Value>(&json)
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?
            .flatten()?;
        let flat_json = serde_json::to_string(&flat_json)
            .map_err(|e| EngineError::InvalidInput(e.to_string()))?;

        self.ask_actor(messages::SetPropertyOverride { handle, flat_json })
            .await
    }

    /// Clears project property overrides for an open scene.
    ///
    /// # Errors
    ///
    /// Returns an error if actor communication fails or the renderer rejects
    /// the reset.
    pub async fn reset_property_override(&self, handle: SceneHandle) -> Result<(), EngineError> {
        self.ask_actor(messages::ResetPropertyOverride { handle })
            .await
    }
}

impl<M> From<kameo::error::SendError<M, EngineError>> for EngineError {
    fn from(error: kameo::error::SendError<M, EngineError>) -> Self {
        match error {
            kameo::error::SendError::HandlerError(error) => error,
            error => Self::Platform(format!("engine actor mailbox failed: {error}")),
        }
    }
}

impl AudioFrameConsumer for WallpaperEngine {
    fn submit_audio_frames(
        &self,
        frames: InterleavedStereoF32<'_>,
    ) -> Result<(), AudioCaptureError> {
        self.submit_mono_audio_frames(MonoPcmF32::from_interleaved_stereo(&frames))
    }

    fn submit_mono_audio_frames(&self, frames: MonoPcmF32<'_>) -> Result<(), AudioCaptureError> {
        let Ok(mut resampler) = self.audio_response_resampler.try_lock() else {
            return Ok(());
        };

        for block in resampler.push(&frames) {
            self.backend
                .submit_audio_mono_frames(&block)
                .map_err(|error| AudioCaptureError::Engine(error.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        display::state::{DisplayAction, DisplayKey, DisplayRecord},
        engine::state::DisplayRuntimeRecord,
    };

    fn engine_with_display_records(records: Vec<DisplayRecord>) -> WallpaperEngine {
        let state = EngineState::with_display_model(DisplayStateModel { records });
        let snapshot = state.snapshot();
        let snapshots = Arc::new(EngineSnapshotPublisher::new(snapshot));
        let actor = EngineActorHandle::spawn(OweBackend, state, Arc::clone(&snapshots))
            .expect("test actor shell should start");
        let refresh_target = Arc::new(EngineRefreshTarget {
            actor: actor.actor().clone(),
        });
        let lifecycle = Arc::new(EngineLifecycle {
            refresh_target,
            display_callback: None,
        });
        WallpaperEngine {
            backend: OweBackend,
            snapshots,
            audio_response_resampler: Arc::new(
                std::sync::Mutex::new(AudioResponseResampler::new()),
            ),
            actor,
            lifecycle,
        }
    }

    #[test]
    fn default_engine_state_has_active_primary_record() {
        let state = EngineState::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default()).unwrap(),
        );

        let primary = state
            .display_records
            .iter()
            .find(|record| record.model.key == DisplayKey::Primary)
            .expect("primary record should exist");
        assert!(primary.model.window_active);
        assert!(primary.runtime.is_none());
    }

    #[test]
    fn with_config_seeds_display_records() {
        let external = crate::DisplayIdentity {
            uuid: Some("external".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let model = DisplayStateModel::from_config(crate::WallpaperEngineConfig {
            displays: vec![crate::DisplayConfig {
                selector: crate::DisplaySelector::Identity(external.clone()),
                window_active: true,
                wallpaper: None,
            }],
        })
        .unwrap();
        let state = EngineState::with_display_model(model);

        assert!(state.display_records.iter().any(|record| {
            record.model.key == DisplayKey::Identity(external.clone()) && record.model.window_active
        }));
    }

    #[test]
    fn scene_runtime_state_survives_snapshot_round_trip() {
        let desc = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let state = SceneRuntimeState::try_from(&desc).expect("state should build");

        assert_eq!(state.scaling_mode, ScalingMode::default());
        assert!(
            (state.scaling_factor - 1.0).abs() <= f64::EPSILON,
            "expected scaling factor {} to be within f64::EPSILON of 1.0",
            state.scaling_factor
        );
        assert_eq!(state.render_resolution, None);
        assert!(!state.paused);
    }

    #[test]
    fn scene_runtime_state_initial_uses_descriptor_scaling() {
        let desc = crate::project::SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .scaling_mode(ScalingMode::Fill)
        .scaling_factor(1.25)
        .build()
        .expect("scene should build");

        let state = SceneRuntimeState::try_from(&desc).expect("state should build");

        assert_eq!(state.scaling_mode, ScalingMode::Fill);
        assert!(
            (state.scaling_factor - 1.25).abs() <= f64::EPSILON,
            "expected scaling factor {} to be within f64::EPSILON of 1.25",
            state.scaling_factor
        );
    }

    #[test]
    fn runtime_refresh_required_only_when_descriptor_changes() {
        let desc = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut resized = desc.clone();
        resized.display.width = 2560;
        let mut different_wallpaper = desc.clone();
        different_wallpaper.scene_path = "/tmp/other.json".to_string();

        assert!(!RuntimeRefreshMode::from_transition(Some(&desc), &desc).is_required());
        assert!(RuntimeRefreshMode::from_transition(Some(&desc), &resized).is_required());
        assert!(
            RuntimeRefreshMode::from_transition(Some(&desc), &different_wallpaper).is_required()
        );
        assert!(RuntimeRefreshMode::from_transition(None, &desc).is_required());
    }

    #[test]
    fn runtime_refresh_mode_reopens_runtime_when_display_descriptor_changes() {
        let desc = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut resized = desc.clone();
        resized.display.width = 2560;
        let mut different_wallpaper = desc.clone();
        different_wallpaper.scene_path = "/tmp/other.json".to_string();

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&desc), &desc),
            RuntimeRefreshMode::Unchanged
        );
        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&desc), &resized),
            RuntimeRefreshMode::ReopenRuntime
        );
        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&desc), &different_wallpaper),
            RuntimeRefreshMode::RebuildExistingRuntime
        );
        assert_eq!(
            RuntimeRefreshMode::from_transition(None, &desc),
            RuntimeRefreshMode::OpenRuntime
        );
    }

    #[test]
    fn runtime_refresh_mode_rebuilds_when_shader_refresh_is_forced() {
        let current = crate::project::SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .shader_cache_path("/tmp/cache")
        .build()
        .expect("scene should build");
        let mut desired = current.clone();
        desired.force_shader_refresh = true;

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&current), &desired),
            RuntimeRefreshMode::RebuildExistingRuntime
        );
    }

    #[test]
    fn runtime_refresh_mode_treats_completed_shader_refresh_as_unchanged() {
        let mut current = crate::project::SceneDesc::builder(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .shader_cache_path("/tmp/cache")
        .force_shader_refresh(true)
        .build()
        .expect("scene should build");
        current.mark_shader_refresh_complete();
        let mut desired = current.clone();
        desired.force_shader_refresh = false;

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&current), &desired),
            RuntimeRefreshMode::Unchanged
        );
    }

    #[test]
    fn runtime_refresh_mode_updates_window_only_when_origin_changes() {
        let desc = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(3, 1920, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut moved = desc.clone();
        moved.display.x = -1920;

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&desc), &moved),
            RuntimeRefreshMode::UpdateWindowOnly
        );
    }

    #[test]
    fn runtime_refresh_mode_reopens_display_change_after_runtime_pause() {
        let mut current = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut desired = current.clone();
        current.paused = true;
        desired.display.width = 2560;

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&current), &desired),
            RuntimeRefreshMode::ReopenRuntime
        );
    }

    #[test]
    fn runtime_refresh_mode_ignores_initial_pause_change_after_open() {
        let current = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut desired = current.clone();
        desired.paused = true;

        assert_eq!(
            RuntimeRefreshMode::from_transition(Some(&current), &desired),
            RuntimeRefreshMode::Unchanged
        );
    }

    #[test]
    fn close_all_preserves_display_model_records() {
        let identity = crate::DisplayIdentity {
            uuid: Some("external-display".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let mut state = EngineState::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig {
                displays: vec![crate::DisplayConfig {
                    selector: crate::DisplaySelector::Identity(identity.clone()),
                    window_active: true,
                    wallpaper: None,
                }],
            })
            .unwrap(),
        );
        let live_key = DisplayKey::LiveDisplayId(42);
        let handle = state.reserve_handle_for_key(live_key.clone());
        let desc = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(42, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let last_runtime_state = SceneRuntimeState::try_from(&desc).unwrap();
        let record = state.ensure_record(DisplayRecord {
            key: live_key.clone(),
            live_display: Some(desc.display.clone()),
            assignment: None,
            window_active: true,
            runtime_open: true,
            primary_inheritance_consumed: false,
        });
        record.handle = Some(handle);
        record.last_runtime_state = Some(last_runtime_state.clone());

        state.close_all().unwrap();

        assert_eq!(state.next_handle, 2);
        assert!(state.handles_by_display.is_empty());
        assert!(state.displays_by_handle.is_empty());
        assert!(state.display_records.iter().any(|record| {
            record.model.key == DisplayKey::Primary
                && record.model.window_active
                && !record.model.runtime_open
        }));
        assert!(state.display_records.iter().any(|record| {
            record.model.key == DisplayKey::Identity(identity.clone())
                && record.model.window_active
                && !record.model.runtime_open
        }));
        let live_record = state
            .display_records
            .iter()
            .find(|record| record.model.key == live_key)
            .expect("live display record should be preserved");
        assert!(live_record.handle.is_none());
        assert!(live_record.runtime.is_none());
        assert!(!live_record.model.runtime_open);
        assert!(live_record.last_runtime_state.is_some());
    }

    #[test]
    fn reconcile_scene_configs_make_inputs_active_direct_assignments() {
        let display = crate::DisplayDesc::new(9, 0, 0, 1920, 1080, 1.0);
        let scene = crate::project::SceneDesc::new(
            display.clone(),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut model =
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default()).unwrap();

        model.apply_reconcile(&[scene]).unwrap();

        let key = DisplayKey::LiveDisplayId(display.display_id);
        let record = model
            .records
            .iter()
            .find(|record| record.key == key)
            .expect("display record should exist");
        assert!(record.window_active);
        assert!(matches!(
            record.assignment,
            Some(crate::WallpaperAssignment::Direct(_))
        ));
    }

    #[test]
    fn empty_reconcile_deactivates_active_connected_records() {
        let identity = crate::DisplayIdentity {
            uuid: Some("external-display".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let primary = crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0);
        let external =
            crate::DisplayDesc::with_identity(2, identity.clone(), 1920, 0, 1920, 1080, 1.0);
        let live = crate::DisplayDesc::new(3, 3840, 0, 1920, 1080, 1.0);
        let template = crate::project::SceneTemplate::builder("/tmp/project.json")
            .build()
            .unwrap();
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(primary),
                    assignment: Some(crate::WallpaperAssignment::Direct(template.clone())),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(identity),
                    live_display: Some(external),
                    assignment: Some(crate::WallpaperAssignment::Direct(template.clone())),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::LiveDisplayId(live.display_id),
                    live_display: Some(live),
                    assignment: Some(crate::WallpaperAssignment::Direct(template)),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model.apply_reconcile(&[]).unwrap();

        assert!(model.records.iter().all(|record| !record.window_active));
    }

    #[test]
    fn reconcile_scene_deactivates_existing_connected_alias_records() {
        let identity = crate::DisplayIdentity {
            uuid: Some("requested-display".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let display = crate::DisplayDesc::with_identity(7, identity.clone(), 0, 0, 1920, 1080, 1.0);
        let scene = crate::project::SceneDesc::new(
            display.clone(),
            "/tmp/requested.json",
            "/tmp/assets",
            60,
            false,
        );
        let template = crate::project::SceneTemplate::builder("/tmp/old.json")
            .build()
            .unwrap();
        let mut model = DisplayStateModel {
            records: vec![
                DisplayRecord {
                    key: DisplayKey::Primary,
                    live_display: Some(display.clone()),
                    assignment: Some(crate::WallpaperAssignment::Direct(template.clone())),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
                DisplayRecord {
                    key: DisplayKey::Identity(identity),
                    live_display: Some(display.clone()),
                    assignment: Some(crate::WallpaperAssignment::Direct(template)),
                    window_active: true,
                    runtime_open: true,
                    primary_inheritance_consumed: false,
                },
            ],
        };

        model.apply_reconcile(&[scene]).unwrap();

        let active_records: Vec<_> = model
            .records
            .iter()
            .filter(|record| record.window_active && record.live_display.as_ref() == Some(&display))
            .collect();
        assert_eq!(active_records.len(), 1);
        assert_eq!(active_records[0].key, DisplayKey::Primary);
        assert!(matches!(
            active_records[0].assignment,
            Some(crate::WallpaperAssignment::Direct(_))
        ));
    }

    #[test]
    fn reconcile_scene_for_current_primary_follows_primary_after_display_switch() {
        let old_primary = crate::DisplayDesc::with_identity(
            1,
            crate::DisplayIdentity {
                uuid: Some("old-primary".to_string()),
                vendor_id: Some(1),
                model_id: Some(2),
                serial_number: Some(3),
                unit_number: None,
                name: None,
            },
            0,
            0,
            3420,
            2214,
            2.0,
        );
        let new_primary = crate::DisplayDesc::with_identity(
            2,
            crate::DisplayIdentity {
                uuid: Some("new-primary".to_string()),
                vendor_id: Some(4),
                model_id: Some(5),
                serial_number: Some(6),
                unit_number: None,
                name: None,
            },
            0,
            0,
            1920,
            1080,
            1.0,
        );
        let old_primary_shifted = crate::DisplayDesc::with_identity(
            1,
            old_primary.identity.clone(),
            -1710,
            0,
            3420,
            2214,
            2.0,
        );
        let scene = crate::project::SceneDesc::new(
            old_primary.clone(),
            "/tmp/requested.json",
            "/tmp/assets",
            60,
            false,
        );
        let mut model = DisplayStateModel {
            records: vec![DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(old_primary.clone()),
                assignment: None,
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            }],
        };

        model.apply_reconcile(&[scene]).unwrap();

        let primary = model
            .records
            .iter_mut()
            .find(|record| record.key == DisplayKey::Primary)
            .expect("primary record should remain");
        assert!(primary.window_active);
        assert!(primary.assignment.is_some());
        primary.runtime_open = true;
        assert!(!model.records.iter().any(|record| {
            record.key == DisplayKey::LiveDisplayId(old_primary.display_id) && record.window_active
        }));

        let actions = model
            .refresh_connected(
                new_primary.clone(),
                vec![old_primary_shifted, new_primary.clone()],
            )
            .unwrap();

        let primary = model
            .records
            .iter()
            .find(|record| record.key == DisplayKey::Primary)
            .expect("primary record should remain after display switch");
        assert!(primary.window_active);
        assert_eq!(primary.live_display, Some(new_primary));
        assert!(primary.assignment.is_some());
        assert_eq!(actions, vec![DisplayAction::Rebuild(DisplayKey::Primary)]);
    }

    #[test]
    fn reconcile_keeps_disconnected_future_identity_records_untouched() {
        let identity = crate::DisplayIdentity {
            uuid: Some("future-display".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let template = crate::project::SceneTemplate::builder("/tmp/future.json")
            .build()
            .unwrap();
        let future_record = DisplayRecord {
            key: DisplayKey::Identity(identity),
            live_display: None,
            assignment: Some(crate::WallpaperAssignment::Direct(template)),
            window_active: true,
            runtime_open: false,
            primary_inheritance_consumed: false,
        };
        let mut model = DisplayStateModel {
            records: vec![future_record.clone()],
        };

        model.apply_reconcile(&[]).unwrap();

        assert_eq!(model.records, vec![future_record]);
    }

    #[test]
    fn reconcile_results_error_when_requested_handle_is_missing() {
        let scene = crate::project::SceneDesc::new(
            crate::DisplayDesc::new(11, 0, 0, 1920, 1080, 1.0),
            "/tmp/project.json",
            "/tmp/assets",
            60,
            false,
        );
        let state = EngineState::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default()).unwrap(),
        );

        let scenes = [scene];
        let model = DisplayStateModel {
            records: state
                .display_records
                .iter()
                .map(|record| record.model.clone())
                .collect(),
        };
        let error = scenes
            .reconcile_results(&model, &state.handles_by_display)
            .expect_err("missing requested handle should be an error");

        match error {
            EngineError::Platform(message) => {
                assert!(message.contains("missing scene handle for requested display 11"));
            }
            other => panic!("expected platform error, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_preserves_handle_reservation_when_runtime_no_longer_needed() {
        let primary = crate::DisplayDesc::with_identity(
            1,
            crate::DisplayIdentity {
                uuid: Some("primary".to_string()),
                vendor_id: Some(1),
                model_id: Some(2),
                serial_number: Some(3),
                unit_number: Some(4),
                name: None,
            },
            0,
            0,
            1920,
            1080,
            1.0,
        );
        let mut state = EngineState::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default()).unwrap(),
        );
        let key = DisplayKey::Primary;
        let handle = {
            let handle = state.reserve_handle_for_key(key.clone());
            let record = state
                .display_records
                .iter_mut()
                .find(|record| record.model.key == key)
                .expect("primary record should exist");
            record.handle = Some(handle);
            record.model.live_display = Some(primary.clone());
            record.model.runtime_open = true;
            handle
        };

        let snapshots = Arc::new(EngineSnapshotPublisher::new(state.snapshot()));
        let mut actor = EngineActor::new(OweBackend, state, snapshots);

        actor
            .reconcile_display_descriptors(primary.clone(), vec![primary])
            .unwrap();

        let record = actor
            .state
            .display_records
            .iter()
            .find(|record| record.model.key == key)
            .expect("primary record should remain");
        assert_eq!(record.handle, Some(handle));
        assert!(!record.model.runtime_open);
        assert!(record.runtime.is_none());
        assert_eq!(actor.state.handles_by_display.get(&key), Some(&handle));
        assert_eq!(actor.state.displays_by_handle.get(&handle), Some(&key));
    }

    #[test]
    fn display_record_blank_when_active_without_assignment() {
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0)),
                assignment: None,
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        assert!(!record.should_have_runtime());
    }

    #[test]
    fn display_record_needs_runtime_when_active_connected_and_assigned() {
        let template = crate::project::SceneTemplate::builder("/tmp/project.json")
            .build()
            .unwrap();
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(crate::DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0)),
                assignment: Some(crate::WallpaperAssignment::Direct(template)),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        assert!(record.should_have_runtime());
    }

    #[test]
    fn display_record_needs_runtime_for_connected_mirror_assignment() {
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::LiveDisplayId(2),
                live_display: Some(crate::DisplayDesc::new(2, 1920, 0, 1920, 1080, 1.0)),
                assignment: Some(crate::WallpaperAssignment::Mirror(
                    crate::DisplaySelector::Primary,
                )),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        assert!(record.should_have_runtime());
    }

    #[test]
    fn display_record_scene_desc_converts_direct_assignment() {
        let template = crate::project::SceneTemplate::builder("/tmp/project.json")
            .assets_path("/tmp/assets")
            .fps(30)
            .build()
            .unwrap();
        let display = crate::DisplayDesc::new(3, 0, 0, 1280, 720, 1.0);
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::LiveDisplayId(3),
                live_display: Some(display.clone()),
                assignment: Some(crate::WallpaperAssignment::Direct(template)),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        let desc = record
            .scene_desc()
            .expect("direct scene should convert")
            .expect("direct assignment should produce a scene");

        assert_eq!(desc.display, display);
        assert_eq!(desc.scene_path, "/tmp/project.json");
        assert_eq!(desc.assets_path, "/tmp/assets");
        assert_eq!(desc.fps, 30);
    }

    #[test]
    fn display_record_scene_desc_returns_none_when_disconnected() {
        let template = crate::project::SceneTemplate::builder("/tmp/project.json")
            .build()
            .unwrap();
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::LiveDisplayId(4),
                live_display: None,
                assignment: Some(crate::WallpaperAssignment::Direct(template)),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        assert!(record.scene_desc().unwrap().is_none());
    }

    #[test]
    fn display_record_scene_desc_errors_for_unresolved_mirror() {
        let record = DisplayRuntimeRecord {
            model: DisplayRecord {
                key: DisplayKey::LiveDisplayId(5),
                live_display: Some(crate::DisplayDesc::new(5, 0, 1080, 1920, 1080, 1.0)),
                assignment: Some(crate::WallpaperAssignment::Mirror(
                    crate::DisplaySelector::Primary,
                )),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            handle: None,
            runtime: None,
            last_runtime_state: None,
        };

        let error = record
            .scene_desc()
            .expect_err("unresolved mirror should error");
        match error {
            EngineError::Platform(message) => assert_eq!(
                message,
                "mirror assignments must be resolved before runtime creation"
            ),
            other => panic!("expected platform error, got {other:?}"),
        }
    }

    #[test]
    fn display_snapshot_returns_entries_for_configured_displays() {
        let primary_identity = crate::DisplayIdentity {
            uuid: Some("primary".to_string()),
            vendor_id: Some(1),
            model_id: Some(2),
            serial_number: Some(3),
            unit_number: Some(4),
            name: None,
        };
        let external_identity = crate::DisplayIdentity {
            uuid: Some("external".to_string()),
            vendor_id: Some(5),
            model_id: Some(6),
            serial_number: Some(7),
            unit_number: Some(8),
            name: None,
        };
        let disconnected_identity = crate::DisplayIdentity {
            uuid: Some("disconnected".to_string()),
            vendor_id: Some(9),
            model_id: Some(10),
            serial_number: Some(11),
            unit_number: Some(12),
            name: None,
        };
        let primary_display =
            crate::DisplayDesc::with_identity(1, primary_identity.clone(), 0, 0, 1920, 1080, 1.0);
        let external_display = crate::DisplayDesc::with_identity(
            2,
            external_identity.clone(),
            1920,
            0,
            1920,
            1080,
            1.0,
        );
        let primary_assignment = WallpaperAssignment::Direct(
            crate::project::SceneTemplate::builder("/tmp/primary.json")
                .build()
                .unwrap(),
        );
        let engine = engine_with_display_records(vec![
            DisplayRecord {
                key: DisplayKey::Primary,
                live_display: Some(primary_display.clone()),
                assignment: Some(primary_assignment.clone()),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            DisplayRecord {
                key: DisplayKey::Identity(external_identity.clone()),
                live_display: Some(external_display.clone()),
                assignment: None,
                window_active: false,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
            DisplayRecord {
                key: DisplayKey::Identity(disconnected_identity),
                live_display: None,
                assignment: Some(WallpaperAssignment::Mirror(DisplaySelector::Primary)),
                window_active: true,
                runtime_open: false,
                primary_inheritance_consumed: false,
            },
        ]);

        let snapshot = engine.display_snapshot();

        assert_eq!(snapshot.len(), 2);
        let primary_entry = snapshot
            .iter()
            .find(|entry| entry.identity == primary_identity)
            .expect("primary entry should be included");
        assert_eq!(primary_entry.desc, primary_display);
        assert_eq!(primary_entry.identity, primary_entry.desc.identity);
        assert!(primary_entry.handle.is_none());
        assert!(primary_entry.window_active);
        assert_eq!(primary_entry.assignment, Some(primary_assignment));

        let external_entry = snapshot
            .iter()
            .find(|entry| entry.identity == external_identity)
            .expect("external entry should be included");
        assert_eq!(external_entry.desc, external_display);
        assert_eq!(external_entry.identity, external_entry.desc.identity);
        assert!(external_entry.handle.is_none());
        assert!(!external_entry.window_active);
        assert!(external_entry.assignment.is_none());
        assert!(
            snapshot
                .iter()
                .all(|entry| entry.desc.display_id == 1 || entry.desc.display_id == 2)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn set_fps_returns_err_for_unknown_handle() {
        let engine = engine_with_display_records(Vec::new());

        let err = engine
            .set_fps(SceneHandle::new(u64::MAX), 60)
            .await
            .expect_err("unknown handle should fail");
        assert!(matches!(
            err,
            EngineError::InvalidInput(_) | EngineError::Platform(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn set_paused_returns_err_for_unknown_handle() {
        let engine = engine_with_display_records(Vec::new());

        let err = engine
            .set_paused(SceneHandle::new(u64::MAX), true)
            .await
            .expect_err("unknown handle should fail");
        assert!(matches!(
            err,
            EngineError::InvalidInput(_) | EngineError::Platform(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn set_all_paused_allows_empty_runtime_set() {
        let engine = engine_with_display_records(Vec::new());

        engine
            .set_all_paused(true)
            .await
            .expect("empty runtime set should be a no-op");
    }

    #[test]
    fn set_all_paused_skips_preserved_handles_without_runtime() {
        let mut state = EngineState::with_display_model(
            DisplayStateModel::from_config(crate::WallpaperEngineConfig::default()).unwrap(),
        );
        let key = DisplayKey::Primary;
        let handle = state.reserve_handle_for_key(key.clone());
        let record = state
            .display_records
            .iter_mut()
            .find(|record| record.model.key == key)
            .expect("primary record should exist");
        record.handle = Some(handle);
        record.runtime = None;
        record.model.runtime_open = false;

        let snapshots = Arc::new(EngineSnapshotPublisher::new(state.snapshot()));
        let mut actor = EngineActor::new(OweBackend, state, snapshots);

        actor
            .set_all_paused(true)
            .expect("inactive preserved handles should be ignored");
    }
}

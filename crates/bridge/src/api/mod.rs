mod error;
mod types;

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub use error::{BridgeError, BridgeErrorKind};
use futures_util::Future;
pub use types::{
    BridgeAppSnapshot, BridgeDisplayConfigRow, BridgeDisplayMode, BridgeDisplayMutationBundle,
    BridgeDisplaySettingsRow, BridgeLibraryScanStatus, BridgeLibrarySnapshot, BridgeLogLevel,
    BridgeLogStatus, BridgeMonitorInfoRow, BridgeMonitorInformationSnapshot, BridgePlaybackState,
    BridgePropertyDescriptor, BridgePropertyKind, BridgePropertyValue, BridgeScalingMode,
    BridgeSettingsSnapshot, BridgeSliderMetadata, BridgeSnapshotBundle, BridgeStorageStatus,
    BridgeWallpaperEntry, BridgeWallpaperKind, BridgeWallpaperMutationBundle,
    BridgeWallpaperOptionsSnapshot,
};
use wallpaper_core::{
    DisplaySelector, FirstFrameCallback, WallpaperAssignment, WallpaperEngine,
    media::audio::AudioVolume,
    project::{ScalingMode, SceneHandle, SceneResult},
};

#[cfg(test)]
use crate::actor::messages::{
    InjectDisplayForTest, InjectSceneProjectForTest, InjectSceneWallpaperConfigForTest,
    InjectWallpaperForTest, ReplaceLibraryForTest, ReplaceWallpaperConfigForTest, SetPowerSource,
};
#[cfg(test)]
use crate::config::WallpaperConfig;
#[cfg(test)]
use crate::engine::FakeEngineFacade;
use crate::{
    actor::{
        BridgeActorHandle,
        messages::{
            ApplyWallpaperOptions, Bootstrap, CancelWallpaperOptions, ClearShaderCache,
            EditProperty, EjectWallpaperFromDisplay, GetAllSnapshots, GetAppSnapshot,
            GetLibrarySnapshot, GetMonitorInformationSnapshot, GetSettingsSnapshot,
            GetWallpaperOptionsSnapshot, InitialFrameReady, PollMousePosition, RefreshDisplays,
            RefreshLibrary, RestorePropertyDefault, SelectWallpaper, SetAssetsDir,
            SetAudioResponseEnabled, SetDisplayConfigEnabled, SetDisplayEnabled, SetDisplayMode,
            SetFilter, SetGlobalPlayback, SetLaunchAtLogin, SetMirrorMuted, SetMirrorScalingFactor,
            SetMirrorScalingMode, SetMirrorTarget, SetMirrorTargetFps, SetMirrorVolume, SetMuted,
            SetPauseOnBatteryPower, SetScalingFactor, SetScalingMode, SetTargetFps,
            SetVolume, SetWorkshopDir, Shutdown,
        },
        state::BridgeActorState,
    },
    config::ConfigStore,
    engine::{EngineFacade, RealEngineFacade},
    login::LaunchAtLoginController,
    paths::BridgePaths,
    power::{PowerSource, PowerWatcher, SystemPowerSource},
};

pub(crate) fn bridge_log_status(status: crate::logging::LogStatus) -> BridgeLogStatus {
    BridgeLogStatus {
        logs_root: status.logs_root.to_string_lossy().into_owned(),
        active_session: status.active_session,
        active_file: status.active_file.to_string_lossy().into_owned(),
        active_file_size_bytes: status.active_file_size_bytes,
    }
}

pub struct BridgeBuilder<E: EngineFacade> {
    engine: E,
    state: Option<BridgeActorState>,
    config_store: Option<ConfigStore>,
    launch_at_login: LaunchAtLoginController,
    paths: BridgePaths,
    mouse_polling_enabled: bool,
    power_watching_enabled: bool,
    startup_power_source: Option<PowerSource>,
}

impl<E: EngineFacade> BridgeBuilder<E> {
    #[allow(clippy::single_call_fn)]
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            state: None,
            config_store: None,
            launch_at_login: LaunchAtLoginController::default(),
            paths: BridgePaths::new(),
            mouse_polling_enabled: true,
            power_watching_enabled: !cfg!(test),
            startup_power_source: None,
        }
    }

    #[cfg(test)]
    pub fn with_state(mut self, state: BridgeActorState) -> Self {
        self.state = Some(state);
        self
    }

    pub fn with_config_store(mut self, config_store: ConfigStore) -> Self {
        self.config_store = Some(config_store);
        self
    }

    pub fn with_paths(mut self, paths: BridgePaths) -> Self {
        self.paths = paths;
        self
    }

    #[cfg(test)]
    pub fn with_launch_at_login(mut self, launch_at_login: LaunchAtLoginController) -> Self {
        self.launch_at_login = launch_at_login;
        self
    }

    #[cfg(test)]
    pub fn with_mouse_polling_enabled(mut self, enabled: bool) -> Self {
        self.mouse_polling_enabled = enabled;
        self
    }

    #[cfg(test)]
    pub fn with_power_watching_enabled(mut self, enabled: bool) -> Self {
        self.power_watching_enabled = enabled;
        self
    }

    #[cfg(test)]
    pub fn with_startup_power_source(mut self, source: PowerSource) -> Self {
        self.startup_power_source = Some(source);
        self
    }

    pub fn build(self) -> Result<WallpaperBridge, BridgeError> {
        let startup_power_source = if cfg!(test) {
            self.startup_power_source
        } else {
            Some(
                self.startup_power_source
                    .unwrap_or_else(SystemPowerSource::current),
            )
        };
        let loaded_store = if let Some(store) = &self.config_store {
            Some(store.load()?)
        } else {
            None
        };

        let mut state = self.state.unwrap_or_else(|| {
            if let Some(loaded_store) = loaded_store {
                BridgeActorState::from_app_config(loaded_store.config)
            } else {
                BridgeActorState::default()
            }
        });
        if let Some(source) = startup_power_source {
            state.apply_startup_power_source(source);
            log::info!("startup power source sampled: {source:?}");
        }

        let engine = ArcEngineFacade::new(self.engine);
        let actor = BridgeActorHandle::spawn(
            state,
            engine.clone(),
            self.config_store.clone(),
            self.launch_at_login,
            self.paths,
        )?;
        let first_frame_notifier = FirstFrameNotifier {
            actor: actor.clone(),
        };
        engine.set_first_frame_callback(first_frame_notifier.callback());
        let mouse_poller = if self.mouse_polling_enabled {
            Some(MousePoller::spawn(actor.clone()))
        } else {
            None
        };
        let power_watcher = if self.power_watching_enabled {
            Some(PowerWatcher::spawn(actor.clone()))
        } else {
            None
        };

        Ok(WallpaperBridge {
            actor,
            mouse_poller,
            power_watcher,
            _config_store: self.config_store,
        })
    }
}

struct MousePoller {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

struct FirstFrameNotifier {
    actor: BridgeActorHandle<ArcEngineFacade>,
}

impl FirstFrameNotifier {
    fn callback(&self) -> FirstFrameCallback {
        let actor = self.actor.clone();
        Arc::new(move |handle| {
            log::info!("first frame ready for scene handle {}", handle.raw());
            let actor = actor.clone();
            let _ = std::thread::Builder::new()
                .name("wallpaper-bridge-first-frame".to_string())
                .spawn(move || {
                    let result: Result<BridgeSnapshotBundle, BridgeError> =
                        actor.blocking_ask(InitialFrameReady);
                    if let Err(error) = result {
                        log::debug!("initial-frame notification skipped: {error}");
                    }
                });
        })
    }
}

impl MousePoller {
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

    #[allow(clippy::single_call_fn)]
    fn spawn(actor: BridgeActorHandle<ArcEngineFacade>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::Builder::new()
            .name("wallpaper-bridge-mouse-poller".to_string())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    let poll_result: Result<(), BridgeError> =
                        actor.blocking_ask(PollMousePosition);
                    if let Err(error) = poll_result {
                        log::debug!("mouse poll skipped: {error}");
                    }
                    std::thread::sleep(Self::INTERVAL);
                }
            })
            .ok();

        Self { stop, worker }
    }
}

impl Drop for MousePoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(uniffi::Object)]
pub struct WallpaperBridge {
    actor: BridgeActorHandle<ArcEngineFacade>,
    #[allow(dead_code)]
    mouse_poller: Option<MousePoller>,
    #[allow(dead_code)]
    power_watcher: Option<PowerWatcher<ArcEngineFacade>>,
    _config_store: Option<ConfigStore>,
}

#[derive(Clone)]
struct ArcEngineFacade(Arc<dyn EngineFacade>);

impl ArcEngineFacade {
    #[allow(clippy::single_call_fn)]
    fn new<E: EngineFacade>(engine: E) -> Self {
        Self(Arc::new(engine))
    }
}

impl EngineFacade for ArcEngineFacade {
    fn reconcile_scenes(
        &self,
        scenes: Vec<wallpaper_core::project::SceneDesc>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SceneResult>, wallpaper_core::EngineError>> + Send>>
    {
        self.0.reconcile_scenes(scenes)
    }

    fn refresh_displays(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.refresh_displays()
    }

    fn display_snapshot(&self) -> Vec<wallpaper_core::DisplaySnapshotEntry> {
        self.0.display_snapshot()
    }

    fn close_all_scenes(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.close_all_scenes()
    }

    fn set_all_paused(
        &self,
        paused: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_all_paused(paused)
    }

    fn set_audio_volume(
        &self,
        handle: SceneHandle,
        volume: AudioVolume,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_audio_volume(handle, volume)
    }

    fn set_audio_muted(
        &self,
        handle: SceneHandle,
        muted: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_audio_muted(handle, muted)
    }

    fn set_audio_response_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_audio_response_enabled(handle, enabled)
    }

    fn set_audio_capture_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_audio_capture_enabled(handle, enabled)
    }

    fn set_scaling_mode(
        &self,
        handle: SceneHandle,
        mode: ScalingMode,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_scaling_mode(handle, mode)
    }

    fn set_scaling_factor(
        &self,
        handle: SceneHandle,
        factor: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_scaling_factor(handle, factor)
    }

    fn set_fps(
        &self,
        handle: SceneHandle,
        fps: u32,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_fps(handle, fps)
    }

    fn poll_mouse_position(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.poll_mouse_position()
    }

    fn set_mouse_position(
        &self,
        handle: SceneHandle,
        x: f64,
        y: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_mouse_position(handle, x, y)
    }

    fn set_mouse_button(
        &self,
        handle: SceneHandle,
        button: u32,
        pressed: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_mouse_button(handle, button, pressed)
    }

    fn set_mouse_entered(
        &self,
        handle: SceneHandle,
        entered: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), wallpaper_core::EngineError>> + Send>> {
        self.0.set_mouse_entered(handle, entered)
    }

    fn create_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> Pin<
        Box<dyn Future<Output = Result<Option<SceneHandle>, wallpaper_core::EngineError>> + Send>,
    > {
        self.0.create_window_for_display(selector)
    }

    fn set_wallpaper_for_display(
        &self,
        selector: DisplaySelector,
        assignment: WallpaperAssignment,
    ) -> Pin<
        Box<dyn Future<Output = Result<Option<SceneHandle>, wallpaper_core::EngineError>> + Send>,
    > {
        self.0.set_wallpaper_for_display(selector, assignment)
    }

    fn set_first_frame_callback(&self, callback: FirstFrameCallback) {
        self.0.set_first_frame_callback(callback);
    }
}

#[uniffi::export]
impl WallpaperBridge {
    #[uniffi::constructor]
    /// # Errors
    ///
    /// Returns an error when the native engine cannot start or persisted
    /// configuration cannot load.
    pub fn new() -> Result<Self, BridgeError> {
        let config_store = ConfigStore::open(ConfigStore::default_root());
        let loaded = config_store.load()?;
        let mut paths = BridgePaths::new();
        if let Some(ref dir) = loaded.config.general.workshop_dir {
            paths = paths.with_workshop_dir(dir.as_str());
        }
        if let Some(ref dir) = loaded.config.general.assets_dir {
            paths = paths.with_assets_dir(dir.as_str());
        }
        crate::logging::ApplicationLogger::install(&paths)?;
        let engine =
            WallpaperEngine::new().map_err(|error| BridgeError::engine(error.to_string()))?;

        BridgeBuilder::new(RealEngineFacade::new(engine))
            .with_config_store(config_store)
            .with_paths(paths)
            .build()
    }

    /// # Errors
    ///
    /// Returns an error when the bridge actor cannot produce an app snapshot.
    pub async fn app_snapshot(&self) -> Result<BridgeAppSnapshot, BridgeError> {
        self.actor.ask(GetAppSnapshot).await
    }

    /// # Errors
    ///
    /// Returns an error when the bridge actor cannot produce a library
    /// snapshot.
    pub async fn library_snapshot(&self) -> Result<BridgeLibrarySnapshot, BridgeError> {
        self.actor.ask(GetLibrarySnapshot).await
    }

    /// # Errors
    ///
    /// Returns an error when the bridge actor cannot produce monitor
    /// information.
    pub async fn monitor_information_snapshot(
        &self,
    ) -> Result<BridgeMonitorInformationSnapshot, BridgeError> {
        self.actor.ask(GetMonitorInformationSnapshot).await
    }

    /// # Errors
    ///
    /// Returns an error when the bridge actor cannot produce settings.
    pub async fn settings_snapshot(&self) -> Result<BridgeSettingsSnapshot, BridgeError> {
        self.actor.ask(GetSettingsSnapshot).await
    }

    /// # Errors
    ///
    /// Returns an error when GUI log emission cannot be accepted.
    #[allow(clippy::needless_pass_by_value)]
    pub fn emit_gui_log(
        &self,
        level: BridgeLogLevel,
        file: String,
        line: u32,
        message: String,
    ) -> Result<(), BridgeError> {
        let level = match level {
            BridgeLogLevel::Trace => log::Level::Trace,
            BridgeLogLevel::Debug => log::Level::Debug,
            BridgeLogLevel::Info => log::Level::Info,
            BridgeLogLevel::Warn => log::Level::Warn,
            BridgeLogLevel::Error => log::Level::Error,
        };
        crate::logging::ApplicationLogger::emit_gui_log(level, &file, line, &message);
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the logger has not been installed.
    pub fn log_folder_path(&self) -> Result<String, BridgeError> {
        crate::logging::ApplicationLogger::logs_root()
            .map(|path| path.to_string_lossy().into_owned())
            .ok_or_else(|| BridgeError::Error {
                kind: BridgeErrorKind::Io,
                message: "application logger is not installed".to_string(),
            })
    }

    /// # Errors
    ///
    /// Returns an error when a new log session cannot be created.
    pub fn clear_logs(&self) -> Result<BridgeLogStatus, BridgeError> {
        crate::logging::ApplicationLogger::clear().map(bridge_log_status)
    }

    /// # Errors
    ///
    /// Returns an error when the shader cache cannot be cleared or active
    /// scenes cannot be rebuilt.
    pub async fn clear_shader_cache(&self) -> Result<BridgeSettingsSnapshot, BridgeError> {
        self.actor.ask(ClearShaderCache).await
    }

    /// # Errors
    ///
    /// Returns an error when any snapshot in the bundle cannot be produced.
    pub async fn all_snapshots(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(GetAllSnapshots).await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown or its options cannot
    /// be read.
    pub async fn wallpaper_options_snapshot(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperOptionsSnapshot, BridgeError> {
        self.actor
            .ask(GetWallpaperOptionsSnapshot { wallpaper_id })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the library cannot be scanned.
    pub async fn refresh_library(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(RefreshLibrary).await
    }

    /// # Errors
    ///
    /// Returns an error when display refresh, library refresh, config load, or
    /// reconciliation fails.
    pub async fn bootstrap(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(Bootstrap).await
    }

    /// # Errors
    ///
    /// Returns an error when display refresh fails.
    pub async fn refresh_displays(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(RefreshDisplays).await
    }

    /// # Errors
    ///
    /// Returns an error when host pointer state cannot be forwarded to active
    /// wallpaper scenes.
    pub async fn poll_mouse_position(&self) -> Result<(), BridgeError> {
        self.actor.ask(PollMousePosition).await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown or selection cannot be
    /// committed.
    pub async fn select_wallpaper(&self, id: String) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(SelectWallpaper { id }).await
    }

    /// # Errors
    ///
    /// Returns an error when filter state cannot be persisted.
    pub async fn set_filter(
        &self,
        kind: BridgeWallpaperKind,
        enabled: bool,
    ) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(SetFilter { kind, enabled }).await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown, the volume is
    /// invalid, or persistence fails.
    pub async fn set_volume(
        &self,
        wallpaper_id: String,
        volume: f32,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetVolume {
                wallpaper_id,
                volume,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown or persistence fails.
    pub async fn set_muted(
        &self,
        wallpaper_id: String,
        muted: bool,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetMuted {
                wallpaper_id,
                muted,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown or persistence fails.
    pub async fn set_audio_response_enabled(
        &self,
        wallpaper_id: String,
        enabled: bool,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetAudioResponseEnabled {
                wallpaper_id,
                enabled,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper or display id is unknown.
    pub async fn set_display_config_enabled(
        &self,
        wallpaper_id: String,
        display_id: String,
        enabled: bool,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetDisplayConfigEnabled {
                wallpaper_id,
                display_id,
                enabled,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper or display id is unknown, or
    /// persistence fails.
    pub async fn set_scaling_mode(
        &self,
        wallpaper_id: String,
        display_id: String,
        mode: BridgeScalingMode,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetScalingMode {
                wallpaper_id,
                display_id,
                mode,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper or display id is unknown, the
    /// scaling factor is invalid, or live engine update fails.
    pub async fn edit_scaling_factor(
        &self,
        wallpaper_id: String,
        display_id: String,
        factor: f64,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetScalingFactor {
                wallpaper_id,
                display_id,
                factor,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper or display id is unknown, or
    /// persistence fails.
    pub async fn set_target_fps(
        &self,
        wallpaper_id: String,
        display_id: String,
        fps: u32,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(SetTargetFps {
                wallpaper_id,
                display_id,
                fps,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown or the display update
    /// fails.
    pub async fn set_display_enabled(
        &self,
        display_id: String,
        enabled: bool,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor
            .ask(SetDisplayEnabled {
                display_id,
                enabled,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown or the mode update
    /// fails.
    pub async fn set_display_mode(
        &self,
        display_id: String,
        mode: BridgeDisplayMode,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor.ask(SetDisplayMode { display_id, mode }).await
    }

    /// # Errors
    ///
    /// Returns an error when the display id, target display id, or mirror graph
    /// is invalid.
    pub async fn set_mirror_target(
        &self,
        display_id: String,
        target_display_id: String,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor
            .ask(SetMirrorTarget {
                display_id,
                target_display_id,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown, not in mirror mode, or
    /// the display update fails.
    pub async fn set_mirror_scaling_mode(
        &self,
        display_id: String,
        mode: BridgeScalingMode,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor
            .ask(SetMirrorScalingMode { display_id, mode })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown, not in mirror mode, the
    /// factor is invalid, or the display update fails.
    pub async fn set_mirror_scaling_factor(
        &self,
        display_id: String,
        factor: f64,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor
            .ask(SetMirrorScalingFactor { display_id, factor })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown, not in mirror mode, or
    /// the display update fails.
    pub async fn set_mirror_target_fps(
        &self,
        display_id: String,
        fps: u32,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor.ask(SetMirrorTargetFps { display_id, fps }).await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown, not in mirror mode, the
    /// volume is invalid, or the display update fails.
    pub async fn set_mirror_volume(
        &self,
        display_id: String,
        volume: f32,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor.ask(SetMirrorVolume { display_id, volume }).await
    }

    /// # Errors
    ///
    /// Returns an error when the display id is unknown, not in mirror mode, or
    /// the display update fails.
    pub async fn set_mirror_muted(
        &self,
        display_id: String,
        muted: bool,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor.ask(SetMirrorMuted { display_id, muted }).await
    }

    /// # Errors
    ///
    /// Returns an error when launch at login is unavailable or
    /// `ServiceManagement` rejects the update.
    pub async fn set_launch_at_login(
        &self,
        enabled: bool,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor.ask(SetLaunchAtLogin { enabled }).await
    }

    /// # Errors
    ///
    /// Returns an error when the setting cannot be persisted or an immediate
    /// power-policy playback transition fails.
    pub async fn set_pause_on_battery_power(
        &self,
        enabled: bool,
    ) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(SetPauseOnBatteryPower { enabled }).await
    }

    /// # Errors
    ///
    /// Returns an error when the directory cannot be set or the library
    /// cannot be rescanned.
    pub async fn set_workshop_dir(&self, dir: String) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(SetWorkshopDir { dir }).await
    }

    /// # Errors
    ///
    /// Returns an error when the directory cannot be persisted.
    pub async fn set_assets_dir(&self, dir: String) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor.ask(SetAssetsDir { dir }).await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id, property id, or value is
    /// invalid.
    pub async fn edit_property(
        &self,
        wallpaper_id: String,
        property_id: String,
        value: BridgePropertyValue,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(EditProperty {
                wallpaper_id,
                property_id,
                value,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper or property id is unknown.
    pub async fn restore_property_default(
        &self,
        wallpaper_id: String,
        property_id: String,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(RestorePropertyDefault {
                wallpaper_id,
                property_id,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when pending options cannot be applied or persisted.
    pub async fn apply_wallpaper_options(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor.ask(ApplyWallpaperOptions { wallpaper_id }).await
    }

    /// # Errors
    ///
    /// Returns an error when the wallpaper id is unknown.
    pub async fn cancel_wallpaper_options(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.actor
            .ask(CancelWallpaperOptions { wallpaper_id })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when pending options cannot be applied or persisted.
    pub async fn ok_wallpaper_options(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        self.apply_wallpaper_options(wallpaper_id).await
    }

    /// # Errors
    ///
    /// Returns an error when the engine cannot pause all scenes.
    pub async fn pause_all(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor
            .ask(SetGlobalPlayback {
                playback_state: BridgePlaybackState::Paused,
                paused: true,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the engine cannot resume all scenes.
    pub async fn play_all(&self) -> Result<BridgeSnapshotBundle, BridgeError> {
        self.actor
            .ask(SetGlobalPlayback {
                playback_state: BridgePlaybackState::Playing,
                paused: false,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the display update cannot be committed.
    pub async fn eject_wallpaper_from_display(
        &self,
        display_id: String,
        wallpaper_id: String,
    ) -> Result<BridgeDisplayMutationBundle, BridgeError> {
        self.actor
            .ask(EjectWallpaperFromDisplay {
                display_id,
                wallpaper_id,
            })
            .await
    }

    /// # Errors
    ///
    /// Returns an error when the engine cannot shut down active scenes.
    pub async fn shutdown(&self) -> Result<(), BridgeError> {
        self.actor.ask(Shutdown).await
    }
}

#[cfg(test)]
impl WallpaperBridge {
    /// # Panics
    ///
    /// Panics if the test bridge actor cannot be spawned.
    #[must_use]
    pub fn new_for_test() -> Self {
        BridgeBuilder::new(FakeEngineFacade::default())
            .build()
            .expect("tokio runtime and config load for wallpaper bridge")
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test wallpaper injection.
    pub async fn inject_wallpaper_for_test(
        &self,
        id: &str,
        title: &str,
        kind: BridgeWallpaperKind,
    ) {
        self.actor
            .ask(InjectWallpaperForTest {
                id: id.to_string(),
                title: title.to_string(),
                kind,
            })
            .await
            .expect("test wallpaper injection should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test scene wallpaper config injection.
    pub async fn inject_scene_wallpaper_config_for_test(&self, id: &str, title: &str) {
        self.actor
            .ask(InjectSceneWallpaperConfigForTest {
                id: id.to_string(),
                title: title.to_string(),
            })
            .await
            .expect("test scene wallpaper config injection should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test scene project injection.
    pub async fn inject_scene_project_for_test(&self, id: &str, title: &str, project_json: &str) {
        self.actor
            .ask(InjectSceneProjectForTest {
                id: id.to_string(),
                title: title.to_string(),
                project_json: project_json.to_string(),
            })
            .await
            .expect("test scene project injection should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test display injection.
    pub async fn inject_display_for_test(&self, display_id: &str, title: &str) {
        self.actor
            .ask(InjectDisplayForTest {
                display_id: display_id.to_string(),
                title: title.to_string(),
            })
            .await
            .expect("test display injection should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test library replacement.
    pub async fn replace_library_for_test(&self, entries: Vec<BridgeWallpaperEntry>) {
        self.actor
            .ask(ReplaceLibraryForTest { entries })
            .await
            .expect("test library replacement should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test wallpaper config replacement.
    pub async fn replace_wallpaper_config_for_test(&self, id: &str, config: WallpaperConfig) {
        self.actor
            .ask(ReplaceWallpaperConfigForTest {
                id: id.to_string(),
                config,
            })
            .await
            .expect("test wallpaper config replacement should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test power source update.
    pub async fn set_power_source_for_test(&self, source: crate::power::PowerSource) {
        self.actor
            .ask(SetPowerSource {
                source,
                initial_sample: false,
            })
            .await
            .expect("test power source update should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test initial power source update.
    pub async fn set_initial_power_source_for_test(&self, source: crate::power::PowerSource) {
        self.actor
            .ask(SetPowerSource {
                source,
                initial_sample: true,
            })
            .await
            .expect("test initial power source update should succeed");
    }

    /// # Panics
    ///
    /// Panics if the actor rejects the test initial-frame readiness update.
    pub async fn initial_frame_ready_for_test(&self) {
        self.actor
            .ask(InitialFrameReady)
            .await
            .expect("test initial-frame readiness update should succeed");
    }
}

impl From<&crate::library::WallpaperEntry> for BridgeWallpaperEntry {
    fn from(entry: &crate::library::WallpaperEntry) -> Self {
        Self {
            id: entry.workshop_id.clone(),
            title: entry.title.clone(),
            kind: BridgeWallpaperKind::from(entry.project_type),
            supported: entry.supported,
            active: false,
            selected: false,
            preview_path: entry
                .preview_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
        }
    }
}

impl From<BridgeScalingMode> for ScalingMode {
    fn from(value: BridgeScalingMode) -> Self {
        match value {
            BridgeScalingMode::None => Self::None,
            BridgeScalingMode::Stretch => Self::Stretch,
            BridgeScalingMode::Match => Self::Fit,
            BridgeScalingMode::Fill => Self::Fill,
        }
    }
}

impl From<ScalingMode> for BridgeScalingMode {
    fn from(value: ScalingMode) -> Self {
        match value {
            ScalingMode::None => Self::None,
            ScalingMode::Stretch => Self::Stretch,
            ScalingMode::Fit => Self::Match,
            ScalingMode::Fill => Self::Fill,
        }
    }
}

impl From<&crate::project::PropertyKind> for BridgePropertyKind {
    fn from(value: &crate::project::PropertyKind) -> Self {
        match value {
            crate::project::PropertyKind::Slider => Self::Slider,
            crate::project::PropertyKind::Combo => Self::Combo,
            crate::project::PropertyKind::Bool => Self::Bool,
            crate::project::PropertyKind::Color => Self::Color,
            crate::project::PropertyKind::TextInput => Self::TextInput,
            crate::project::PropertyKind::Text => Self::Text,
            crate::project::PropertyKind::Group => Self::Group,
            crate::project::PropertyKind::Directory => Self::Directory,
            crate::project::PropertyKind::Unknown(_) => Self::Unknown,
        }
    }
}

impl From<crate::project::PropertyValue> for BridgePropertyValue {
    fn from(value: crate::project::PropertyValue) -> Self {
        match value {
            crate::project::PropertyValue::Bool(value) => Self::Bool { value },
            crate::project::PropertyValue::Number(value) => Self::Number { value },
            crate::project::PropertyValue::String(value) => Self::String { value },
            crate::project::PropertyValue::ColorRgb(red, green, blue) => Self::ColorRgb {
                red: f64::from(red),
                green: f64::from(green),
                blue: f64::from(blue),
            },
            crate::project::PropertyValue::Null => Self::Empty,
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl From<BridgePropertyValue> for crate::project::PropertyValue {
    fn from(value: BridgePropertyValue) -> Self {
        match value {
            BridgePropertyValue::Bool { value } => Self::Bool(value),
            BridgePropertyValue::Number { value } => Self::Number(value),
            BridgePropertyValue::String { value } => Self::String(value),
            BridgePropertyValue::ColorRgb { red, green, blue } => {
                Self::ColorRgb(red as f32, green as f32, blue as f32)
            }
            BridgePropertyValue::Empty => Self::Null,
        }
    }
}

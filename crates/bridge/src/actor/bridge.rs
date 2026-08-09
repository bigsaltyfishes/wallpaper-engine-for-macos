use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
};

use kameo::{
    actor::{ActorRef, Spawn},
    error::SendError,
    message::{Context, Message},
    reply::{DelegatedReply, Reply},
};
use wallpaper_core::{
    DisplayIdentity, DisplaySelector, DisplaySnapshotEntry, WallpaperAssignment,
    media::audio::AudioVolume,
    project::{ScalingMode, SceneDesc, SceneHandle},
};

use crate::{
    actor::{
        messages::{
            self, ApplyWallpaperOptions, Bootstrap, CancelWallpaperOptions, ClearShaderCache,
            CommitApplyAfterReconcile, CommitDisplayAfterReconcile, CompleteRestoreAfterReconcile,
            EditProperty, EjectWallpaperFromDisplay, GetAllSnapshots, GetAppSnapshot,
            GetLibrarySnapshot, GetMonitorInformationSnapshot, GetSettingsSnapshot,
            GetWallpaperOptionsSnapshot, InitialFrameReady, InjectDisplayForTest,
            InjectSceneProjectForTest, InjectSceneWallpaperConfigForTest, InjectWallpaperForTest,
            PollMousePosition, ReconcileFailed, RefreshDisplays, RefreshLibrary,
            ReplaceLibraryForTest, ReplaceWallpaperConfigForTest, RestorePropertyDefault,
            SelectWallpaper, SetAudioResponseEnabled, SetDisplayConfigEnabled, SetDisplayEnabled,
            SetDisplayMode, SetFilter, SetGlobalPlayback, SetLaunchAtLogin, SetMirrorMuted,
            SetMirrorScalingFactor, SetMirrorScalingMode, SetMirrorTarget, SetMirrorTargetFps,
            SetMirrorVolume, SetMuted, SetPauseOnBatteryPower, SetPowerSource, SetScalingFactor,
            SetScalingMode, SetTargetFps, SetVolume, Shutdown, SetDisplayHorizontalFlip,
        },
        state::BridgeActorState,
    },
    api::{
        BridgeAppSnapshot, BridgeDisplayMode, BridgeDisplayMutationBundle,
        BridgeDisplaySettingsRow, BridgeError, BridgeLibraryScanStatus, BridgeLibrarySnapshot,
        BridgePlaybackState, BridgePropertyValue, BridgeScalingMode, BridgeSnapshotBundle,
        BridgeWallpaperEntry, BridgeWallpaperKind, BridgeWallpaperMutationBundle,
    },
    config::{AppConfig, ConfigStore, SerializedSelector, WallpaperConfig},
    display::{DisplaySelectorExt, DisplaySnapshotExt},
    engine::{ActivationInputs, EngineFacade},
    library::scan,
    login::LaunchAtLoginController,
    paths::BridgePaths,
    project::{ProjectModel, PropertyKind, PropertyMetadata, PropertyValue},
    state::drafts::WallpaperOptionsDraft,
};

const PRIMARY_DISPLAY_ID: &str = "primary";
const IDENTITY_DISPLAY_ID_PREFIX: &str = "identity:";
const INDEPENDENT_DISPLAY_MODE: &str = "independent";
const MIRROR_DISPLAY_MODE: &str = "mirror";

macro_rules! is_color_channel_valid {
    ($color:expr) => {
        ($color.is_finite() && (0.0..=1.0).contains(&$color))
    };
}

#[derive(kameo::Actor)]
pub struct BridgeActor<E: EngineFacade> {
    pub state: BridgeActorState,
    generation: u64,
    latest_reconcile_generation: u64,
    reconciled_generation: u64,
    repair_after_reconcile_generation: Option<u64>,
    active_restore_generation: Option<u64>,
    restore_requested_after_active: bool,
    #[allow(dead_code)]
    pub engine: E,
    #[allow(dead_code)]
    pub config_store: Option<ConfigStore>,
    launch_at_login: LaunchAtLoginController,
    paths: BridgePaths,
}

enum PlaybackChangeOrigin {
    Manual,
    Power,
}

#[derive(Clone)]
pub struct BridgeActorHandle<E: EngineFacade> {
    actor: ActorRef<BridgeActor<E>>,
    runtime: Option<Arc<tokio::runtime::Runtime>>,
}

impl<E: EngineFacade> Drop for BridgeActorHandle<E> {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };

        match Arc::try_unwrap(runtime) {
            Ok(runtime) => runtime.shutdown_background(),
            Err(runtime) => {
                self.runtime = Some(runtime);
            }
        }
    }
}

impl<E: EngineFacade> BridgeActorHandle<E> {
    #[allow(clippy::single_call_fn)]
    pub fn spawn(
        state: BridgeActorState,
        engine: E,
        config_store: Option<ConfigStore>,
        launch_at_login: LaunchAtLoginController,
        paths: BridgePaths,
    ) -> Result<Self, BridgeError> {
        let actor = BridgeActor {
            state,
            generation: 0,
            latest_reconcile_generation: 0,
            reconciled_generation: 0,
            repair_after_reconcile_generation: None,
            active_restore_generation: None,
            restore_requested_after_active: false,
            engine,
            config_store,
            launch_at_login,
            paths,
        };

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("wallpaper-bridge-actor-runtime")
                .build()
                .map_err(|error| {
                    BridgeError::engine(format!("failed to start bridge actor runtime: {error}"))
                })?,
        );
        let actor = {
            let _guard = runtime.enter();
            BridgeActor::spawn_in_thread(actor)
        };

        Ok(Self {
            actor,
            runtime: Some(runtime),
        })
    }

    pub async fn ask<M, T>(&self, message: M) -> Result<T, BridgeError>
    where
        BridgeActor<E>: Message<M>,
        <BridgeActor<E> as Message<M>>::Reply: kameo::reply::Reply<Ok = T, Error = BridgeError>,
        M: Send + 'static,
        T: Send + 'static,
    {
        self.actor.ask(message).await.map_err(map_send_error)
    }

    pub fn blocking_ask<M, T>(&self, message: M) -> Result<T, BridgeError>
    where
        BridgeActor<E>: Message<M>,
        <BridgeActor<E> as Message<M>>::Reply: Reply<Ok = T, Error = BridgeError>,
        M: Send + 'static,
        T: Send + 'static,
    {
        self.actor
            .ask(message)
            .blocking_send()
            .map_err(map_send_error)
    }
}

fn map_send_error<M>(error: SendError<M, BridgeError>) -> BridgeError {
    match error {
        SendError::HandlerError(error) => error,
        other => BridgeError::engine(other.to_string()),
    }
}

fn duplicate_error(error: &BridgeError) -> BridgeError {
    BridgeError::Error {
        kind: error.kind(),
        message: error.message().to_string(),
    }
}

impl<E: EngineFacade + Clone> BridgeActor<E> {
    fn app_snapshot(&self) -> BridgeAppSnapshot {
        BridgeAppSnapshot {
            playback_state: self.state.playback_state,
            selected_wallpaper_id: self.state.selected_wallpaper_id.clone(),
            active_wallpaper_ids: self.state.active_wallpaper_ids.clone(),
            errors: self.state.errors.clone(),
        }
    }

    fn library_snapshot(&self) -> BridgeLibrarySnapshot {
        let selected_wallpaper_id = self.state.selected_wallpaper_id.as_deref();
        let active_wallpaper_ids = self.state.active_wallpaper_ids.clone();
        let all_wallpapers = self
            .state
            .library
            .iter()
            .map(|entry| {
                let mut entry = entry.clone();
                entry.selected = selected_wallpaper_id == Some(entry.id.as_str());
                entry.active = active_wallpaper_ids.iter().any(|id| id == &entry.id);
                entry
            })
            .collect::<Vec<_>>();
        let wallpapers = all_wallpapers
            .iter()
            .filter(|entry| self.state.filter_enabled(entry.kind))
            .cloned()
            .collect::<Vec<_>>();

        BridgeLibrarySnapshot {
            scene_count: all_wallpapers
                .iter()
                .filter(|entry| entry.kind == BridgeWallpaperKind::ProjectScene)
                .count() as u64,
            video_count: all_wallpapers
                .iter()
                .filter(|entry| entry.kind == BridgeWallpaperKind::Video)
                .count() as u64,
            webpage_count: all_wallpapers
                .iter()
                .filter(|entry| entry.kind == BridgeWallpaperKind::Webpage)
                .count() as u64,
            unknown_count: all_wallpapers
                .iter()
                .filter(|entry| entry.kind == BridgeWallpaperKind::Unknown)
                .count() as u64,
            wallpapers,
            scan_status: BridgeLibraryScanStatus {
                scanning: false,
                done: 0,
                total: 0,
            },
        }
    }

    fn all_snapshots(&self) -> BridgeSnapshotBundle {
        let displays = self.engine.display_snapshot();
        let launch_at_login = self.launch_at_login.status();
        BridgeSnapshotBundle {
            app: self.app_snapshot(),
            library: self.library_snapshot(),
            wallpaper_options: None,
            monitor_information: self.state.monitor_info(&displays),
            settings: self.state.settings(&displays, launch_at_login, &self.paths),
        }
    }

    fn snapshots_with_options(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeSnapshotBundle, BridgeError> {
        let displays = self.engine.display_snapshot();
        let launch_at_login = self.launch_at_login.status();
        Ok(BridgeSnapshotBundle {
            app: self.app_snapshot(),
            library: self.library_snapshot(),
            wallpaper_options: Some(self.state.options(&displays, wallpaper_id)?),
            monitor_information: self.state.monitor_info(&displays),
            settings: self.state.settings(&displays, launch_at_login, &self.paths),
        })
    }

    fn wallpaper_bundle(
        &self,
        wallpaper_id: String,
    ) -> Result<BridgeWallpaperMutationBundle, BridgeError> {
        let displays = self.engine.display_snapshot();
        let launch_at_login = self.launch_at_login.status();
        Ok(BridgeWallpaperMutationBundle {
            app: self.app_snapshot(),
            library: self.library_snapshot(),
            wallpaper_options: self.state.options(&displays, wallpaper_id)?,
            monitor_information: self.state.monitor_info(&displays),
            settings: self.state.settings(&displays, launch_at_login, &self.paths),
        })
    }

    fn display_bundle(&self) -> BridgeDisplayMutationBundle {
        let displays = self.engine.display_snapshot();
        let launch_at_login = self.launch_at_login.status();
        BridgeDisplayMutationBundle {
            app: self.app_snapshot(),
            library: self.library_snapshot(),
            monitor_information: self.state.monitor_info(&displays),
            settings: self.state.settings(&displays, launch_at_login, &self.paths),
        }
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn reserve_reconcile(&mut self) -> u64 {
        self.bump_generation();
        self.latest_reconcile_generation = self.generation;
        self.repair_after_reconcile_generation = None;
        self.generation
    }

    fn finish_reconcile(&mut self, generation: u64, actor: ActorRef<BridgeActor<E>>) {
        self.reconciled_generation = generation;
        if self.active_restore_generation == Some(generation) {
            self.active_restore_generation = None;
            if self.restore_requested_after_active {
                self.restore_requested_after_active = false;
                self.spawn_restore(actor);
                return;
            }
        }
        if self.repair_after_reconcile_generation == Some(generation) {
            self.spawn_restore(actor);
        }
    }

    fn reconcile_current(&self, generation: u64) -> bool {
        generation == self.generation && generation == self.latest_reconcile_generation
    }

    fn stale_reconcile(&mut self, generation: u64, actor: ActorRef<BridgeActor<E>>) {
        if self.active_restore_generation == Some(generation) {
            self.active_restore_generation = None;
            if self.restore_requested_after_active {
                self.restore_requested_after_active = false;
                self.spawn_restore(actor);
                return;
            }
        }

        let latest_generation = self.latest_reconcile_generation;
        if latest_generation == 0 {
            return;
        }

        if generation == latest_generation || self.reconciled_generation == latest_generation {
            self.spawn_restore(actor);
        } else {
            self.repair_after_reconcile_generation = Some(latest_generation);
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn reconcile_failure(
        &mut self,
        generation: u64,
        error: BridgeError,
        actor: ActorRef<BridgeActor<E>>,
    ) {
        self.state.errors.push(error.message().to_string());

        if self.reconcile_current(generation) {
            self.spawn_restore(actor);
            return;
        }

        if self.active_restore_generation.is_some() {
            self.restore_requested_after_active = true;
        } else {
            self.spawn_restore(actor);
        }
    }

    fn wallpaper_handles(&self, wallpaper_id: &str, include_mirrors: bool) -> Vec<SceneHandle> {
        let displays = self.engine.display_snapshot();
        let mut handles = Vec::new();
        let mut used_display_ids = Vec::new();

        for monitor in self.state.app_config.monitors.iter().filter(|monitor| {
            monitor.enabled
                && (monitor.wallpaper.as_deref() == Some(wallpaper_id)
                    || include_mirrors
                        && monitor.mode.eq_ignore_ascii_case(MIRROR_DISPLAY_MODE)
                        && monitor
                            .mirror_target
                            .as_ref()
                            .is_some_and(|target| self.target_has_wallpaper(target, wallpaper_id)))
        }) {
            let Some(snapshot) = monitor.selector.to_selector().resolve_display(&displays) else {
                continue;
            };
            let Some(handle) = snapshot.handle else {
                continue;
            };
            if used_display_ids.contains(&snapshot.desc.display_id) {
                continue;
            }

            handles.push(handle);
            used_display_ids.push(snapshot.desc.display_id);
        }

        handles
    }

    fn target_has_wallpaper(&self, selector: &SerializedSelector, wallpaper_id: &str) -> bool {
        self.state.app_config.monitors.iter().any(|monitor| {
            monitor.enabled
                && monitor.wallpaper.as_deref() == Some(wallpaper_id)
                && monitor.selector == *selector
        })
    }

    async fn set_playback(
        &mut self,
        playback_state: BridgePlaybackState,
        paused: bool,
        origin: PlaybackChangeOrigin,
    ) -> Result<(), BridgeError> {
        self.engine
            .set_all_paused(paused)
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))?;
        self.state.playback_state = playback_state;
        match origin {
            PlaybackChangeOrigin::Manual => {
                if !paused && self.state.power_source == crate::power::PowerSource::Battery {
                    self.state.auto_paused_for_battery = false;
                    self.state.battery_pause_suppressed = true;
                } else if paused {
                    self.state.auto_paused_for_battery = false;
                }
            }
            PlaybackChangeOrigin::Power => {}
        }
        self.bump_generation();
        Ok(())
    }

    async fn apply_power_policy(&mut self) -> Result<(), BridgeError> {
        if !self.state.app_config.power.pause_on_battery_power {
            self.state.auto_paused_for_battery = false;
            self.state.battery_pause_suppressed = false;
            self.state.pending_battery_pause_after_initial_frame = false;
            return Ok(());
        }

        match self.state.power_source {
            crate::power::PowerSource::Battery => {
                if self.state.playback_state == BridgePlaybackState::Playing
                    && !self.state.battery_pause_suppressed
                {
                    if self.state.pending_battery_pause_after_initial_frame {
                        return Ok(());
                    }
                    log::info!("pausing wallpaper playback on battery power");
                    self.set_playback(
                        BridgePlaybackState::Paused,
                        true,
                        PlaybackChangeOrigin::Power,
                    )
                    .await?;
                    self.state.auto_paused_for_battery = true;
                }
            }
            crate::power::PowerSource::External => {
                self.state.battery_pause_suppressed = false;
                self.state.pending_battery_pause_after_initial_frame = false;
                if self.state.auto_paused_for_battery {
                    log::info!("resuming wallpaper playback on external power");
                    self.set_playback(
                        BridgePlaybackState::Playing,
                        false,
                        PlaybackChangeOrigin::Power,
                    )
                    .await?;
                    self.state.auto_paused_for_battery = false;
                }
            }
            crate::power::PowerSource::Unknown => {}
        }
        Ok(())
    }

    fn display_handle(
        &self,
        wallpaper_id: &str,
        selector: &SerializedSelector,
    ) -> Option<SceneHandle> {
        let displays = self.engine.display_snapshot();

        self.state
            .app_config
            .monitors
            .iter()
            .find(|monitor| {
                monitor.enabled
                    && monitor.wallpaper.as_deref() == Some(wallpaper_id)
                    && &monitor.selector == selector
            })
            .and_then(|monitor| monitor.selector.to_selector().resolve_display(&displays))
            .and_then(|snapshot| snapshot.handle)
    }

    fn mirror_display_handle(
        &self,
        selector: &SerializedSelector,
        displays: &[DisplaySnapshotEntry],
    ) -> Option<SceneHandle> {
        self.state
            .app_config
            .monitors
            .iter()
            .find(|monitor| {
                monitor.enabled
                    && monitor.mode.eq_ignore_ascii_case(MIRROR_DISPLAY_MODE)
                    && &monitor.selector == selector
            })
            .and_then(|monitor| monitor.selector.to_selector().resolve_display(displays))
            .and_then(|snapshot| snapshot.handle)
    }

    fn commit_app_config(&mut self, app_config: AppConfig) -> Result<(), BridgeError> {
        if let Some(store) = &self.config_store {
            store.save_app_config(&app_config)?;
        }
        self.state.app_config = app_config;
        Ok(())
    }

    fn persist_app_config(&mut self) -> Result<(), BridgeError> {
        if let Some(store) = &self.config_store {
            store.save_app_config(&self.state.app_config)?;
        }
        Ok(())
    }

    fn save_wallpaper(
        &mut self,
        wallpaper_id: String,
        config: WallpaperConfig,
    ) -> Result<(), BridgeError> {
        if let Some(store) = &self.config_store {
            store.save_wallpaper(&config)?;
        }
        self.state.wallpaper_configs.insert(wallpaper_id, config);
        Ok(())
    }

    fn refresh_library(&mut self) -> Result<(), BridgeError> {
        let workshop_root = self.paths.steam_workshop_root();
        let entries = scan(&workshop_root)?;
        let project_models = entries
            .iter()
            .filter_map(|entry| {
                let project_json = workshop_root.join(&entry.workshop_id).join("project.json");
                ProjectModel::load(&entry.workshop_id, project_json)
                    .ok()
                    .map(|model| (entry.workshop_id.clone(), model))
            })
            .collect();
        self.state.replace_library(
            entries
                .iter()
                .map(crate::api::BridgeWallpaperEntry::from)
                .collect(),
        );
        self.state.project_models = project_models;
        Ok(())
    }

    fn load_wallpapers(&mut self) -> Result<(), BridgeError> {
        let Some(store) = &self.config_store else {
            return Ok(());
        };
        let missing_ids = self
            .state
            .configured_ids()
            .iter()
            .filter(|id| !self.state.wallpaper_configs.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        let loaded = missing_ids
            .iter()
            .map(|id| store.load_wallpaper(id).map(|config| (id.clone(), config)))
            .collect::<Result<Vec<_>, _>>()?;

        for (id, config) in loaded {
            self.state.wallpaper_configs.insert(id, config);
        }
        Ok(())
    }

    async fn refresh_displays(&mut self) -> Result<(), BridgeError> {
        self.engine
            .refresh_displays()
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))?;
        self.sync_displays()
    }

    fn sync_displays(&mut self) -> Result<(), BridgeError> {
        let displays = self.engine.display_snapshot();
        let mut next = self.state.app_config.clone();
        if !next.sync_known_monitors(&displays) {
            return Ok(());
        }

        if let Some(store) = &self.config_store {
            store.save_app_config(&next)?;
        }
        self.state.app_config = next;
        self.state.rebase_drafts();
        Ok(())
    }

    async fn reconcile_configured(&mut self) -> Result<(), BridgeError> {
        self.load_wallpapers()?;
        let has_configured_wallpapers = !self.state.configured_ids().is_empty();
        let displays = self.engine.display_snapshot();
        if !has_configured_wallpapers || displays.is_empty() {
            return Ok(());
        }
        if let Some(scenes) = self.unchanged_configured_scenes(&displays)? {
            self.state.set_active_ids_from_scenes(&scenes);
            return Ok(());
        }

        let app_config = self.state.app_config.clone();
        let wallpaper_configs = self.state.wallpaper_configs.clone();
        let scenes = self
            .reconcile_engine(app_config.clone(), wallpaper_configs)
            .await?;
        self.state.set_active_ids_from_scenes(&scenes);
        Ok(())
    }

    fn unchanged_configured_scenes(
        &self,
        displays: &[DisplaySnapshotEntry],
    ) -> Result<Option<Vec<SceneDesc>>, BridgeError> {
        let scenes = ActivationInputs {
            app_config: &self.state.app_config,
            wallpapers: &self.state.wallpaper_configs,
            displays,
            paused: self.playback_paused(),
            paths: &self.paths,
            force_shader_refresh: false,
        }
        .build()?;
        let snapshot = self.engine.display_snapshot();
        let has_direct_runtime = |entry: &&DisplaySnapshotEntry| {
            entry.handle.is_some()
                && matches!(entry.assignment, Some(WallpaperAssignment::Direct(_)))
        };

        if scenes.len() != snapshot.iter().filter(has_direct_runtime).count() {
            return Ok(None);
        }

        if scenes.iter().all(|scene| {
            snapshot.iter().any(|entry| {
                entry.handle.is_some()
                    && entry
                        .assignment
                        .as_ref()
                        .is_some_and(|assignment| match assignment {
                            WallpaperAssignment::Direct(template) => {
                                template.for_display(scene.display.clone()) == *scene
                            }
                            WallpaperAssignment::Mirror(_) => false,
                        })
            })
        }) {
            Ok(Some(scenes))
        } else {
            Ok(None)
        }
    }

    fn save_configs(
        &self,
        app_config: &AppConfig,
        wallpaper_config: &WallpaperConfig,
    ) -> Result<(), BridgeError> {
        if let Some(store) = &self.config_store {
            store.save_app_config(app_config)?;
            store.save_wallpaper(wallpaper_config)?;
        }
        Ok(())
    }

    fn playback_paused(&self) -> bool {
        self.state.playback_state == crate::api::BridgePlaybackState::Paused
    }

    fn spawn_restore(&mut self, actor: ActorRef<BridgeActor<E>>) {
        if self.active_restore_generation.is_some() {
            self.restore_requested_after_active = true;
            return;
        }
        let generation = self.reserve_reconcile();
        self.active_restore_generation = Some(generation);
        let engine = self.engine.clone();
        let app_config = self.state.app_config.clone();
        let wallpaper_configs = self.state.wallpaper_configs.clone();
        let paused = self.playback_paused();
        let paths = self.paths.clone();
        tokio::spawn(async move {
            let result =
                reconcile_with(engine, app_config, wallpaper_configs, paused, paths, false).await;
            let _ = actor
                .ask(CompleteRestoreAfterReconcile { result, generation })
                .await;
        });
    }

    #[allow(clippy::needless_pass_by_value)]
    fn commit_display_settings(
        &mut self,
        app_config: AppConfig,
        display_settings: BTreeMap<String, BridgeDisplaySettingsRow>,
        scenes: Vec<SceneDesc>,
    ) -> Result<(), BridgeError> {
        if let Some(store) = &self.config_store {
            store.save_app_config(&app_config)?;
        }
        self.state.app_config = app_config;
        self.state.display_settings = display_settings;
        self.state.set_active_ids_from_scenes(&scenes);
        self.state.rebase_drafts();
        Ok(())
    }

    async fn reconcile_engine(
        &self,
        app_config: AppConfig,
        wallpaper_configs: BTreeMap<String, WallpaperConfig>,
    ) -> Result<Vec<SceneDesc>, BridgeError> {
        reconcile_with(
            self.engine.clone(),
            app_config,
            wallpaper_configs,
            self.playback_paused(),
            self.paths.clone(),
            false,
        )
        .await
    }

    fn delegate_display(
        &mut self,
        app_config: AppConfig,
        display_settings: BTreeMap<String, BridgeDisplaySettingsRow>,
        ctx: &mut Context<Self, DelegatedReply<messages::DisplayMutationReply>>,
    ) -> DelegatedReply<messages::DisplayMutationReply> {
        let wallpaper_configs = self.state.wallpaper_configs.clone();
        let generation = self.reserve_reconcile();
        let paused = self.playback_paused();
        let actor = ctx.actor_ref().clone();
        let engine = self.engine.clone();
        let paths = self.paths.clone();
        ctx.spawn(async move {
            let scenes = match reconcile_with(
                engine,
                app_config.clone(),
                wallpaper_configs.clone(),
                paused,
                paths,
                false,
            )
            .await
            {
                Ok(scenes) => scenes,
                Err(error) => {
                    let _ = actor
                        .ask(ReconcileFailed {
                            error: duplicate_error(&error),
                            generation,
                        })
                        .await;
                    return Err(error);
                }
            };
            actor
                .ask(CommitDisplayAfterReconcile {
                    app_config,
                    wallpaper_configs,
                    display_settings,
                    scenes,
                    generation,
                })
                .await
                .map_err(map_send_error)
        })
    }

    fn selector_for(
        &self,
        display_id: &str,
        displays: &[DisplaySnapshotEntry],
    ) -> Result<SerializedSelector, BridgeError> {
        if display_id == PRIMARY_DISPLAY_ID {
            if displays.is_empty() && !self.state.display_settings.contains_key(display_id) {
                return Err(BridgeError::invalid_input(format!(
                    "unknown display id {display_id}"
                )));
            }
            return Ok(SerializedSelector::Primary);
        }

        if let Some(encoded) = display_id.strip_prefix(IDENTITY_DISPLAY_ID_PREFIX) {
            let identity = serde_json::from_str::<DisplayIdentity>(encoded).map_err(|error| {
                BridgeError::invalid_input(format!("invalid display identity selector: {error}"))
            })?;
            let selector = SerializedSelector::from_selector(&DisplaySelector::Identity(identity));
            if displays.is_empty()
                || displays
                    .iter()
                    .any(|display| selector.to_selector().matches_display(display))
            {
                return Ok(selector);
            }
            return Err(BridgeError::invalid_input(format!(
                "unknown display id {display_id}"
            )));
        }

        if displays.is_empty() {
            return if self.state.display_settings.contains_key(display_id) {
                Ok(SerializedSelector::LiveDisplayId {
                    display_id: Self::parse_display_id(display_id)?,
                })
            } else {
                Err(BridgeError::invalid_input(format!(
                    "unknown display id {display_id}"
                )))
            };
        }

        let display_id_u32 = Self::parse_display_id(display_id)?;
        let display = displays
            .iter()
            .find(|display| display.desc.display_id == display_id_u32)
            .ok_or_else(|| {
                BridgeError::invalid_input(format!("unknown display id {display_id}"))
            })?;
        if displays
            .first()
            .is_some_and(|primary| display.matches_primary(primary))
        {
            return Ok(SerializedSelector::Primary);
        }

        Ok(display.connected_selector())
    }

    fn normalized_config(&self, displays: &[DisplaySnapshotEntry]) -> AppConfig {
        self.state.app_config.normalized(displays)
    }

    fn display_rows(
        &self,
        app_config: &AppConfig,
        displays: &[DisplaySnapshotEntry],
    ) -> BTreeMap<String, BridgeDisplaySettingsRow> {
        if displays.is_empty() {
            return self
                .state
                .display_settings
                .iter()
                .map(|(display_id, row)| {
                    let mut row = row.clone();
                    if let Some(monitor) = display_id.parse::<u32>().ok().and_then(|display_id| {
                        app_config.monitors.iter().find(|monitor| {
                            monitor.selector == SerializedSelector::LiveDisplayId { display_id }
                        })
                    }) {
                        row.enabled = monitor.enabled;
                        row.mode = if monitor.mode.eq_ignore_ascii_case(MIRROR_DISPLAY_MODE) {
                            BridgeDisplayMode::Mirror
                        } else {
                            BridgeDisplayMode::Standalone
                        };
                        row.selected_mirror_target =
                            monitor
                                .mirror_target
                                .as_ref()
                                .map(|selector| match selector {
                                    SerializedSelector::LiveDisplayId { display_id } => {
                                        display_id.to_string()
                                    }
                                    SerializedSelector::Primary => PRIMARY_DISPLAY_ID.to_string(),
                                    SerializedSelector::Identity { .. } => {
                                        let DisplaySelector::Identity(identity) =
                                            selector.to_selector()
                                        else {
                                            unreachable!(
                                                "identity selector must convert to identity"
                                            )
                                        };
                                        format!(
                                            "{IDENTITY_DISPLAY_ID_PREFIX}{}",
                                            serde_json::to_string(&identity).expect(
                                                "display identity selector should serialize"
                                            )
                                        )
                                    }
                                });
                    } else if display_id == PRIMARY_DISPLAY_ID {
                        row.enabled = true;
                        row.mode = BridgeDisplayMode::Standalone;
                        row.selected_mirror_target = None;
                    }
                    (display_id.clone(), row)
                })
                .collect();
        }

        let mut candidate_state = self.state.clone();
        candidate_state.app_config = app_config.clone();
        candidate_state
            .settings(displays, self.launch_at_login.status(), &self.paths)
            .displays
            .into_iter()
            .map(|row| (row.display_id.clone(), row))
            .collect()
    }

    fn validate_display_settings(
        app_config: &AppConfig,
        displays: &[DisplaySnapshotEntry],
        display_settings: &BTreeMap<String, BridgeDisplaySettingsRow>,
    ) -> Result<(), BridgeError> {
        let valid_ids = if displays.is_empty() {
            display_settings
                .keys()
                .filter_map(|display_id| display_id.parse::<u32>().ok())
                .collect::<Vec<_>>()
        } else {
            displays
                .iter()
                .map(|display| display.desc.display_id)
                .collect::<Vec<_>>()
        };

        for monitor in &app_config.monitors {
            let source_id = match &monitor.selector {
                SerializedSelector::Primary => valid_ids.first().copied(),
                SerializedSelector::LiveDisplayId { display_id } => {
                    valid_ids.contains(display_id).then_some(*display_id)
                }
                SerializedSelector::Identity { .. } => displays
                    .iter()
                    .find(|display| monitor.selector.to_selector().matches_display(display))
                    .map(|display| display.desc.display_id),
            };
            let Some(source_id) = source_id else {
                continue;
            };

            if monitor.mode != MIRROR_DISPLAY_MODE {
                continue;
            }

            let target = monitor.mirror_target.as_ref().ok_or_else(|| {
                BridgeError::invalid_input(format!(
                    "mirror mode for display {source_id} requires a target"
                ))
            })?;
            if Self::valid_target(target, source_id, displays, &valid_ids).is_none() {
                return Err(BridgeError::invalid_input(format!(
                    "unknown mirror target for display {source_id}"
                )));
            }
            app_config.validate_mirror_change(&monitor.selector, target)?;
        }

        Ok(())
    }

    fn valid_target(
        selector: &SerializedSelector,
        display_id: u32,
        displays: &[DisplaySnapshotEntry],
        valid_ids: &[u32],
    ) -> Option<u32> {
        match selector {
            SerializedSelector::LiveDisplayId {
                display_id: target_id,
            } => valid_ids
                .contains(target_id)
                .then_some(*target_id)
                .filter(|target_id| *target_id != display_id),
            SerializedSelector::Primary => displays
                .first()
                .map(|display| display.desc.display_id)
                .or_else(|| valid_ids.first().copied())
                .filter(|target_id| *target_id != display_id),
            SerializedSelector::Identity { .. } => displays
                .iter()
                .find(|display| {
                    display.desc.display_id != display_id
                        && selector.to_selector().matches_display(display)
                })
                .map(|display| display.desc.display_id),
        }
    }

    fn source_display_id(
        &self,
        selector: &SerializedSelector,
        displays: &[DisplaySnapshotEntry],
    ) -> Result<u32, BridgeError> {
        match selector {
            SerializedSelector::Primary => displays
                .first()
                .map(|display| display.desc.display_id)
                .or_else(|| {
                    self.state
                        .display_settings
                        .get(PRIMARY_DISPLAY_ID)
                        .and_then(|row| {
                            row.title
                                .rsplit_once(" - Primary)")?
                                .0
                                .rsplit_once('(')?
                                .1
                                .parse()
                                .ok()
                        })
                })
                .ok_or_else(|| BridgeError::invalid_input("unknown display id primary")),
            SerializedSelector::LiveDisplayId { display_id } => Ok(*display_id),
            SerializedSelector::Identity { .. } => displays
                .iter()
                .find(|display| selector.to_selector().matches_display(display))
                .map(|display| display.desc.display_id)
                .ok_or_else(|| BridgeError::invalid_input("unknown display identity")),
        }
    }

    fn parse_display_id(display_id: &str) -> Result<u32, BridgeError> {
        display_id
            .parse::<u32>()
            .map_err(|_| BridgeError::invalid_input(format!("invalid display id {display_id}")))
    }

    fn monitor_settings_mut(
        app_config: &mut AppConfig,
        selector: SerializedSelector,
    ) -> &mut crate::config::MonitorSettingsCfg {
        if let Some(index) = app_config
            .monitor_settings
            .iter()
            .position(|settings| settings.selector == selector)
        {
            return &mut app_config.monitor_settings[index];
        }

        app_config
            .monitor_settings
            .push(crate::config::MonitorSettingsCfg {
                selector,
                ..crate::config::MonitorSettingsCfg::default()
            });
        app_config
            .monitor_settings
            .last_mut()
            .expect("settings entry was just inserted")
    }

    fn require_mirror_monitor<'a>(
        app_config: &'a AppConfig,
        selector: &SerializedSelector,
    ) -> Result<&'a crate::config::MonitorCfg, BridgeError> {
        let monitor = app_config
            .monitors
            .iter()
            .find(|monitor| &monitor.selector == selector)
            .ok_or_else(|| BridgeError::invalid_input("unknown mirror display"))?;
        if monitor.enabled && monitor.mode.eq_ignore_ascii_case(MIRROR_DISPLAY_MODE) {
            Ok(monitor)
        } else {
            Err(BridgeError::invalid_input(
                "display is not configured for mirror mode",
            ))
        }
    }
}

async fn reconcile_with<E: EngineFacade>(
    engine: E,
    app_config: AppConfig,
    wallpaper_configs: BTreeMap<String, WallpaperConfig>,
    paused: bool,
    paths: BridgePaths,
    force_shader_refresh: bool,
) -> Result<Vec<SceneDesc>, BridgeError> {
    let displays = engine.display_snapshot();
    let scenes = ActivationInputs {
        app_config: &app_config,
        wallpapers: &wallpaper_configs,
        displays: &displays,
        paused,
        paths: &paths,
        force_shader_refresh,
    }
    .build()?;
    let results = engine
        .reconcile_scenes(scenes.clone())
        .await
        .map_err(|error| BridgeError::engine(error.to_string()))?;

    // Sync audios
    let mut audio_handles = Vec::new();
    let mut used_display_ids = HashSet::new();
    for scene in &scenes {
        let Some(result) = results
            .iter()
            .find(|result| result.display_id == scene.display.display_id)
        else {
            continue;
        };

        if used_display_ids.contains(&result.display_id) {
            continue;
        }

        audio_handles.push((scene, result.handle));
        used_display_ids.insert(result.display_id);
    }
    for (scene, handle) in audio_handles {
        engine
            .set_audio_volume(handle, scene.audio_volume)
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))?;
        engine
            .set_audio_muted(handle, scene.audio_muted)
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))?;
        engine
            .set_audio_capture_enabled(handle, scene.audio_response_enabled)
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))?;
    }

    Ok(scenes)
}

impl<E: EngineFacade + Clone> Message<Bootstrap> for BridgeActor<E> {
    type Reply = messages::BootstrapReply;

    async fn handle(
        &mut self,
        _msg: Bootstrap,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.errors.clear();

        if let Err(error) = self.refresh_displays().await {
            self.state.errors.push(error.message().to_string());
        }
        if let Err(error) = self.refresh_library() {
            self.state.errors.push(error.message().to_string());
        }
        if let Err(error) = self.load_wallpapers() {
            self.state.errors.push(error.message().to_string());
        }
        if let Err(error) = self.reconcile_configured().await {
            self.state.errors.push(error.message().to_string());
        }

        self.bump_generation();
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<GetAllSnapshots> for BridgeActor<E> {
    type Reply = messages::AllSnapshotsReply;

    async fn handle(
        &mut self,
        _msg: GetAllSnapshots,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<GetAppSnapshot> for BridgeActor<E> {
    type Reply = messages::AppSnapshotReply;

    async fn handle(
        &mut self,
        _msg: GetAppSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        Ok(self.app_snapshot())
    }
}

impl<E: EngineFacade + Clone> Message<GetLibrarySnapshot> for BridgeActor<E> {
    type Reply = messages::LibrarySnapshotReply;

    async fn handle(
        &mut self,
        _msg: GetLibrarySnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        Ok(self.library_snapshot())
    }
}

impl<E: EngineFacade + Clone> Message<GetMonitorInformationSnapshot> for BridgeActor<E> {
    type Reply = messages::MonitorInformationSnapshotReply;

    async fn handle(
        &mut self,
        _msg: GetMonitorInformationSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        Ok(self.state.monitor_info(&displays))
    }
}

impl<E: EngineFacade + Clone> Message<GetSettingsSnapshot> for BridgeActor<E> {
    type Reply = messages::SettingsSnapshotReply;

    async fn handle(
        &mut self,
        _msg: GetSettingsSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        Ok(self
            .state
            .settings(&displays, self.launch_at_login.status(), &self.paths))
    }
}

impl<E: EngineFacade + Clone> Message<PollMousePosition> for BridgeActor<E> {
    type Reply = messages::PollMousePositionReply;

    async fn handle(
        &mut self,
        _msg: PollMousePosition,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.engine
            .poll_mouse_position()
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))
    }
}

impl<E: EngineFacade + Clone> Message<ClearShaderCache> for BridgeActor<E> {
    type Reply = messages::ClearShaderCacheReply;

    async fn handle(
        &mut self,
        _msg: ClearShaderCache,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let cache_root = self.paths.shader_cache_root();
        match fs::remove_dir_all(&cache_root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BridgeError::Error {
                    kind: crate::api::BridgeErrorKind::Io,
                    message: format!("failed to clear shader cache: {error}"),
                });
            }
        }
        fs::create_dir_all(&cache_root).map_err(|error| BridgeError::Error {
            kind: crate::api::BridgeErrorKind::Io,
            message: format!("failed to recreate shader cache: {error}"),
        })?;

        self.load_wallpapers()?;
        let app_config = self.state.app_config.clone();
        let wallpaper_configs = self.state.wallpaper_configs.clone();
        let scenes = reconcile_with(
            self.engine.clone(),
            app_config,
            wallpaper_configs,
            self.playback_paused(),
            self.paths.clone(),
            true,
        )
        .await?;
        self.state.set_active_ids_from_scenes(&scenes);
        self.bump_generation();

        let displays = self.engine.display_snapshot();
        Ok(self
            .state
            .settings(&displays, self.launch_at_login.status(), &self.paths))
    }
}

impl<E: EngineFacade + Clone> Message<GetWallpaperOptionsSnapshot> for BridgeActor<E> {
    type Reply = messages::WallpaperOptionsSnapshotReply;

    async fn handle(
        &mut self,
        msg: GetWallpaperOptionsSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        self.state.options(&displays, msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<InjectWallpaperForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: InjectWallpaperForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.library.push(BridgeWallpaperEntry {
            id: msg.id,
            title: msg.title,
            kind: msg.kind,
            supported: matches!(
                msg.kind,
                BridgeWallpaperKind::ProjectScene
                    | BridgeWallpaperKind::Video
                    | BridgeWallpaperKind::Webpage
            ),
            active: false,
            selected: false,
            preview_path: None,
        });
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<InjectSceneWallpaperConfigForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: InjectSceneWallpaperConfigForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let config = WallpaperConfig::new_for(&msg.id, "scene");
        self.state.library.push(BridgeWallpaperEntry {
            id: msg.id.clone(),
            title: msg.title,
            kind: BridgeWallpaperKind::ProjectScene,
            supported: true,
            active: false,
            selected: false,
            preview_path: None,
        });
        self.state.wallpaper_drafts.insert(
            msg.id.clone(),
            WallpaperOptionsDraft::from_committed(config.clone()),
        );
        self.state.wallpaper_configs.insert(msg.id, config);
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<InjectSceneProjectForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: InjectSceneProjectForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let model = ProjectModel::parse(&msg.id, &msg.project_json)
            .map_err(|error| BridgeError::invalid_input(error.to_string()))?;
        let entry = BridgeWallpaperEntry {
            id: msg.id.clone(),
            title: if msg.title.is_empty() {
                model.title.clone()
            } else {
                msg.title
            },
            kind: BridgeWallpaperKind::from(model.project_type),
            supported: true,
            active: false,
            selected: false,
            preview_path: None,
        };
        let config = WallpaperConfig::new_for(&msg.id, "scene");
        self.state.library.push(entry);
        self.state.project_models.insert(msg.id.clone(), model);
        self.state.wallpaper_drafts.insert(
            msg.id.clone(),
            WallpaperOptionsDraft::from_committed(config.clone()),
        );
        self.state.wallpaper_configs.insert(msg.id, config);
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<InjectDisplayForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: InjectDisplayForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let selector = SerializedSelector::LiveDisplayId {
            display_id: Self::parse_display_id(&msg.display_id)?,
        };
        self.state.app_config.ensure_monitor(selector);
        let mirror_targets = self
            .state
            .display_settings
            .keys()
            .filter(|target| target.as_str() != msg.display_id)
            .cloned()
            .collect();
        self.state.display_settings.insert(
            msg.display_id.clone(),
            BridgeDisplaySettingsRow {
                display_id: msg.display_id.clone(),
                title: msg.title,
                enabled: true,
                mode: BridgeDisplayMode::Standalone,
                mirror_targets,
                selected_mirror_target: None,
                scaling_mode: BridgeScalingMode::Match,
                scaling_factor: 1.0,
                target_fps: 60,
                max_fps: 60,
                muted: false,
                volume: 1.0,
                horizontal_flip: false,
            },
        );
        let ids = self
            .state
            .display_settings
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for row in self.state.display_settings.values_mut() {
            row.mirror_targets = ids
                .iter()
                .filter(|target| target.as_str() != row.display_id)
                .cloned()
                .collect();
        }
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<ReplaceLibraryForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: ReplaceLibraryForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.replace_library(msg.entries);
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<ReplaceWallpaperConfigForTest> for BridgeActor<E> {
    type Reply = messages::TestMutationReply;

    async fn handle(
        &mut self,
        msg: ReplaceWallpaperConfigForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.wallpaper_drafts.insert(
            msg.id.clone(),
            WallpaperOptionsDraft::from_committed(msg.config.clone()),
        );
        self.state.wallpaper_configs.insert(msg.id, msg.config);
        self.bump_generation();
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<SelectWallpaper> for BridgeActor<E> {
    type Reply = messages::SelectWallpaperReply;

    async fn handle(
        &mut self,
        msg: SelectWallpaper,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.ensure_wallpaper_exists(&msg.id)?;
        let should_load_wallpaper = !self.state.wallpaper_configs.contains_key(&msg.id)
            && !self.state.wallpaper_drafts.contains_key(&msg.id);
        let loaded_wallpaper = if should_load_wallpaper {
            self.config_store
                .as_ref()
                .map(|store| store.load_wallpaper(&msg.id))
                .transpose()?
        } else {
            None
        };

        if let Some(config) = loaded_wallpaper {
            self.state
                .wallpaper_configs
                .entry(msg.id.clone())
                .or_insert(config);
        }
        self.state.app_config.general.last_selected_wallpaper = Some(msg.id.clone());
        self.state.selected_wallpaper_id = Some(msg.id.clone());
        self.generation = self.generation.wrapping_add(1);
        self.snapshots_with_options(msg.id)
    }
}

impl<E: EngineFacade + Clone> Message<RefreshLibrary> for BridgeActor<E> {
    type Reply = messages::RefreshLibraryReply;

    async fn handle(
        &mut self,
        _msg: RefreshLibrary,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.refresh_library()?;
        self.bump_generation();
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<RefreshDisplays> for BridgeActor<E> {
    type Reply = messages::RefreshDisplaysReply;

    async fn handle(
        &mut self,
        _msg: RefreshDisplays,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.refresh_displays().await?;
        self.reconcile_configured().await?;
        self.bump_generation();
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<SetFilter> for BridgeActor<E> {
    type Reply = messages::SetFilterReply;

    async fn handle(
        &mut self,
        msg: SetFilter,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match msg.kind {
            BridgeWallpaperKind::ProjectScene => self.state.filter_scene = msg.enabled,
            BridgeWallpaperKind::Video => self.state.filter_video = msg.enabled,
            BridgeWallpaperKind::Webpage => self.state.filter_webpage = msg.enabled,
            BridgeWallpaperKind::Unknown => self.state.filter_unknown = msg.enabled,
        }
        self.generation = self.generation.wrapping_add(1);
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<SetDisplayEnabled> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetDisplayEnabledReply>;

    async fn handle(
        &mut self,
        msg: SetDisplayEnabled,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let mut app_config = self.normalized_config(&displays);
        let enabled = if selector == SerializedSelector::Primary {
            true
        } else {
            msg.enabled
        };
        let monitor = app_config.ensure_monitor(selector.clone());
        monitor.enabled = enabled;
        if monitor.selector == SerializedSelector::Primary {
            monitor.mode = INDEPENDENT_DISPLAY_MODE.to_string();
            monitor.mirror_target = None;
        }
        let display_settings = self.display_rows(&app_config, &displays);
        reply_try!(Self::validate_display_settings(
            &app_config,
            &displays,
            &display_settings,
        ));
        self.delegate_display(app_config, display_settings, ctx)
    }
}

impl<E: EngineFacade + Clone> Message<SetDisplayMode> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetDisplayModeReply>;

    async fn handle(
        &mut self,
        msg: SetDisplayMode,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        if selector == SerializedSelector::Primary && msg.mode == BridgeDisplayMode::Mirror {
            return ctx.reply(Err(BridgeError::invalid_input(
                "primary display cannot use mirror mode",
            )));
        }
        let source_display_id = reply_try!(self.source_display_id(&selector, &displays));
        let mut app_config = self.normalized_config(&displays);
        let valid_ids = if displays.is_empty() {
            self.state
                .display_settings
                .keys()
                .filter_map(|display_id| display_id.parse::<u32>().ok())
                .collect::<Vec<_>>()
        } else {
            self.normalized_config(&displays)
                .monitor_rows(&displays)
                .into_iter()
                .filter(|row| row.connected)
                .filter_map(|row| row.display_index.and_then(|index| displays.get(index)))
                .map(|display| display.desc.display_id)
                .collect::<Vec<_>>()
        };

        match msg.mode {
            BridgeDisplayMode::Standalone => {
                let monitor = app_config.ensure_monitor(selector.clone());
                monitor.mode = INDEPENDENT_DISPLAY_MODE.to_string();
                monitor.mirror_target = None;
                if monitor.selector == SerializedSelector::Primary {
                    monitor.enabled = true;
                }
            }
            BridgeDisplayMode::Mirror => {
                let target_display_id = if let Some(target) = app_config
                    .monitors
                    .iter()
                    .find(|monitor| monitor.selector == selector)
                    .and_then(|monitor| monitor.mirror_target.as_ref())
                    .and_then(|target| {
                        Self::valid_target(target, source_display_id, &displays, &valid_ids)
                    }) {
                    target
                } else {
                    reply_try!(
                        valid_ids
                            .iter()
                            .copied()
                            .find(|target| *target != source_display_id)
                            .ok_or_else(|| {
                                BridgeError::invalid_input("mirror mode requires another display")
                            })
                    )
                };
                let target =
                    reply_try!(self.selector_for(&target_display_id.to_string(), &displays));
                reply_try!(app_config.validate_mirror_change(&selector, &target));
                let monitor = app_config.ensure_monitor(selector);
                monitor.enabled = true;
                monitor.mode = MIRROR_DISPLAY_MODE.to_string();
                monitor.mirror_target = Some(target);
            }
        }

        let display_settings = self.display_rows(&app_config, &displays);
        reply_try!(Self::validate_display_settings(
            &app_config,
            &displays,
            &display_settings,
        ));
        self.delegate_display(app_config, display_settings, ctx)
    }
}

impl<E: EngineFacade + Clone> Message<SetDisplayHorizontalFlip> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetDisplayHorizontalFlipReply>;

    async fn handle(
        &mut self,
        msg: SetDisplayHorizontalFlip,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let mut app_config = self.normalized_config(&displays);
        Self::monitor_settings_mut(&mut app_config, selector).horizontal_flip = msg.enabled;
        let display_settings = self.display_rows(&app_config, &displays);
        reply_try!(Self::validate_display_settings(
            &app_config,
            &displays,
            &display_settings,
        ));
        self.delegate_display(app_config, display_settings, ctx)
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorTarget> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorTargetReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorTarget,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        if selector == SerializedSelector::Primary {
            return ctx.reply(Err(BridgeError::invalid_input(
                "primary display cannot use mirror mode",
            )));
        }
        let target = reply_try!(self.selector_for(&msg.target_display_id, &displays));
        let mut app_config = self.normalized_config(&displays);
        reply_try!(app_config.validate_mirror_change(&selector, &target));
        let monitor = app_config.ensure_monitor(selector);
        monitor.enabled = true;
        monitor.mode = MIRROR_DISPLAY_MODE.to_string();
        monitor.mirror_target = Some(target);
        let display_settings = self.display_rows(&app_config, &displays);
        reply_try!(Self::validate_display_settings(
            &app_config,
            &displays,
            &display_settings,
        ));
        self.delegate_display(app_config, display_settings, ctx)
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorScalingMode> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorScalingModeReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorScalingMode,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let handle = self.mirror_display_handle(&selector, &displays);
        let mut app_config = self.normalized_config(&displays);
        reply_try!(Self::require_mirror_monitor(&app_config, &selector));
        let scaling_mode = ScalingMode::from(msg.mode);
        Self::monitor_settings_mut(&mut app_config, selector).scaling_mode =
            scaling_mode.to_string();
        reply_try!(self.commit_app_config(app_config));
        if let Some(handle) = handle {
            reply_try!(
                self.engine
                    .set_scaling_mode(handle, scaling_mode)
                    .await
                    .map_err(|error| BridgeError::engine(error.to_string()))
            );
        }
        self.bump_generation();
        ctx.reply(Ok(self.display_bundle()))
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorScalingFactor> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorScalingFactorReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorScalingFactor,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        if !msg.factor.is_finite() || msg.factor <= 0.0 {
            return ctx.reply(Err(BridgeError::invalid_input(
                "scaling factor must be greater than 0",
            )));
        }
        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let handle = self.mirror_display_handle(&selector, &displays);
        let mut app_config = self.normalized_config(&displays);
        reply_try!(Self::require_mirror_monitor(&app_config, &selector));
        Self::monitor_settings_mut(&mut app_config, selector).scaling_factor = msg.factor;
        reply_try!(self.commit_app_config(app_config));
        if let Some(handle) = handle {
            reply_try!(
                self.engine
                    .set_scaling_factor(handle, msg.factor)
                    .await
                    .map_err(|error| BridgeError::engine(error.to_string()))
            );
        }
        self.bump_generation();
        ctx.reply(Ok(self.display_bundle()))
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorTargetFps> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorTargetFpsReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorTargetFps,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let source_display_id = reply_try!(self.source_display_id(&selector, &displays));
        let max_fps = reply_try!(
            displays
                .iter()
                .find(|display| display.desc.display_id == source_display_id)
                .map(|display| display.desc.refresh_rate_hz.max(1))
                .ok_or_else(|| {
                    BridgeError::invalid_input(format!("unknown display id {source_display_id}"))
                })
        );
        let handle = self.mirror_display_handle(&selector, &displays);
        let mut app_config = self.normalized_config(&displays);
        reply_try!(Self::require_mirror_monitor(&app_config, &selector));
        let target_fps = msg.fps.max(1).min(max_fps);
        Self::monitor_settings_mut(&mut app_config, selector).target_fps = target_fps;
        reply_try!(self.commit_app_config(app_config));
        if let Some(handle) = handle {
            reply_try!(
                self.engine
                    .set_fps(handle, target_fps)
                    .await
                    .map_err(|error| BridgeError::engine(error.to_string()))
            );
        }
        self.bump_generation();
        ctx.reply(Ok(self.display_bundle()))
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorVolume> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorVolumeReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorVolume,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        if !(0.0..=1.0).contains(&msg.volume) {
            return ctx.reply(Err(BridgeError::invalid_input(
                "mirror volume must be between 0 and 1",
            )));
        }
        let volume = reply_try!(
            AudioVolume::try_from(msg.volume)
                .map_err(|error| BridgeError::invalid_input(error.to_string()))
        );
        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let handle = self.mirror_display_handle(&selector, &displays);
        let mut app_config = self.normalized_config(&displays);
        reply_try!(Self::require_mirror_monitor(&app_config, &selector));
        Self::monitor_settings_mut(&mut app_config, selector).volume = msg.volume;
        reply_try!(self.commit_app_config(app_config));
        if let Some(handle) = handle {
            reply_try!(
                self.engine
                    .set_audio_volume(handle, volume)
                    .await
                    .map_err(|error| BridgeError::engine(error.to_string()))
            );
        }
        self.bump_generation();
        ctx.reply(Ok(self.display_bundle()))
    }
}

impl<E: EngineFacade + Clone> Message<SetMirrorMuted> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::SetMirrorMutedReply>;

    async fn handle(
        &mut self,
        msg: SetMirrorMuted,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let handle = self.mirror_display_handle(&selector, &displays);
        let mut app_config = self.normalized_config(&displays);
        reply_try!(Self::require_mirror_monitor(&app_config, &selector));
        Self::monitor_settings_mut(&mut app_config, selector).muted = msg.muted;
        reply_try!(self.commit_app_config(app_config));
        if let Some(handle) = handle {
            reply_try!(
                self.engine
                    .set_audio_muted(handle, msg.muted)
                    .await
                    .map_err(|error| BridgeError::engine(error.to_string()))
            );
        }
        self.bump_generation();
        ctx.reply(Ok(self.display_bundle()))
    }
}

impl<E: EngineFacade + Clone> Message<SetLaunchAtLogin> for BridgeActor<E> {
    type Reply = messages::DisplayMutationReply;

    async fn handle(
        &mut self,
        msg: SetLaunchAtLogin,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.launch_at_login.set_enabled(msg.enabled)?;
        Ok(self.display_bundle())
    }
}

impl<E: EngineFacade + Clone> Message<SetPauseOnBatteryPower> for BridgeActor<E> {
    type Reply = messages::SetPauseOnBatteryPowerReply;

    async fn handle(
        &mut self,
        msg: SetPauseOnBatteryPower,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.app_config.power.pause_on_battery_power = msg.enabled;
        if let Some(store) = &self.config_store {
            store.save_app_config(&self.state.app_config)?;
        }
        if !msg.enabled {
            self.state.auto_paused_for_battery = false;
            self.state.battery_pause_suppressed = false;
            self.state.pending_battery_pause_after_initial_frame = false;
        }
        self.apply_power_policy().await?;
        self.bump_generation();
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<SetWorkshopDir> for BridgeActor<E> {
    type Reply = messages::SetWorkshopDirReply;

    async fn handle(
        &mut self,
        msg: SetWorkshopDir,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.paths.workshop_dir = Some(PathBuf::from(&msg.dir));
        self.state.app_config.general.workshop_dir = Some(msg.dir);
        self.persist_app_config()?;
        self.refresh_library()?;
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<SetPowerSource> for BridgeActor<E> {
    type Reply = messages::SetPowerSourceReply;

    async fn handle(
        &mut self,
        msg: SetPowerSource,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if msg.initial_sample && self.state.startup_power_sample_received {
            return Ok(self.all_snapshots());
        }
        if self.state.power_source != msg.source {
            self.state.power_source = msg.source;
            if msg.source == crate::power::PowerSource::Battery {
                self.state.battery_pause_suppressed = false;
            }
        }
        if msg.initial_sample {
            self.state.apply_startup_power_source(msg.source);
        }
        self.apply_power_policy().await?;
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<InitialFrameReady> for BridgeActor<E> {
    type Reply = messages::InitialFrameReadyReply;

    async fn handle(
        &mut self,
        _msg: InitialFrameReady,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.initial_frame_ready = true;
        if self.state.pending_battery_pause_after_initial_frame {
            self.state.pending_battery_pause_after_initial_frame = false;
            self.apply_power_policy().await?;
        }
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<SetAssetsDir> for BridgeActor<E> {
    type Reply = messages::SetAssetsDirReply;

    async fn handle(
        &mut self,
        msg: SetAssetsDir,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.paths.assets_dir = Some(PathBuf::from(&msg.dir));
        self.state.app_config.general.assets_dir = Some(msg.dir);
        self.persist_app_config()?;
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<EjectWallpaperFromDisplay> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::EjectWallpaperFromDisplayReply>;

    async fn handle(
        &mut self,
        msg: EjectWallpaperFromDisplay,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let selector = reply_try!(self.selector_for(&msg.display_id, &displays));
        let mut app_config = self.normalized_config(&displays);
        let Some(monitor) = app_config
            .monitors
            .iter_mut()
            .find(|monitor| monitor.selector == selector)
        else {
            return ctx.reply(Err(BridgeError::invalid_input(format!(
                "unknown display id {}",
                msg.display_id
            ))));
        };

        if monitor.wallpaper.as_deref() != Some(msg.wallpaper_id.as_str()) {
            return ctx.reply(Err(BridgeError::invalid_input(format!(
                "wallpaper {} is not active on display {}",
                msg.wallpaper_id, msg.display_id
            ))));
        }

        monitor.wallpaper = None;
        if monitor.selector == SerializedSelector::Primary {
            monitor.enabled = true;
            monitor.mode = INDEPENDENT_DISPLAY_MODE.to_string();
            monitor.mirror_target = None;
            if let Some(primary) = displays.first() {
                for alias in app_config.monitors.iter_mut().filter(|candidate| {
                    candidate.selector != SerializedSelector::Primary
                        && candidate
                            .selector
                            .to_selector()
                            .matches_primary(primary, &displays)
                }) {
                    if alias.wallpaper.as_deref() == Some(msg.wallpaper_id.as_str()) {
                        alias.wallpaper = None;
                    }
                }
            }
        }

        let display_settings = self.display_rows(&app_config, &displays);
        self.delegate_display(app_config, display_settings, ctx)
    }
}

impl<E: EngineFacade + Clone> Message<SetGlobalPlayback> for BridgeActor<E> {
    type Reply = messages::SetGlobalPlaybackReply;

    async fn handle(
        &mut self,
        msg: SetGlobalPlayback,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_playback(msg.playback_state, msg.paused, PlaybackChangeOrigin::Manual)
            .await?;
        Ok(self.all_snapshots())
    }
}

impl<E: EngineFacade + Clone> Message<Shutdown> for BridgeActor<E> {
    type Reply = messages::ShutdownReply;

    async fn handle(
        &mut self,
        _msg: Shutdown,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let mut active_handles = Vec::new();
        for display in self.engine.display_snapshot() {
            let Some(handle) = display.handle else {
                continue;
            };
            if active_handles.contains(&handle) {
                continue;
            }
            active_handles.push(handle);
        }

        for handle in active_handles {
            self.engine
                .set_audio_capture_enabled(handle, false)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.engine
            .close_all_scenes()
            .await
            .map_err(|error| BridgeError::engine(error.to_string()))
    }
}

impl<E: EngineFacade + Clone> Message<SetVolume> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetVolume,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let volume = AudioVolume::try_from(msg.volume)
            .map_err(|error| BridgeError::invalid_input(error.to_string()))?;
        let wallpaper_config = self
            .state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_volume_immediate(msg.volume)?;
        self.save_wallpaper(msg.wallpaper_id.clone(), wallpaper_config)?;
        for handle in self.wallpaper_handles(&msg.wallpaper_id, false) {
            self.engine
                .set_audio_volume(handle, volume)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetMuted> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetMuted,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let wallpaper_config = self
            .state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_muted_immediate(msg.muted);
        self.save_wallpaper(msg.wallpaper_id.clone(), wallpaper_config)?;
        for handle in self.wallpaper_handles(&msg.wallpaper_id, false) {
            self.engine
                .set_audio_muted(handle, msg.muted)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetAudioResponseEnabled> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetAudioResponseEnabled,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let wallpaper_config = self
            .state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_audio_response_enabled_immediate(msg.enabled);
        self.save_wallpaper(msg.wallpaper_id.clone(), wallpaper_config)?;
        for handle in self.wallpaper_handles(&msg.wallpaper_id, true) {
            let engine = self.engine.clone();
            let enabled = msg.enabled;
            tokio::spawn(async move {
                let _ = engine.set_audio_capture_enabled(handle, enabled).await;
            });
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetDisplayConfigEnabled> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetDisplayConfigEnabled,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        let selector = self.selector_for(&msg.display_id, &displays)?;
        let enabled_displays = self.state.enabled_selectors(&msg.wallpaper_id);
        let mut selector_aliases = vec![selector.clone()];
        if let Some(display) = selector.to_selector().resolve_display(&displays) {
            if displays
                .first()
                .is_some_and(|primary| display.matches_primary(primary))
            {
                selector_aliases.push(SerializedSelector::Primary);
            }
            if let Some(identity_selector) = display.stable_identity_selector() {
                selector_aliases.push(identity_selector);
            }
            selector_aliases.dedup();
        }
        self.state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_display_aliases_enabled(&selector_aliases, msg.enabled, &enabled_displays);
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetScalingMode> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetScalingMode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        let selector = self.selector_for(&msg.display_id, &displays)?;
        let scaling_mode = ScalingMode::from(msg.mode);
        let wallpaper_config = self
            .state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_scaling_mode_immediate(selector.clone(), scaling_mode);
        self.save_wallpaper(msg.wallpaper_id.clone(), wallpaper_config)?;
        if let Some(handle) = self.display_handle(&msg.wallpaper_id, &selector) {
            self.engine
                .set_scaling_mode(handle, scaling_mode)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetScalingFactor> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetScalingFactor,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        let selector = self.selector_for(&msg.display_id, &displays)?;
        self.state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_scaling_factor(selector.clone(), msg.factor)?;
        if let Some(handle) = self.display_handle(&msg.wallpaper_id, &selector) {
            self.engine
                .set_scaling_factor(handle, msg.factor)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<SetTargetFps> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: SetTargetFps,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let displays = self.engine.display_snapshot();
        let selector = self.selector_for(&msg.display_id, &displays)?;
        let source_display_id = self.source_display_id(&selector, &displays)?;
        let max_fps = displays
            .iter()
            .find(|display| display.desc.display_id == source_display_id)
            .map(|display| display.desc.refresh_rate_hz.max(1))
            .ok_or_else(|| {
                BridgeError::invalid_input(format!("unknown display id {source_display_id}"))
            })?;
        let target_fps = msg.fps.min(max_fps.max(1));
        let wallpaper_config = self
            .state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .set_target_fps_immediate(selector.clone(), msg.fps, max_fps);
        self.save_wallpaper(msg.wallpaper_id.clone(), wallpaper_config)?;
        if let Some(handle) = self.display_handle(&msg.wallpaper_id, &selector) {
            self.engine
                .set_fps(handle, target_fps)
                .await
                .map_err(|error| BridgeError::engine(error.to_string()))?;
        }
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<EditProperty> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: EditProperty,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let model = self.state.project_model(&msg.wallpaper_id)?.clone();
        let property = model
            .properties
            .iter()
            .find(|property| property.id == msg.property_id)
            .ok_or_else(|| {
                BridgeError::invalid_input(format!("unknown property id {}", msg.property_id))
            })?;
        match (&property.kind, &property.metadata, &msg.value) {
            (PropertyKind::Bool, _, BridgePropertyValue::Bool { .. })
            | (PropertyKind::TextInput, _, BridgePropertyValue::String { .. })
            | (PropertyKind::Directory, _, BridgePropertyValue::String { .. }) => {}
            (
                PropertyKind::Slider,
                PropertyMetadata::Slider { min, max, .. },
                BridgePropertyValue::Number { value },
            ) if value.is_finite() && (*min..=*max).contains(value) => {}
            (PropertyKind::Color, _, BridgePropertyValue::ColorRgb { red, green, blue })
                if is_color_channel_valid!(*red)
                    && is_color_channel_valid!(*green)
                    && is_color_channel_valid!(*blue) => {}
            (
                PropertyKind::Combo,
                PropertyMetadata::Combo { options },
                BridgePropertyValue::String { value },
            ) if options.iter().any(|option| option.value == *value) => {}
            _ => {
                return Err(BridgeError::invalid_input(format!(
                    "invalid value for property id {}",
                    property.id
                )));
            }
        }
        self.state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .edit_property(&model, &msg.property_id, PropertyValue::from(msg.value));
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<RestorePropertyDefault> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: RestorePropertyDefault,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let property_exists = self
            .state
            .project_model(&msg.wallpaper_id)?
            .properties
            .iter()
            .any(|property| property.id == msg.property_id);
        if !property_exists {
            return Err(BridgeError::invalid_input(format!(
                "unknown property id {}",
                msg.property_id
            )));
        }
        self.state
            .wallpaper_draft_mut(&msg.wallpaper_id)?
            .restore_property_default(&msg.property_id);
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<ApplyWallpaperOptions> for BridgeActor<E> {
    type Reply = DelegatedReply<messages::WallpaperMutationReply>;

    async fn handle(
        &mut self,
        msg: ApplyWallpaperOptions,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        macro_rules! reply_try {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(error) => return ctx.reply(Err(error)),
                }
            };
        }

        let displays = self.engine.display_snapshot();
        let primary_identity_selector = displays
            .first()
            .and_then(DisplaySnapshotExt::stable_identity_selector);
        let candidates = reply_try!(
            self.state
                .apply_candidates(&msg.wallpaper_id, primary_identity_selector.as_ref())
        );
        let app_config = candidates.app_config.clone();
        let wallpaper_configs = candidates.wallpaper_configs.clone();
        let requires_reconcile = candidates.requires_reconcile;
        let paused = self.playback_paused();
        let predicted_scenes = reply_try!(
            requires_reconcile
                .then(|| {
                    ActivationInputs {
                        app_config: &app_config,
                        wallpapers: &wallpaper_configs,
                        displays: &displays,
                        paused,
                        paths: &self.paths,
                        force_shader_refresh: false,
                    }
                    .build()
                })
                .transpose()
        );

        if requires_reconcile {
            let generation = self.reserve_reconcile();
            let actor = ctx.actor_ref().clone();
            let engine = self.engine.clone();
            let wallpaper_id = msg.wallpaper_id;
            let paths = self.paths.clone();
            return ctx.spawn(async move {
                let scenes = match reconcile_with(
                    engine,
                    app_config,
                    wallpaper_configs,
                    paused,
                    paths,
                    false,
                )
                .await
                {
                    Ok(scenes) => scenes,
                    Err(error) => {
                        let _ = actor
                            .ask(ReconcileFailed {
                                error: duplicate_error(&error),
                                generation,
                            })
                            .await;
                        return Err(error);
                    }
                };
                actor
                    .ask(CommitApplyAfterReconcile {
                        wallpaper_id,
                        candidates,
                        scenes,
                        generation,
                    })
                    .await
                    .map_err(map_send_error)
            });
        }

        reply_try!(self.save_configs(&candidates.app_config, &candidates.wallpaper_config,));
        reply_try!(
            self.state
                .commit_apply_candidates(msg.wallpaper_id.clone(), candidates, false)
        );
        self.state.refresh_active_ids();
        if let Some(predicted_scenes) = predicted_scenes.as_deref() {
            self.state.set_active_ids_from_scenes(predicted_scenes);
        }

        self.bump_generation();
        ctx.reply(self.wallpaper_bundle(msg.wallpaper_id))
    }
}

impl<E: EngineFacade + Clone> Message<CommitApplyAfterReconcile> for BridgeActor<E> {
    type Reply = messages::CommitApplyAfterReconcileReply;

    async fn handle(
        &mut self,
        msg: CommitApplyAfterReconcile,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if !self.reconcile_current(msg.generation) {
            self.stale_reconcile(msg.generation, ctx.actor_ref().clone());
            return self.wallpaper_bundle(msg.wallpaper_id);
        }

        self.save_configs(&msg.candidates.app_config, &msg.candidates.wallpaper_config)?;
        self.state
            .commit_apply_candidates(msg.wallpaper_id.clone(), msg.candidates, false)?;
        self.state.set_active_ids_from_scenes(&msg.scenes);
        self.finish_reconcile(msg.generation, ctx.actor_ref().clone());
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

impl<E: EngineFacade + Clone> Message<CommitDisplayAfterReconcile> for BridgeActor<E> {
    type Reply = messages::CommitDisplayAfterReconcileReply;

    async fn handle(
        &mut self,
        msg: CommitDisplayAfterReconcile,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if !self.reconcile_current(msg.generation)
            || self.state.wallpaper_configs != msg.wallpaper_configs
        {
            self.stale_reconcile(msg.generation, ctx.actor_ref().clone());
            return Ok(self.display_bundle());
        }

        let generation = msg.generation;
        self.commit_display_settings(msg.app_config, msg.display_settings, msg.scenes)?;
        self.finish_reconcile(generation, ctx.actor_ref().clone());
        Ok(self.display_bundle())
    }
}

impl<E: EngineFacade + Clone> Message<CompleteRestoreAfterReconcile> for BridgeActor<E> {
    type Reply = messages::CompleteRestoreAfterReconcileReply;

    async fn handle(
        &mut self,
        msg: CompleteRestoreAfterReconcile,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if !self.reconcile_current(msg.generation) {
            self.stale_reconcile(msg.generation, ctx.actor_ref().clone());
            return Ok(());
        }

        match msg.result {
            Ok(scenes) => {
                self.state.set_active_ids_from_scenes(&scenes);
                self.finish_reconcile(msg.generation, ctx.actor_ref().clone());
                Ok(())
            }
            Err(error) => {
                self.state.errors.push(error.message().to_string());
                Err(error)
            }
        }
    }
}

impl<E: EngineFacade + Clone> Message<ReconcileFailed> for BridgeActor<E> {
    type Reply = messages::ReconcileFailedReply;

    async fn handle(
        &mut self,
        msg: ReconcileFailed,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.reconcile_failure(msg.generation, msg.error, ctx.actor_ref().clone());
        Ok(())
    }
}

impl<E: EngineFacade + Clone> Message<CancelWallpaperOptions> for BridgeActor<E> {
    type Reply = messages::WallpaperMutationReply;

    async fn handle(
        &mut self,
        msg: CancelWallpaperOptions,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.state.wallpaper_draft_mut(&msg.wallpaper_id)?.cancel();
        self.bump_generation();
        self.wallpaper_bundle(msg.wallpaper_id)
    }
}

use std::sync::Arc;

use kameo::{
    actor::{ActorRef, Spawn},
    message::{Context, Message},
};

#[cfg(test)]
use crate::engine::messages::{DisplayRecordCountForTest, SequenceForTest};
use crate::{
    DisplaySelector, EngineError, WallpaperAssignment,
    display::state::{DisplayKey, DisplayStateModel},
    engine::{
        EngineSnapshotPublisher,
        messages::{self, Ping},
        runtime::{RuntimeRefreshJob, RuntimeRefreshMode, SceneRuntime, SceneRuntimeState},
        state::{DisplayRuntimeRecord, EngineState},
    },
    owe::backend::OweBackend,
    project::{SceneDescSliceExt, SceneHandle},
};

#[derive(kameo::Actor)]
pub struct EngineActor {
    #[allow(dead_code)]
    backend: OweBackend,
    pub state: EngineState,
    snapshots: Arc<EngineSnapshotPublisher>,
    #[allow(dead_code)]
    display_callback: Option<()>,
    #[allow(dead_code)]
    refresh_running: bool,
    #[allow(dead_code)]
    refresh_pending: bool,
    #[cfg(test)]
    pub test_sequence: u64,
}

#[derive(Clone)]
pub struct EngineActorHandle {
    #[allow(dead_code)]
    actor: ActorRef<EngineActor>,
    #[allow(dead_code)]
    _runtime: Option<Arc<tokio::runtime::Runtime>>,
}

impl EngineActorHandle {
    #[allow(clippy::single_call_fn)]
    pub fn spawn(
        backend: OweBackend,
        state: EngineState,
        snapshots: Arc<EngineSnapshotPublisher>,
    ) -> Result<Self, EngineError> {
        let actor = EngineActor::new(backend, state, snapshots);

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if matches!(
                handle.runtime_flavor(),
                tokio::runtime::RuntimeFlavor::CurrentThread
            ) {
                return Ok(Self {
                    actor: EngineActor::spawn(actor),
                    _runtime: None,
                });
            }

            let _guard = handle.enter();
            return Ok(Self {
                actor: EngineActor::spawn_in_thread(actor),
                _runtime: None,
            });
        }

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("wallpaper-engine-actor-runtime")
                .build()
                .map_err(|error| {
                    EngineError::Platform(format!("failed to start engine actor runtime: {error}"))
                })?,
        );
        let actor = {
            let _guard = runtime.enter();
            EngineActor::spawn_in_thread(actor)
        };

        Ok(Self {
            actor,
            _runtime: Some(runtime),
        })
    }

    pub fn actor(&self) -> &ActorRef<EngineActor> {
        &self.actor
    }
}

#[allow(clippy::single_call_fn)]
impl EngineActor {
    pub fn new(
        backend: OweBackend,
        state: EngineState,
        snapshots: Arc<EngineSnapshotPublisher>,
    ) -> Self {
        Self {
            backend,
            state,
            snapshots,
            display_callback: None,
            refresh_running: false,
            refresh_pending: false,
            #[cfg(test)]
            test_sequence: 0,
        }
    }

    pub fn publish_snapshot(&self) {
        self.snapshots.publish(self.state.snapshot());
    }

    pub fn refresh_displays_now(&mut self) -> Result<(), EngineError> {
        let primary = crate::DisplayDesc::primary()?;
        let displays = crate::DisplayDesc::all()?;
        self.reconcile_display_descriptors(primary, displays)
    }

    pub fn reconcile_display_descriptors(
        &mut self,
        primary: crate::DisplayDesc,
        displays: Vec<crate::DisplayDesc>,
    ) -> Result<(), EngineError> {
        self.update_display_state(|model| {
            model.refresh_connected(primary, displays)?;
            Ok(())
        })
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn set_wallpaper_for(
        &mut self,
        selector: DisplaySelector,
        wallpaper: WallpaperAssignment,
    ) -> Result<Option<SceneHandle>, EngineError> {
        let key = DisplayKey::from_selector(&selector)?;
        self.update_display_state(|model| model.set_assignment(&selector, wallpaper))?;
        self.handle_for_key(&key)
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn create_window_for(
        &mut self,
        selector: DisplaySelector,
    ) -> Result<Option<SceneHandle>, EngineError> {
        let key = DisplayKey::from_selector(&selector)?;
        self.update_display_state(|model| model.set_window_active(&selector, true).map(|_| ()))?;
        self.handle_for_key(&key)
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn destroy_window_for(&mut self, selector: DisplaySelector) -> Result<(), EngineError> {
        self.update_display_state(|model| model.set_window_active(&selector, false).map(|_| ()))
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn reconcile_scenes(
        &mut self,
        scenes: Vec<crate::project::SceneDesc>,
    ) -> Result<Vec<crate::project::SceneResult>, EngineError> {
        self.update_display_state(|model| model.apply_reconcile(&scenes))?;

        let model = DisplayStateModel {
            records: self
                .state
                .display_records
                .iter()
                .map(|record| record.model.clone())
                .collect(),
        };
        scenes.reconcile_results(&model, &self.state.handles_by_display)
    }

    pub fn close_all_scenes(&mut self) -> Result<(), EngineError> {
        self.state.close_all()
    }

    pub fn set_scaling_mode(
        &mut self,
        handle: SceneHandle,
        mode: crate::project::ScalingMode,
    ) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_scaling_mode(mode))
    }

    pub fn set_scaling_factor(
        &mut self,
        handle: SceneHandle,
        factor: f64,
    ) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_scaling_factor(factor))
    }

    pub fn set_fps(&mut self, handle: SceneHandle, fps: u32) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_fps(fps))
    }

    pub fn set_paused(&mut self, handle: SceneHandle, paused: bool) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_paused(paused))
    }

    pub fn set_all_paused(&mut self, paused: bool) -> Result<(), EngineError> {
        let handles: Vec<SceneHandle> = self.state.active_runtime_handles().collect();

        for handle in handles {
            self.set_paused(handle, paused)?;
        }

        Ok(())
    }

    pub fn set_render_resolution(
        &mut self,
        handle: SceneHandle,
        width: u32,
        height: u32,
    ) -> Result<(), EngineError> {
        let backend = self.backend;
        self.with_scene_mut(handle, |scene| {
            scene.set_render_resolution(backend, width, height)
        })
    }

    pub fn set_audio_response_enabled(
        &mut self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_audio_response_enabled(enabled))
    }

    pub fn set_audio_volume(
        &mut self,
        handle: SceneHandle,
        volume: crate::media::audio::AudioVolume,
    ) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_audio_volume(volume))
    }

    pub fn set_audio_muted(&mut self, handle: SceneHandle, muted: bool) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_audio_muted(muted))
    }

    pub fn set_property_override(
        &mut self,
        handle: SceneHandle,
        flat_json: String,
    ) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| {
            scene.set_property_override_json(Some(flat_json))
        })
    }

    pub fn reset_property_override(&mut self, handle: SceneHandle) -> Result<(), EngineError> {
        self.with_scene_mut(handle, |scene| scene.set_property_override_json(None))
    }

    pub fn update_display_state(
        &mut self,
        update: impl FnOnce(&mut DisplayStateModel) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let mut runtimes_to_close = Vec::new();
        let mut model = DisplayStateModel {
            records: self
                .state
                .display_records
                .iter()
                .map(|record| record.model.clone())
                .collect(),
        };
        update(&mut model)?;

        self.state.apply_display_model(&model)?;

        for index in 0..self.state.display_records.len() {
            let mut resolved_model = self.state.display_records[index].model.clone();
            resolved_model.assignment = model.resolved_assignment(&resolved_model.key)?;
            if !resolved_model.should_have_runtime() {
                let record = &mut self.state.display_records[index];
                record.last_runtime_state = record
                    .runtime
                    .as_ref()
                    .map(SceneRuntime::runtime_state)
                    .or_else(|| record.last_runtime_state.clone());
                record.model.runtime_open = false;
                let key = record.model.key.clone();
                let runtime = record.runtime.take();
                let handle = record.handle;
                self.state.sync_handle_reservation(key, handle);
                if let Some(runtime) = runtime {
                    runtimes_to_close.push(runtime);
                }
            }
        }
        for mut runtime in runtimes_to_close {
            runtime.close()?;
        }

        self.open_or_rebuild_required_runtimes()
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn handle_for_key(&self, key: &DisplayKey) -> Result<Option<SceneHandle>, EngineError> {
        Ok(self.state.handles_by_display.get(key).copied())
    }

    pub fn with_scene_mut(
        &mut self,
        handle: SceneHandle,
        operation: impl FnOnce(&mut SceneRuntime) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let runtime = self.state.scene_mut(handle)?;
        operation(runtime)?;
        self.state.record_runtime_state(handle)
    }

    pub fn open_or_rebuild_required_runtimes(&mut self) -> Result<(), EngineError> {
        let mut jobs = Vec::new();
        let model = DisplayStateModel {
            records: self
                .state
                .display_records
                .iter()
                .map(|record| record.model.clone())
                .collect(),
        };

        for index in 0..self.state.display_records.len() {
            let mut runtime_model = self.state.display_records[index].model.clone();
            runtime_model.assignment = model.resolved_assignment(&runtime_model.key)?;
            let runtime_record = DisplayRuntimeRecord {
                model: runtime_model,
                handle: None,
                runtime: None,
                last_runtime_state: None,
            };
            if !runtime_record.should_have_runtime() {
                continue;
            }
            let Some(desc) = runtime_record.scene_desc()? else {
                continue;
            };
            let current_desc = self.state.display_records[index]
                .runtime
                .as_ref()
                .map(|runtime| &runtime.desc);
            if !RuntimeRefreshMode::from_transition(current_desc, &desc).is_required()
                && self.state.display_records[index].handle.is_some()
            {
                self.state.display_records[index].model.runtime_open = true;
                continue;
            }
            let key = self.state.display_records[index].model.key.clone();
            let handle = self.state.reserve_handle_for_key(key.clone());
            let runtime_state = self.state.display_records[index]
                .runtime
                .as_ref()
                .map(SceneRuntime::runtime_state)
                .or_else(|| self.state.display_records[index].last_runtime_state.clone())
                .unwrap_or(SceneRuntimeState::try_from(&desc)?);
            let existing_runtime = self.state.display_records[index].runtime.take();
            self.state.display_records[index].handle = Some(handle);
            self.state.display_records[index].model.runtime_open = false;
            jobs.push(RuntimeRefreshJob {
                key,
                handle,
                desc,
                runtime_state,
                existing_runtime,
            });
        }

        let mut first_error = None;
        for job in jobs {
            if let Err(error) = self.run_runtime_refresh_job(job) {
                first_error.get_or_insert(error);
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn run_runtime_refresh_job(&mut self, job: RuntimeRefreshJob) -> Result<(), EngineError> {
        let RuntimeRefreshJob {
            key,
            handle,
            desc,
            runtime_state,
            existing_runtime,
        } = job;

        let mut runtime = match existing_runtime {
            Some(mut runtime) => {
                match RuntimeRefreshMode::from_transition(Some(&runtime.desc), &desc) {
                    RuntimeRefreshMode::Unchanged => runtime,
                    RuntimeRefreshMode::OpenRuntime => {
                        unreachable!("existing runtime cannot require initial open")
                    }
                    RuntimeRefreshMode::UpdateWindowOnly => {
                        let before_display = runtime.desc.display.clone();
                        match runtime.update_window_display(desc.display.clone()) {
                            Ok(()) => {
                                log::debug!(
                                    "[wallpaper-core display] updated runtime window in place: \
                                     key={key:?} before_display={before_display:?} \
                                     after_display={:?}",
                                    desc.display
                                );
                                runtime
                            }
                            Err(error) => {
                                self.restore_failed_runtime_refresh(&key, handle, Some(runtime))?;
                                return Err(error);
                            }
                        }
                    }
                    RuntimeRefreshMode::ReopenRuntime => {
                        let before_display = runtime.desc.display.clone();
                        let reconfigure_result =
                            runtime.reconfigure_for_display(self.backend, desc.display.clone());
                        match reconfigure_result {
                            Ok(()) => {
                                log::debug!(
                                    "[wallpaper-core display] reconfigured runtime surface in \
                                     place: key={key:?} before_display={before_display:?} \
                                     after_display={:?}",
                                    desc.display
                                );
                                runtime
                            }
                            Err(reconfigure_error) => {
                                log::warn!(
                                    "[wallpaper-core display] fast surface reconfigure failed, \
                                     falling back to full rebuild: key={key:?} \
                                     before_display={before_display:?} after_display={:?} \
                                     error={reconfigure_error}",
                                    desc.display
                                );
                                match SceneRuntime::open(self.backend, &desc, runtime_state) {
                                    Ok(replacement) => {
                                        if let Err(error) = runtime.close() {
                                            log::warn!(
                                                "[wallpaper-core display] failed to close \
                                                 replaced display runtime: {error}"
                                            );
                                        }
                                        replacement
                                    }
                                    Err(error) => {
                                        self.restore_failed_runtime_refresh(
                                            &key,
                                            handle,
                                            Some(runtime),
                                        )?;
                                        return Err(error);
                                    }
                                }
                            }
                        }
                    }
                    RuntimeRefreshMode::RebuildExistingRuntime => {
                        let rebuild_result = if runtime.desc.same_wallpaper(&desc) {
                            runtime.resize_or_rebuild(self.backend, desc.display.clone())
                        } else {
                            runtime.replace_wallpaper(self.backend, desc)
                        };
                        if let Err(error) = rebuild_result {
                            self.restore_failed_runtime_refresh(&key, handle, Some(runtime))?;
                            return Err(error);
                        }
                        runtime
                    }
                }
            }
            None => match SceneRuntime::open(self.backend, &desc, runtime_state) {
                Ok(runtime) => runtime,
                Err(error) => {
                    self.restore_failed_runtime_refresh(&key, handle, None)?;
                    return Err(error);
                }
            },
        };

        if let Some(index) = self.state.record_index(&key) {
            self.state.display_records[index].model.runtime_open = true;
            self.state.display_records[index].handle = Some(handle);
            self.state.display_records[index].last_runtime_state = Some(runtime.runtime_state());
            self.state.display_records[index].runtime = Some(runtime);
            self.state.handles_by_display.insert(key.clone(), handle);
            self.state.displays_by_handle.insert(handle, key);
        } else {
            runtime.close()?;
        }

        Ok(())
    }

    pub fn restore_failed_runtime_refresh(
        &mut self,
        key: &DisplayKey,
        handle: SceneHandle,
        runtime: Option<SceneRuntime>,
    ) -> Result<(), EngineError> {
        let Some(mut runtime) = runtime else {
            if let Some(index) = self.state.record_index(key) {
                self.state.display_records[index].model.runtime_open = false;
            }
            return Ok(());
        };

        if let Some(index) = self.state.record_index(key) {
            self.state.display_records[index].model.runtime_open = true;
            self.state.display_records[index].handle = Some(handle);
            self.state.display_records[index].last_runtime_state = Some(runtime.runtime_state());
            self.state.display_records[index].runtime = Some(runtime);
            self.state.handles_by_display.insert(key.clone(), handle);
            self.state.displays_by_handle.insert(handle, key.clone());
        } else {
            runtime.close()?;
        }

        Ok(())
    }
}

impl Message<Ping> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(&mut self, _msg: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        Ok(())
    }
}

impl Message<messages::RefreshDisplays> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        _msg: messages::RefreshDisplays,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.refresh_displays_now()?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::RefreshDisplayDescriptors> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::RefreshDisplayDescriptors,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.reconcile_display_descriptors(msg.primary, msg.displays)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::ReconcileScenes> for EngineActor {
    type Reply = messages::ReconcileReply;

    async fn handle(
        &mut self,
        msg: messages::ReconcileScenes,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let result = self.reconcile_scenes(msg.scenes)?;
        self.publish_snapshot();
        Ok(result)
    }
}

impl Message<messages::CreateWindowForDisplay> for EngineActor {
    type Reply = messages::SceneHandleReply;

    async fn handle(
        &mut self,
        msg: messages::CreateWindowForDisplay,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let result = self.create_window_for(msg.selector)?;
        self.publish_snapshot();
        Ok(result)
    }
}

impl Message<messages::DestroyWindowForDisplay> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::DestroyWindowForDisplay,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.destroy_window_for(msg.selector)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetWallpaperForDisplay> for EngineActor {
    type Reply = messages::SceneHandleReply;

    async fn handle(
        &mut self,
        msg: messages::SetWallpaperForDisplay,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let result = self.set_wallpaper_for(msg.selector, msg.wallpaper)?;
        self.publish_snapshot();
        Ok(result)
    }
}

impl Message<messages::SetScalingMode> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetScalingMode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_scaling_mode(msg.handle, msg.mode)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetScalingFactor> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetScalingFactor,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_scaling_factor(msg.handle, msg.factor)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetFps> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetFps,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_fps(msg.handle, msg.fps)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetPaused> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetPaused,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_paused(msg.handle, msg.paused)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetAllPaused> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetAllPaused,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_all_paused(msg.paused)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetRenderResolution> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetRenderResolution,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_render_resolution(msg.handle, msg.width, msg.height)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetAudioResponseEnabled> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetAudioResponseEnabled,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_audio_response_enabled(msg.handle, msg.enabled)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetAudioVolume> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetAudioVolume,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_audio_volume(msg.handle, msg.volume)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetAudioMuted> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetAudioMuted,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_audio_muted(msg.handle, msg.muted)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::SetPropertyOverride> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::SetPropertyOverride,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.set_property_override(msg.handle, msg.flat_json)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::ResetPropertyOverride> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        msg: messages::ResetPropertyOverride,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.reset_property_override(msg.handle)?;
        self.publish_snapshot();
        Ok(())
    }
}

impl Message<messages::CloseAllScenes> for EngineActor {
    type Reply = Result<(), EngineError>;

    async fn handle(
        &mut self,
        _msg: messages::CloseAllScenes,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.close_all_scenes()?;
        self.publish_snapshot();
        Ok(())
    }
}

#[cfg(test)]
impl Message<DisplayRecordCountForTest> for EngineActor {
    type Reply = Result<usize, EngineError>;

    async fn handle(
        &mut self,
        _msg: DisplayRecordCountForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        Ok(self.state.display_records.len())
    }
}

#[cfg(test)]
impl Message<SequenceForTest> for EngineActor {
    type Reply = Result<u64, EngineError>;

    async fn handle(
        &mut self,
        msg: SequenceForTest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.test_sequence += 1;
        assert_eq!(self.test_sequence, msg.expected);
        Ok(self.test_sequence)
    }
}

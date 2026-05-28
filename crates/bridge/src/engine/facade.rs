#[cfg(test)]
use std::{sync::mpsc::Receiver, time::Duration};
use std::{
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    thread,
};

#[cfg(test)]
use arc_swap::ArcSwap;
#[cfg(test)]
use crossbeam_queue::SegQueue;
use futures_util::future::{BoxFuture, FutureExt};
#[cfg(test)]
use wallpaper_core::project::SceneTemplate;
use wallpaper_core::{
    DisplaySelector, DisplaySnapshotEntry, EngineError, WallpaperAssignment, WallpaperEngine,
    media::audio::{
        AudioCaptureError, AudioFrameConsumer, AudioResponseController, AudioResponseEngine,
        AudioVolume, InterleavedStereoF32, PlatformAudioCaptureBackend,
    },
    project::{ScalingMode, SceneDesc, SceneHandle, SceneResult},
};

pub type EngineFuture<T> = BoxFuture<'static, Result<T, EngineError>>;

pub trait EngineFacade: Send + Sync + 'static {
    fn reconcile_scenes(&self, scenes: Vec<SceneDesc>) -> EngineFuture<Vec<SceneResult>>;
    fn refresh_displays(&self) -> EngineFuture<()>;
    fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry>;
    fn close_all_scenes(&self) -> EngineFuture<()>;
    fn set_all_paused(&self, paused: bool) -> EngineFuture<()>;
    fn set_audio_volume(&self, handle: SceneHandle, volume: AudioVolume) -> EngineFuture<()>;
    fn set_audio_muted(&self, handle: SceneHandle, muted: bool) -> EngineFuture<()>;
    fn set_audio_response_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()>;
    fn set_audio_capture_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()>;
    fn set_scaling_mode(&self, handle: SceneHandle, mode: ScalingMode) -> EngineFuture<()>;
    fn set_scaling_factor(&self, handle: SceneHandle, factor: f64) -> EngineFuture<()>;
    fn set_fps(&self, handle: SceneHandle, fps: u32) -> EngineFuture<()>;
    fn poll_mouse_position(&self) -> EngineFuture<()>;
    fn set_mouse_position(&self, handle: SceneHandle, x: f64, y: f64) -> EngineFuture<()>;
    fn set_mouse_button(&self, handle: SceneHandle, button: u32, pressed: bool)
    -> EngineFuture<()>;
    fn set_mouse_entered(&self, handle: SceneHandle, entered: bool) -> EngineFuture<()>;
    fn create_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> EngineFuture<Option<SceneHandle>>;
    fn set_wallpaper_for_display(
        &self,
        selector: DisplaySelector,
        assignment: WallpaperAssignment,
    ) -> EngineFuture<Option<SceneHandle>>;
}

#[derive(Clone)]
pub struct RealEngineFacade {
    engine: WallpaperEngine,
    audio_capture: AudioCaptureWorker,
}

impl RealEngineFacade {
    #[must_use]
    pub fn new(engine: WallpaperEngine) -> Self {
        Self {
            audio_capture: AudioCaptureWorker::spawn(engine.clone()),
            engine,
        }
    }
}

impl EngineFacade for RealEngineFacade {
    fn reconcile_scenes(&self, scenes: Vec<SceneDesc>) -> EngineFuture<Vec<SceneResult>> {
        let engine = self.engine.clone();
        async move { engine.reconcile_scenes(scenes).await }.boxed()
    }

    fn refresh_displays(&self) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.refresh_displays().await }.boxed()
    }

    fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry> {
        self.engine.display_snapshot()
    }

    fn close_all_scenes(&self) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.close_all_scenes().await }.boxed()
    }

    fn set_all_paused(&self, paused: bool) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_all_paused(paused).await }.boxed()
    }

    fn set_audio_volume(&self, handle: SceneHandle, volume: AudioVolume) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_audio_volume(handle, volume).await }.boxed()
    }

    fn set_audio_muted(&self, handle: SceneHandle, muted: bool) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_audio_muted(handle, muted).await }.boxed()
    }

    fn set_audio_response_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_audio_response_enabled(handle, enabled).await }.boxed()
    }

    fn set_audio_capture_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()> {
        let audio_capture = self.audio_capture.clone();
        async move {
            audio_capture
                .set_enabled(handle, enabled)
                .map_err(EngineError::Platform)
        }
        .boxed()
    }

    fn set_scaling_mode(&self, handle: SceneHandle, mode: ScalingMode) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_scaling_mode(handle, mode).await }.boxed()
    }

    fn set_scaling_factor(&self, handle: SceneHandle, factor: f64) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_scaling_factor(handle, factor).await }.boxed()
    }

    fn set_fps(&self, handle: SceneHandle, fps: u32) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_fps(handle, fps).await }.boxed()
    }

    fn poll_mouse_position(&self) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.poll_mouse_position().await }.boxed()
    }

    fn set_mouse_position(&self, handle: SceneHandle, x: f64, y: f64) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_mouse_position(handle, x, y).await }.boxed()
    }

    fn set_mouse_button(
        &self,
        handle: SceneHandle,
        button: u32,
        pressed: bool,
    ) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_mouse_button(handle, button, pressed).await }.boxed()
    }

    fn set_mouse_entered(&self, handle: SceneHandle, entered: bool) -> EngineFuture<()> {
        let engine = self.engine.clone();
        async move { engine.set_mouse_entered(handle, entered).await }.boxed()
    }

    fn create_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> EngineFuture<Option<SceneHandle>> {
        let engine = self.engine.clone();
        async move { engine.create_window_for_display(selector).await }.boxed()
    }

    fn set_wallpaper_for_display(
        &self,
        selector: DisplaySelector,
        assignment: WallpaperAssignment,
    ) -> EngineFuture<Option<SceneHandle>> {
        let engine = self.engine.clone();
        async move { engine.set_wallpaper_for_display(selector, assignment).await }.boxed()
    }
}

#[derive(Clone)]
struct AudioCaptureWorker {
    sender: Sender<AudioCaptureCommand>,
}

struct AudioCaptureCommand {
    handle: SceneHandle,
    enabled: bool,
    reply: Sender<Result<(), String>>,
}

impl AudioCaptureWorker {
    #[allow(clippy::single_call_fn)]
    fn spawn(engine: WallpaperEngine) -> Self {
        let (sender, receiver) = mpsc::channel::<AudioCaptureCommand>();
        thread::Builder::new()
            .name("wallpaper-bridge-audio-capture".to_string())
            .spawn(move || {
                let mut controller = PlatformAudioCaptureBackend::new().ok().map(|backend| {
                    AudioResponseController::new(
                        Arc::new(BridgeAudioResponseEngine { engine }),
                        backend,
                    )
                });

                while let Ok(command) = receiver.recv() {
                    let result = match controller.as_mut() {
                        Some(controller) => {
                            if command.enabled {
                                match controller
                                    .has_permission()
                                    .map_err(|error| error.to_string())
                                {
                                    Ok(true) => controller
                                        .set_scene_enabled(command.handle, true)
                                        .map_err(|error| error.to_string()),
                                    Ok(false) => match controller
                                        .request_permission()
                                        .map_err(|error| error.to_string())
                                    {
                                        Ok(true) => controller
                                            .set_scene_enabled(command.handle, true)
                                            .map_err(|error| error.to_string()),
                                        Ok(false) => Err("system audio capture permission was \
                                                          not granted"
                                            .to_string()),
                                        Err(error) => Err(error),
                                    },
                                    Err(error) => Err(error),
                                }
                            } else {
                                controller
                                    .set_scene_enabled(command.handle, false)
                                    .map_err(|error| error.to_string())
                            }
                        }
                        None => Ok(()),
                    };
                    let _ = command.reply.send(result);
                }
                Ok::<(), String>(())
            })
            .expect("audio capture worker thread should start");

        Self { sender }
    }

    fn set_enabled(&self, handle: SceneHandle, enabled: bool) -> Result<(), String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(AudioCaptureCommand {
                handle,
                enabled,
                reply,
            })
            .map_err(|error| format!("audio capture worker stopped: {error}"))?;
        response
            .recv()
            .map_err(|error| format!("audio capture worker did not reply: {error}"))?
    }
}

struct BridgeAudioResponseEngine {
    engine: WallpaperEngine,
}

impl AudioFrameConsumer for BridgeAudioResponseEngine {
    fn submit_audio_frames(
        &self,
        frames: InterleavedStereoF32<'_>,
    ) -> Result<(), AudioCaptureError> {
        self.engine.submit_audio_frames(frames)
    }
}

impl AudioResponseEngine for BridgeAudioResponseEngine {
    fn set_audio_response_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), AudioCaptureError> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("audio response runtime should start")
            .block_on(self.engine.set_audio_response_enabled(handle, enabled))
            .map_err(|error| AudioCaptureError::Engine(error.to_string()))
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct FakeEngineFacade {
    calls: Arc<ArcSwap<Vec<Vec<SceneDesc>>>>,
    snapshot: Arc<ArcSwap<Vec<DisplaySnapshotEntry>>>,
    snapshot_after_refresh: Arc<ArcSwap<Option<Vec<DisplaySnapshotEntry>>>>,
    paused_calls: Arc<ArcSwap<Vec<bool>>>,
    audio_volume_calls: Arc<ArcSwap<Vec<(SceneHandle, f32)>>>,
    audio_muted_calls: Arc<ArcSwap<Vec<(SceneHandle, bool)>>>,
    audio_response_calls: Arc<ArcSwap<Vec<(SceneHandle, bool)>>>,
    audio_capture_calls: Arc<ArcSwap<Vec<(SceneHandle, bool)>>>,
    audio_capture_block: Arc<SegQueue<ReconcileBlockGate>>,
    scaling_mode_calls: Arc<ArcSwap<Vec<(SceneHandle, ScalingMode)>>>,
    scaling_factor_calls: Arc<ArcSwap<Vec<(SceneHandle, f64)>>>,
    fps_calls: Arc<ArcSwap<Vec<(SceneHandle, u32)>>>,
    mouse_poll_calls: Arc<ArcSwap<Vec<()>>>,
    mouse_poll_block: Arc<SegQueue<ReconcileBlockGate>>,
    mouse_position_calls: Arc<ArcSwap<Vec<(SceneHandle, f64, f64)>>>,
    mouse_button_calls: Arc<ArcSwap<Vec<(SceneHandle, u32, bool)>>>,
    mouse_entered_calls: Arc<ArcSwap<Vec<(SceneHandle, bool)>>>,
    window_create_calls: Arc<ArcSwap<Vec<DisplaySelector>>>,
    wallpaper_calls: Arc<ArcSwap<Vec<(DisplaySelector, WallpaperAssignment)>>>,
    reconcile_failure: Arc<ArcSwap<Option<String>>>,
    reconcile_block: Arc<SegQueue<ReconcileBlockGate>>,
    reconcile_done: Arc<SegQueue<Sender<()>>>,
}

#[cfg(test)]
pub struct ReconcileBlock {
    blocked_rx: Receiver<()>,
    release_tx: Sender<()>,
}

#[cfg(test)]
pub struct ReconcileDone {
    done_rx: Receiver<()>,
}

#[cfg(test)]
struct ReconcileBlockGate {
    blocked_tx: Sender<()>,
    release_rx: Receiver<()>,
}

#[cfg(test)]
fn load_log<T: Clone>(log: &ArcSwap<Vec<T>>) -> Vec<T> {
    log.load_full().as_ref().clone()
}

#[cfg(test)]
fn push_log<T: Clone>(log: &ArcSwap<Vec<T>>, value: T) {
    log.rcu(|current| {
        let mut next = current.as_ref().clone();
        next.push(value.clone());
        next
    });
}

#[cfg(test)]
fn complete_reconcile_waiters(waiters: &SegQueue<Sender<()>>) {
    while let Some(waiter) = waiters.pop() {
        let _ = waiter.send(());
    }
}

#[cfg(test)]
impl ReconcileBlock {
    #[must_use]
    pub fn wait_until_blocked(&self, timeout: Duration) -> bool {
        self.blocked_rx.recv_timeout(timeout).is_ok()
    }

    pub fn release(&self) {
        let _ = self.release_tx.send(());
    }
}

#[cfg(test)]
impl ReconcileDone {
    #[must_use]
    pub fn wait(self, timeout: Duration) -> bool {
        self.done_rx.recv_timeout(timeout).is_ok()
    }
}

#[cfg(test)]
impl FakeEngineFacade {
    #[must_use]
    pub fn calls(&self) -> Vec<Vec<SceneDesc>> {
        load_log(&self.calls)
    }

    pub fn set_snapshot(&self, snapshot: Vec<DisplaySnapshotEntry>) {
        self.snapshot.store(Arc::new(snapshot));
    }

    pub fn set_snapshot_after_refresh(&self, snapshot: Vec<DisplaySnapshotEntry>) {
        self.snapshot_after_refresh.store(Arc::new(Some(snapshot)));
    }

    #[must_use]
    pub fn paused_calls(&self) -> Vec<bool> {
        load_log(&self.paused_calls)
    }

    #[must_use]
    pub fn audio_volume_calls(&self) -> Vec<(SceneHandle, f32)> {
        load_log(&self.audio_volume_calls)
    }

    #[must_use]
    pub fn audio_muted_calls(&self) -> Vec<(SceneHandle, bool)> {
        load_log(&self.audio_muted_calls)
    }

    #[must_use]
    pub fn audio_response_calls(&self) -> Vec<(SceneHandle, bool)> {
        load_log(&self.audio_response_calls)
    }

    #[must_use]
    pub fn audio_capture_calls(&self) -> Vec<(SceneHandle, bool)> {
        load_log(&self.audio_capture_calls)
    }

    #[must_use]
    pub fn scaling_mode_calls(&self) -> Vec<(SceneHandle, ScalingMode)> {
        load_log(&self.scaling_mode_calls)
    }

    #[must_use]
    pub fn scaling_factor_calls(&self) -> Vec<(SceneHandle, f64)> {
        load_log(&self.scaling_factor_calls)
    }

    #[must_use]
    pub fn fps_calls(&self) -> Vec<(SceneHandle, u32)> {
        load_log(&self.fps_calls)
    }

    #[must_use]
    pub fn mouse_poll_calls(&self) -> Vec<()> {
        load_log(&self.mouse_poll_calls)
    }

    #[must_use]
    pub fn mouse_position_calls(&self) -> Vec<(SceneHandle, f64, f64)> {
        load_log(&self.mouse_position_calls)
    }

    #[must_use]
    pub fn mouse_button_calls(&self) -> Vec<(SceneHandle, u32, bool)> {
        load_log(&self.mouse_button_calls)
    }

    #[must_use]
    pub fn mouse_entered_calls(&self) -> Vec<(SceneHandle, bool)> {
        load_log(&self.mouse_entered_calls)
    }

    #[must_use]
    pub fn window_create_calls(&self) -> Vec<DisplaySelector> {
        load_log(&self.window_create_calls)
    }

    #[must_use]
    pub fn wallpaper_calls(&self) -> Vec<(DisplaySelector, WallpaperAssignment)> {
        load_log(&self.wallpaper_calls)
    }

    pub fn fail_reconcile_with(&self, message: impl Into<String>) {
        self.reconcile_failure.store(Arc::new(Some(message.into())));
    }

    #[must_use]
    pub fn block_next_reconcile(&self) -> ReconcileBlock {
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let gate = ReconcileBlockGate {
            blocked_tx,
            release_rx,
        };
        self.reconcile_block.push(gate);

        ReconcileBlock {
            blocked_rx,
            release_tx,
        }
    }

    #[must_use]
    pub fn block_next_audio_capture(&self) -> ReconcileBlock {
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let gate = ReconcileBlockGate {
            blocked_tx,
            release_rx,
        };
        self.audio_capture_block.push(gate);

        ReconcileBlock {
            blocked_rx,
            release_tx,
        }
    }

    #[must_use]
    pub fn block_next_mouse_poll(&self) -> ReconcileBlock {
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let gate = ReconcileBlockGate {
            blocked_tx,
            release_rx,
        };
        self.mouse_poll_block.push(gate);

        ReconcileBlock {
            blocked_rx,
            release_tx,
        }
    }

    #[must_use]
    pub fn wait_for_next_reconcile(&self) -> ReconcileDone {
        let (done_tx, done_rx) = mpsc::channel();
        self.reconcile_done.push(done_tx);

        ReconcileDone { done_rx }
    }

    fn update_direct_assignment(&self, handle: SceneHandle, update: impl Fn(&mut SceneTemplate)) {
        self.snapshot.rcu(|current| {
            let mut next = current.as_ref().clone();
            if let Some(WallpaperAssignment::Direct(template)) = next
                .iter_mut()
                .find(|entry| entry.handle == Some(handle))
                .and_then(|entry| entry.assignment.as_mut())
            {
                update(template);
            }
            next
        });
    }

    fn update_direct_assignment_after_refresh(
        &self,
        handle: SceneHandle,
        update: impl Fn(&mut SceneTemplate),
    ) {
        self.snapshot_after_refresh.rcu(|current| {
            let Some(current) = current.as_ref() else {
                return None;
            };
            let mut next = current.clone();
            if let Some(WallpaperAssignment::Direct(template)) = next
                .iter_mut()
                .find(|entry| entry.handle == Some(handle))
                .and_then(|entry| entry.assignment.as_mut())
            {
                update(template);
            }
            Some(next)
        });
    }
}

#[cfg(test)]
impl EngineFacade for FakeEngineFacade {
    fn reconcile_scenes(&self, scenes: Vec<SceneDesc>) -> EngineFuture<Vec<SceneResult>> {
        let fake = self.clone();
        async move {
            push_log(&fake.calls, scenes.clone());
            if let Some(block) = fake.reconcile_block.pop() {
                let _ = block.blocked_tx.send(());
                let _ = block.release_rx.recv();
            }
            if let Some(message) = fake.reconcile_failure.load_full().as_ref().clone() {
                complete_reconcile_waiters(&fake.reconcile_done);
                return Err(EngineError::Render(message));
            }
            let results = scenes
                .iter()
                .enumerate()
                .map(|(index, scene)| {
                    SceneResult::new(
                        scene.display.display_id,
                        SceneHandle::new(index as u64 + 1),
                        0,
                    )
                })
                .collect();
            complete_reconcile_waiters(&fake.reconcile_done);
            Ok(results)
        }
        .boxed()
    }

    fn refresh_displays(&self) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            let refresh_snapshot = fake.snapshot_after_refresh.load_full().as_ref().clone();
            if let Some(snapshot) = refresh_snapshot {
                fake.snapshot.store(Arc::new(snapshot));
            }
            Ok(())
        }
        .boxed()
    }

    fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry> {
        self.snapshot.load_full().as_ref().clone()
    }

    fn close_all_scenes(&self) -> EngineFuture<()> {
        async move { Ok(()) }.boxed()
    }

    fn set_all_paused(&self, paused: bool) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.paused_calls, paused);
            Ok(())
        }
        .boxed()
    }

    fn set_audio_volume(&self, handle: SceneHandle, volume: AudioVolume) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.audio_volume_calls, (handle, f32::from(volume)));
            fake.update_direct_assignment(handle, |template| {
                template.audio_volume = volume;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.audio_volume = volume;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_audio_muted(&self, handle: SceneHandle, muted: bool) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.audio_muted_calls, (handle, muted));
            fake.update_direct_assignment(handle, |template| {
                template.audio_muted = muted;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.audio_muted = muted;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_audio_response_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.audio_response_calls, (handle, enabled));
            fake.update_direct_assignment(handle, |template| {
                template.audio_response_enabled = enabled;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.audio_response_enabled = enabled;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_audio_capture_enabled(&self, handle: SceneHandle, enabled: bool) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            if let Some(block) = fake.audio_capture_block.pop() {
                let _ = block.blocked_tx.send(());
                let _ = block.release_rx.recv();
            }
            push_log(&fake.audio_capture_calls, (handle, enabled));
            push_log(&fake.audio_response_calls, (handle, enabled));
            fake.update_direct_assignment(handle, |template| {
                template.audio_response_enabled = enabled;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.audio_response_enabled = enabled;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_scaling_mode(&self, handle: SceneHandle, mode: ScalingMode) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.scaling_mode_calls, (handle, mode));
            fake.update_direct_assignment(handle, |template| {
                template.scaling_mode = mode;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.scaling_mode = mode;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_scaling_factor(&self, handle: SceneHandle, factor: f64) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.scaling_factor_calls, (handle, factor));
            fake.update_direct_assignment(handle, |template| {
                template.scaling_factor = factor;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.scaling_factor = factor;
            });
            Ok(())
        }
        .boxed()
    }

    fn set_fps(&self, handle: SceneHandle, fps: u32) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.fps_calls, (handle, fps));
            fake.update_direct_assignment(handle, |template| {
                template.fps = fps;
            });
            fake.update_direct_assignment_after_refresh(handle, |template| {
                template.fps = fps;
            });
            Ok(())
        }
        .boxed()
    }

    fn poll_mouse_position(&self) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.mouse_poll_calls, ());
            if let Some(block) = fake.mouse_poll_block.pop() {
                let _ = block.blocked_tx.send(());
                let _ = block.release_rx.recv();
            }
            Ok(())
        }
        .boxed()
    }

    fn set_mouse_position(&self, handle: SceneHandle, x: f64, y: f64) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.mouse_position_calls, (handle, x, y));
            Ok(())
        }
        .boxed()
    }

    fn set_mouse_button(
        &self,
        handle: SceneHandle,
        button: u32,
        pressed: bool,
    ) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.mouse_button_calls, (handle, button, pressed));
            Ok(())
        }
        .boxed()
    }

    fn set_mouse_entered(&self, handle: SceneHandle, entered: bool) -> EngineFuture<()> {
        let fake = self.clone();
        async move {
            push_log(&fake.mouse_entered_calls, (handle, entered));
            Ok(())
        }
        .boxed()
    }

    fn create_window_for_display(
        &self,
        selector: DisplaySelector,
    ) -> EngineFuture<Option<SceneHandle>> {
        let fake = self.clone();
        async move {
            push_log(&fake.window_create_calls, selector);
            if let Some(message) = fake.reconcile_failure.load_full().as_ref().clone() {
                return Err(EngineError::Render(message));
            }
            Ok(Some(SceneHandle::new(98)))
        }
        .boxed()
    }

    fn set_wallpaper_for_display(
        &self,
        selector: DisplaySelector,
        assignment: WallpaperAssignment,
    ) -> EngineFuture<Option<SceneHandle>> {
        let fake = self.clone();
        async move {
            push_log(&fake.wallpaper_calls, (selector, assignment));
            if let Some(message) = fake.reconcile_failure.load_full().as_ref().clone() {
                return Err(EngineError::Render(message));
            }
            Ok(Some(SceneHandle::new(99)))
        }
        .boxed()
    }
}

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use futures_util::future::{BoxFuture, FutureExt};
use wallpaper_core::{
    DisplaySelector, DisplaySnapshotEntry, EngineError, WallpaperAssignment,
    media::audio::AudioVolume,
    project::{ScalingMode, SceneDesc, SceneHandle, SceneResult},
};

use crate::{
    BridgePlaybackState,
    api::BridgeBuilder,
    engine::{EngineFacade, FakeEngineFacade},
};

#[tokio::test]
async fn pause_and_play_update_global_snapshot_state_and_engine() {
    let engine = FakeEngineFacade::default();
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.pause_all().await.unwrap();
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Paused
    );
    wait_for_paused_calls(&engine, &[true]);

    bridge.play_all().await.unwrap();
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Playing
    );
    wait_for_paused_calls(&engine, &[true, false]);
}

#[tokio::test]
async fn failed_pause_keeps_playback_state_unchanged() {
    let engine = FailingPlaybackEngine;
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let error = bridge
        .pause_all()
        .await
        .expect_err("pause should report engine failure");

    assert!(error.message().contains("pause failed"));
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Playing
    );
}

#[tokio::test]
async fn shutdown_closes_all_engine_scenes() {
    let engine = ShutdownEngine::default();
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.shutdown().await.unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.close_calls();
        if calls == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected close calls 1, got {calls}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
async fn shutdown_disables_audio_capture_before_closing_scenes() {
    let engine = ShutdownEngine::default();
    engine.set_snapshot(vec![DisplaySnapshotEntry {
        identity: wallpaper_core::DisplayIdentity::default(),
        desc: wallpaper_core::DisplayDesc::new(7, 0, 0, 1920, 1080, 1.0),
        handle: Some(SceneHandle::new(42)),
        window_active: true,
        assignment: Some(WallpaperAssignment::Direct(
            wallpaper_core::project::SceneTemplate::builder("/tmp/project.json")
                .audio_response_enabled(true)
                .build()
                .expect("template should build"),
        )),
    }]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.shutdown().await.unwrap();

    assert_eq!(
        engine.events(),
        vec![
            ShutdownEvent::AudioCapture(SceneHandle::new(42), false),
            ShutdownEvent::CloseAll,
        ]
    );
}

fn wait_for_paused_calls(engine: &FakeEngineFacade, expected: &[bool]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.paused_calls();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected paused calls {expected:?}, got {calls:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone)]
struct FailingPlaybackEngine;

impl EngineFacade for FailingPlaybackEngine {
    fn reconcile_scenes(
        &self,
        _scenes: Vec<SceneDesc>,
    ) -> BoxFuture<'static, Result<Vec<SceneResult>, EngineError>> {
        async move { Ok(Vec::new()) }.boxed()
    }

    fn refresh_displays(&self) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok(()) }.boxed()
    }

    fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry> {
        Vec::new()
    }

    fn close_all_scenes(&self) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok(()) }.boxed()
    }

    fn set_all_paused(&self, _paused: bool) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Err(EngineError::Platform("pause failed".to_string())) }.boxed()
    }

    fn set_audio_volume(
        &self,
        _handle: SceneHandle,
        _volume: AudioVolume,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_muted(
        &self,
        _handle: SceneHandle,
        _muted: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_response_enabled(
        &self,
        _handle: SceneHandle,
        _enabled: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_capture_enabled(
        &self,
        _handle: SceneHandle,
        _enabled: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_scaling_mode(
        &self,
        _handle: SceneHandle,
        _mode: ScalingMode,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_scaling_factor(
        &self,
        _handle: SceneHandle,
        _factor: f64,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_fps(
        &self,
        _handle: SceneHandle,
        _fps: u32,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn create_window_for_display(
        &self,
        _selector: DisplaySelector,
    ) -> BoxFuture<'static, Result<Option<SceneHandle>, EngineError>> {
        async move { Ok::<Option<SceneHandle>, EngineError>(None) }.boxed()
    }

    fn set_wallpaper_for_display(
        &self,
        _selector: DisplaySelector,
        _assignment: WallpaperAssignment,
    ) -> BoxFuture<'static, Result<Option<SceneHandle>, EngineError>> {
        async move { Ok::<Option<SceneHandle>, EngineError>(None) }.boxed()
    }
}

#[derive(Clone, Default)]
struct ShutdownEngine {
    close_calls: Arc<AtomicUsize>,
    snapshot: Arc<ArcSwap<Vec<DisplaySnapshotEntry>>>,
    events: Arc<ArcSwap<Vec<ShutdownEvent>>>,
}

impl ShutdownEngine {
    fn close_calls(&self) -> usize {
        self.close_calls.load(Ordering::SeqCst)
    }

    fn set_snapshot(&self, snapshot: Vec<DisplaySnapshotEntry>) {
        self.snapshot.store(Arc::new(snapshot));
    }

    fn events(&self) -> Vec<ShutdownEvent> {
        self.events.load_full().as_ref().clone()
    }

    #[allow(clippy::needless_pass_by_value)]
    fn push_event(&self, event: ShutdownEvent) {
        self.events.rcu(|current| {
            let mut next = current.as_ref().clone();
            next.push(event.clone());
            next
        });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ShutdownEvent {
    AudioCapture(SceneHandle, bool),
    CloseAll,
}

impl EngineFacade for ShutdownEngine {
    fn reconcile_scenes(
        &self,
        _scenes: Vec<SceneDesc>,
    ) -> BoxFuture<'static, Result<Vec<SceneResult>, EngineError>> {
        async move { Ok(Vec::new()) }.boxed()
    }

    fn refresh_displays(&self) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok(()) }.boxed()
    }

    fn display_snapshot(&self) -> Vec<DisplaySnapshotEntry> {
        self.snapshot.load_full().as_ref().clone()
    }

    fn close_all_scenes(&self) -> BoxFuture<'static, Result<(), EngineError>> {
        let engine = self.clone();
        async move {
            engine.close_calls.fetch_add(1, Ordering::SeqCst);
            engine.push_event(ShutdownEvent::CloseAll);
            Ok(())
        }
        .boxed()
    }

    fn set_all_paused(&self, _paused: bool) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_volume(
        &self,
        _handle: SceneHandle,
        _volume: AudioVolume,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_muted(
        &self,
        _handle: SceneHandle,
        _muted: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_response_enabled(
        &self,
        _handle: SceneHandle,
        _enabled: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_audio_capture_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        let engine = self.clone();
        async move {
            engine.push_event(ShutdownEvent::AudioCapture(handle, enabled));
            Ok::<(), EngineError>(())
        }
        .boxed()
    }

    fn set_scaling_mode(
        &self,
        _handle: SceneHandle,
        _mode: ScalingMode,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_scaling_factor(
        &self,
        _handle: SceneHandle,
        _factor: f64,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn set_fps(
        &self,
        _handle: SceneHandle,
        _fps: u32,
    ) -> BoxFuture<'static, Result<(), EngineError>> {
        async move { Ok::<(), EngineError>(()) }.boxed()
    }

    fn create_window_for_display(
        &self,
        _selector: DisplaySelector,
    ) -> BoxFuture<'static, Result<Option<SceneHandle>, EngineError>> {
        async move { Ok::<Option<SceneHandle>, EngineError>(None) }.boxed()
    }

    fn set_wallpaper_for_display(
        &self,
        _selector: DisplaySelector,
        _assignment: WallpaperAssignment,
    ) -> BoxFuture<'static, Result<Option<SceneHandle>, EngineError>> {
        async move { Ok::<Option<SceneHandle>, EngineError>(None) }.boxed()
    }
}

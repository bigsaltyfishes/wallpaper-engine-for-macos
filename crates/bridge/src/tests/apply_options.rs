use std::{
    fs,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use wallpaper_core::{
    DisplayDesc, DisplayIdentity, DisplaySnapshotEntry, WallpaperAssignment,
    project::{ScalingMode, SceneHandle, SceneTemplate},
};

use crate::{
    BridgeErrorKind, BridgePlaybackState, BridgeScalingMode, WallpaperBridge, api::BridgeBuilder,
    config::ConfigStore, engine::FakeEngineFacade,
};

fn assert_f32_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= f32::EPSILON,
        "expected {actual} to be within f32::EPSILON of {expected}"
    );
}

fn assert_f64_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON,
        "expected {actual} to be within f64::EPSILON of {expected}"
    );
}

#[tokio::test]
async fn apply_options_clears_dirty_state() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    assert!(
        bridge
            .wallpaper_options_snapshot("100".to_string())
            .await
            .unwrap()
            .dirty
    );

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    assert!(
        !bridge
            .wallpaper_options_snapshot("100".to_string())
            .await
            .unwrap()
            .dirty
    );
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Playing
    );
}

#[tokio::test]
async fn apply_while_globally_paused_preserves_paused_snapshot_and_scene_seed() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.pause_all().await.unwrap();
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Paused
    );

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));

    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Paused
    );
    assert_eq!(engine.paused_calls(), vec![true]);

    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 1);
    assert!(calls[0][0].paused, "reconciled scene should stay paused");
}

#[tokio::test]
async fn apply_options_keeps_draft_uncommitted_when_reconcile_fails() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    engine.fail_reconcile_with("reconcile failed");
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.set_volume("100".to_string(), 0.5).await.unwrap();
    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();

    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .expect_err("reconcile should fail");
    assert!(done.wait(Duration::from_secs(2)));

    let edited = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(edited.dirty);
    assert_f32_close(edited.volume, 0.5);
    assert!(edited.display_configurations[0].enabled);

    bridge
        .cancel_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let restored = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!restored.dirty);
    assert_f32_close(restored.volume, 0.5);
    assert!(!restored.display_configurations[0].enabled);
}

#[tokio::test]
async fn cancel_options_restores_display_enable_draft() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let edited = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(edited.dirty);
    assert!(edited.display_configurations[0].enabled);

    bridge
        .cancel_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let restored = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!restored.dirty);
    assert!(!restored.display_configurations[0].enabled);
}

#[tokio::test]
async fn target_fps_is_clamped_to_display_refresh_rate() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_target_fps("100".to_string(), "7".to_string(), 144)
        .await
        .unwrap();

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert_eq!(options.display_configurations[0].max_fps, 75);
    assert_eq!(options.display_configurations[0].target_fps, 75);
}

#[tokio::test]
async fn scaling_factor_edit_updates_draft_and_apply_persists_without_reconcile() {
    let root = tempfile::tempdir().unwrap();
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    assert_eq!(engine.calls().len(), 1);
    engine.set_snapshot(vec![active_display_snapshot(7, 75, 42)]);

    bridge
        .edit_scaling_factor("100".to_string(), "7".to_string(), 1.25)
        .await
        .unwrap();

    assert_eq!(
        engine.scaling_factor_calls(),
        vec![(SceneHandle::new(42), 1.25)]
    );

    let edited = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(edited.dirty);
    assert_f64_close(edited.display_configurations[0].scaling_factor, 1.25);

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    assert_eq!(
        engine.calls().len(),
        1,
        "scaling-factor-only apply must not reconstruct scenes"
    );

    let next_engine = FakeEngineFacade::default();
    next_engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let next_bridge = BridgeBuilder::new(next_engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    next_bridge
        .inject_wallpaper_for_test("100", "Scene", crate::BridgeWallpaperKind::ProjectScene)
        .await;
    let persisted_config = ConfigStore::open(root.path().to_path_buf())
        .load_wallpaper("100")
        .unwrap();
    next_bridge
        .replace_wallpaper_config_for_test("100", persisted_config)
        .await;

    let persisted = next_bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!persisted.dirty);
    assert_f64_close(persisted.display_configurations[0].scaling_factor, 1.25);
}

#[tokio::test]
async fn scaling_factor_edit_rejects_non_positive_and_non_finite_values() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    for invalid_factor in [0.0, -0.1, f64::INFINITY, f64::NAN] {
        let error = bridge
            .edit_scaling_factor("100".to_string(), "7".to_string(), invalid_factor)
            .await
            .expect_err("invalid scaling factor should be rejected");
        assert_eq!(error.kind(), BridgeErrorKind::InvalidInput);
        assert!(error.message().contains("greater than 0"));
    }

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!options.dirty);
    assert_f64_close(options.display_configurations[0].scaling_factor, 1.0);
}

#[tokio::test]
async fn scaling_factor_edit_does_not_update_another_wallpapers_active_scene() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Other Scene")
        .await;

    bridge
        .set_display_config_enabled("200".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("200".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    engine.set_snapshot(vec![active_display_snapshot_for_wallpaper(
        7, 75, 42, "200",
    )]);

    bridge
        .edit_scaling_factor("100".to_string(), "7".to_string(), 1.25)
        .await
        .unwrap();

    assert_eq!(
        engine.scaling_factor_calls(),
        Vec::<(SceneHandle, f64)>::new()
    );
}

#[tokio::test]
async fn audio_response_option_is_editable() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_audio_response_enabled("100".to_string(), true)
        .await
        .unwrap();

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(options.audio_response_enabled);
    assert!(!options.dirty);
}

#[tokio::test]
async fn audio_option_edits_apply_to_active_scene_without_reconcile() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    assert_eq!(engine.calls().len(), 1);
    engine.set_snapshot(vec![active_display_snapshot(7, 75, 42)]);

    bridge.set_volume("100".to_string(), 0.25).await.unwrap();
    bridge.set_muted("100".to_string(), true).await.unwrap();
    bridge
        .set_audio_response_enabled("100".to_string(), true)
        .await
        .unwrap();

    assert_eq!(engine.calls().len(), 1);
    wait_for_audio_volume_calls(
        &engine,
        &[(SceneHandle::new(1), 1.0), (SceneHandle::new(42), 0.25)],
    );
    wait_for_audio_muted_calls(
        &engine,
        &[(SceneHandle::new(1), false), (SceneHandle::new(42), true)],
    );
    wait_for_audio_response_calls(
        &engine,
        &[(SceneHandle::new(1), false), (SceneHandle::new(42), true)],
    );
    wait_for_audio_capture_calls(
        &engine,
        &[(SceneHandle::new(1), false), (SceneHandle::new(42), true)],
    );

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!options.dirty);

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    assert_eq!(engine.calls().len(), 1);
    assert!(
        !bridge
            .wallpaper_options_snapshot("100".to_string())
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
async fn scaling_and_fps_option_edits_apply_to_active_scene_without_reconcile() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    assert_eq!(engine.calls().len(), 1);
    engine.set_snapshot(vec![active_display_snapshot(7, 75, 42)]);

    bridge
        .set_scaling_mode("100".to_string(), "7".to_string(), BridgeScalingMode::Fill)
        .await
        .unwrap();
    bridge
        .set_target_fps("100".to_string(), "7".to_string(), 30)
        .await
        .unwrap();

    assert_eq!(
        engine.scaling_mode_calls(),
        vec![(SceneHandle::new(42), ScalingMode::Fill)]
    );
    assert_eq!(engine.fps_calls(), vec![(SceneHandle::new(42), 30)]);

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!options.dirty);
    assert_eq!(
        options.display_configurations[0].scaling_mode,
        BridgeScalingMode::Fill
    );
    assert_eq!(options.display_configurations[0].target_fps, 30);

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    assert_eq!(engine.calls().len(), 1);
}

#[tokio::test]
async fn pending_render_option_edits_apply_to_active_scene_without_reconcile() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    assert_eq!(engine.calls().len(), 1);
    engine.set_snapshot(vec![active_display_snapshot(7, 75, 42)]);

    bridge
        .edit_scaling_factor("100".to_string(), "7".to_string(), 1.5)
        .await
        .unwrap();

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(options.dirty);
    assert_f64_close(options.display_configurations[0].scaling_factor, 1.5);

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    assert_eq!(
        engine.calls().len(),
        1,
        "render-only option edits on an active scene must not reconstruct scenes"
    );
    assert_eq!(
        engine.scaling_factor_calls(),
        vec![(SceneHandle::new(42), 1.5)]
    );
    assert!(
        !bridge
            .wallpaper_options_snapshot("100".to_string())
            .await
            .unwrap()
            .dirty
    );
}

#[tokio::test]
async fn applying_audio_response_enabled_scene_starts_audio_capture() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.set_volume("100".to_string(), 0.25).await.unwrap();
    bridge.set_muted("100".to_string(), true).await.unwrap();
    bridge
        .set_audio_response_enabled("100".to_string(), true)
        .await
        .unwrap();
    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    wait_for_audio_volume_calls(&engine, &[(SceneHandle::new(1), 0.25)]);
    wait_for_audio_muted_calls(&engine, &[(SceneHandle::new(1), true)]);
    wait_for_audio_response_calls(&engine, &[(SceneHandle::new(1), true)]);
    wait_for_audio_capture_calls(&engine, &[(SceneHandle::new(1), true)]);

    assert_eq!(
        engine.audio_volume_calls(),
        vec![(SceneHandle::new(1), 0.25)]
    );
    assert_eq!(
        engine.audio_muted_calls(),
        vec![(SceneHandle::new(1), true)]
    );
    assert_eq!(
        engine.audio_response_calls(),
        vec![(SceneHandle::new(1), true)]
    );
    assert_eq!(
        engine.audio_capture_calls(),
        vec![(SceneHandle::new(1), true)]
    );
}

#[tokio::test]
async fn live_audio_response_toggle_returns_before_audio_capture_finishes() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_state(crate::actor::state::BridgeActorState::default())
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Other")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));
    engine.set_snapshot(vec![active_display_snapshot(7, 75, 42)]);

    let block = engine.block_next_audio_capture();
    let (toggle_tx, toggle_rx) = mpsc::channel();
    let toggle_bridge = Arc::clone(&bridge);
    let toggle = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            toggle_bridge
                .set_audio_response_enabled("100".to_string(), true)
                .await
        });
        toggle_tx.send(result).unwrap();
    });
    assert!(
        block.wait_until_blocked(Duration::from_secs(2)),
        "audio response toggle did not reach audio capture"
    );

    match toggle_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => {
            result.unwrap();
        }
        Err(error) => {
            block.release();
            toggle.join().unwrap();
            panic!("audio response toggle should return before capture finishes: {error}");
        }
    }

    bridge
        .select_wallpaper("200".to_string())
        .await
        .expect("selection should stay responsive while capture finishes");
    assert_eq!(
        bridge
            .app_snapshot()
            .await
            .unwrap()
            .selected_wallpaper_id
            .as_deref(),
        Some("200")
    );

    block.release();
    toggle.join().unwrap();
}

#[tokio::test]
async fn apply_options_clamps_scene_fps_to_display_refresh_rate() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 30)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(Duration::from_secs(2)));

    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].fps, 30);
}

#[tokio::test]
async fn apply_options_updates_active_wallpaper_snapshots() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1, 75), display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let app = bridge.app_snapshot().await.unwrap();
    assert_eq!(app.active_wallpaper_ids, vec!["100".to_string()]);
    assert!(
        bridge
            .library_snapshot()
            .await
            .unwrap()
            .wallpapers
            .iter()
            .any(|entry| entry.id == "100" && entry.active)
    );
    assert!(
        bridge
            .monitor_information_snapshot()
            .await
            .unwrap()
            .rows
            .iter()
            .any(|row| row.display_id == "7" && row.wallpaper_id == "100")
    );

    bridge
        .set_display_enabled("7".to_string(), false)
        .await
        .expect("display setting should commit");

    assert!(
        bridge
            .app_snapshot()
            .await
            .unwrap()
            .active_wallpaper_ids
            .is_empty()
    );
    assert!(
        !bridge
            .library_snapshot()
            .await
            .unwrap()
            .wallpapers
            .iter()
            .any(|entry| entry.id == "100" && entry.active)
    );
}

#[tokio::test]
async fn applied_options_persist_across_bridge_instances() {
    let root = tempfile::tempdir().unwrap();
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.set_volume("100".to_string(), 0.25).await.unwrap();
    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let next_engine = FakeEngineFacade::default();
    next_engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let next_bridge = BridgeBuilder::new(next_engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    next_bridge
        .inject_wallpaper_for_test("100", "Scene", crate::BridgeWallpaperKind::ProjectScene)
        .await;
    next_bridge
        .select_wallpaper("100".to_string())
        .await
        .unwrap();

    let options = next_bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(!options.dirty);
    assert_f32_close(options.volume, 0.25);
    assert!(options.display_configurations[0].enabled);
}

#[tokio::test]
async fn failed_wallpaper_persistence_writes_app_config_first() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("wallpapers"), b"not a directory").unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .expect_err("wallpaper config write should fail");

    assert!(
        root.path().join("config.toml").exists(),
        "app config should be written before wallpaper persistence"
    );
}

#[tokio::test]
async fn control_plane_stays_responsive_when_apply_reconcile_is_in_flight() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    let block = engine.block_next_reconcile();
    let bridge = Arc::new(
        BridgeBuilder::new(engine)
            .with_state(crate::actor::state::BridgeActorState::default())
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Other")
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();

    let (apply_tx, apply_rx) = mpsc::channel();
    let apply_bridge = Arc::clone(&bridge);
    let apply = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            apply_bridge
                .apply_wallpaper_options("100".to_string())
                .await
        });
        apply_tx.send(result).unwrap();
    });
    assert!(
        block.wait_until_blocked(Duration::from_secs(2)),
        "apply did not reach reconcile"
    );

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .expect("options should be readable while reconcile is blocked");
    assert!(options.dirty);
    assert!(options.display_configurations[0].enabled);

    bridge
        .select_wallpaper("200".to_string())
        .await
        .expect("wallpaper selection should not wait for renderer reconcile");
    assert_eq!(
        bridge
            .app_snapshot()
            .await
            .unwrap()
            .selected_wallpaper_id
            .as_deref(),
        Some("200")
    );

    bridge
        .pause_all()
        .await
        .expect("tray pause should not wait for renderer reconcile");
    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Paused
    );

    block.release();
    apply_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("apply should return after renderer reconcile completes")
        .unwrap();
    apply.join().unwrap();

    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        BridgePlaybackState::Paused,
        "stale in-flight apply must not overwrite newer playback state"
    );
}

#[tokio::test]
async fn failed_reconcile_does_not_persist_config() {
    let root = tempfile::tempdir().unwrap();
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(7, 75)]);
    engine.fail_reconcile_with("reconcile failed");
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.set_volume("100".to_string(), 0.25).await.unwrap();
    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();

    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .expect_err("reconcile should fail");
    assert!(done.wait(Duration::from_secs(2)));

    assert!(
        !root.path().join("config.toml").exists(),
        "failed reconcile must not persist app config"
    );
    let persisted_wallpaper = fs::read_to_string(root.path().join("wallpapers").join("100.json"))
        .expect("immediate volume edit should have persisted wallpaper config");
    assert!(
        persisted_wallpaper.contains("\"volume\": 0.25"),
        "immediate wallpaper edits should remain persisted"
    );
    assert!(
        !persisted_wallpaper.contains("\"selector\""),
        "failed reconcile must not persist pending display assignments"
    );
}

fn wait_for_audio_capture_calls(engine: &FakeEngineFacade, expected: &[(SceneHandle, bool)]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.audio_capture_calls();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected audio capture calls {expected:?}, got {calls:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_audio_volume_calls(engine: &FakeEngineFacade, expected: &[(SceneHandle, f32)]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.audio_volume_calls();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected audio volume calls {expected:?}, got {calls:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_audio_muted_calls(engine: &FakeEngineFacade, expected: &[(SceneHandle, bool)]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.audio_muted_calls();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected audio muted calls {expected:?}, got {calls:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_audio_response_calls(engine: &FakeEngineFacade, expected: &[(SceneHandle, bool)]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let calls = engine.audio_response_calls();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected audio response calls {expected:?}, got {calls:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn display_snapshot(display_id: u32, refresh_rate_hz: u32) -> DisplaySnapshotEntry {
    let desc = DisplayDesc::with_identity(
        display_id,
        DisplayIdentity::default(),
        0,
        0,
        1920,
        1080,
        1.0,
    )
    .with_refresh_rate(refresh_rate_hz);

    DisplaySnapshotEntry {
        identity: DisplayIdentity::default(),
        desc,
        handle: None,
        window_active: false,
        assignment: None,
    }
}

fn active_display_snapshot(
    display_id: u32,
    refresh_rate_hz: u32,
    handle: u64,
) -> DisplaySnapshotEntry {
    active_display_snapshot_for_wallpaper(display_id, refresh_rate_hz, handle, "100")
}

fn active_display_snapshot_for_wallpaper(
    display_id: u32,
    refresh_rate_hz: u32,
    handle: u64,
    wallpaper_id: &str,
) -> DisplaySnapshotEntry {
    DisplaySnapshotEntry {
        handle: Some(SceneHandle::new(handle)),
        window_active: true,
        assignment: Some(WallpaperAssignment::Direct(
            SceneTemplate::builder(format!(
                "/workshop/content/431960/{wallpaper_id}/project.json"
            ))
            .build()
            .unwrap(),
        )),
        ..display_snapshot(display_id, refresh_rate_hz)
    }
}

use std::{
    sync::{Arc, mpsc},
    thread,
};

use wallpaper_core::{
    DisplayDesc, DisplayIdentity, DisplaySelector, DisplaySnapshotEntry, project::SceneHandle,
};

use crate::{
    BridgeDisplayMode, BridgeErrorKind, BridgePropertyValue, BridgeScalingMode,
    BridgeWallpaperKind, WallpaperBridge,
    api::BridgeBuilder,
    config::{
        AppConfig, ConfigStore, MonitorCfg, MonitorRender, MonitorSettingsCfg, SerializedSelector,
        WallpaperConfig,
    },
    engine::FakeEngineFacade,
    login::{LaunchAtLoginController, LaunchAtLoginStatus},
    paths::BridgePaths,
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
async fn display_settings_commit_persists_reconciles_and_clears_stale_wallpaper_drafts() {
    let root = tempfile::tempdir().unwrap();
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1), display_snapshot(7)]);
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
    assert!(done.wait(std::time::Duration::from_secs(2)));
    assert_eq!(engine.calls().len(), 1);
    assert_eq!(engine.calls()[0].len(), 1);

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    bridge
        .set_display_enabled("7".to_string(), false)
        .await
        .expect("display setting should commit");

    let calls = engine.calls();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1].is_empty(),
        "disabling an assigned display should reconcile with no scenes"
    );
    assert_display_enabled(&bridge, "7", false).await;

    let next_engine = FakeEngineFacade::default();
    next_engine.set_snapshot(vec![display_snapshot(1), display_snapshot(7)]);
    let next_bridge = BridgeBuilder::new(next_engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    next_bridge
        .inject_wallpaper_for_test("100", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;
    next_bridge
        .select_wallpaper("100".to_string())
        .await
        .unwrap();

    assert_display_enabled(&next_bridge, "7", false).await;

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert_display_enabled(&bridge, "7", false).await;
}

#[tokio::test]
async fn control_plane_stays_responsive_when_display_setting_reconcile_is_in_flight() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1), display_snapshot(7)]);
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
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let block = engine.block_next_reconcile();
    let (settings_tx, settings_rx) = mpsc::channel();
    let settings_bridge = Arc::clone(&bridge);
    let settings = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            settings_bridge
                .set_display_enabled("7".to_string(), false)
                .await
        });
        settings_tx.send(result).unwrap();
    });

    assert!(
        block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "display setting did not reach reconcile"
    );

    let snapshot = bridge
        .app_snapshot()
        .await
        .expect("snapshots should not wait for display setting reconcile");
    assert_eq!(snapshot.active_wallpaper_ids, vec!["100"]);

    bridge
        .pause_all()
        .await
        .expect("playback controls should not wait for display setting reconcile");

    block.release();
    settings_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("display setting should return after renderer reconcile completes")
        .unwrap();
    settings.join().unwrap();

    assert_eq!(
        bridge.app_snapshot().await.unwrap().playback_state,
        crate::BridgePlaybackState::Paused,
        "stale display setting commit must not overwrite newer playback state"
    );
}

#[tokio::test]
async fn later_display_mutation_wins_when_multiple_reconciles_start_from_same_generation() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary.clone()]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_config_store(ConfigStore::open(root.path().to_path_buf()))
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge.bootstrap().await.unwrap();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    let first_block = engine.block_next_reconcile();
    let second_block = engine.block_next_reconcile();

    let (disable_tx, disable_rx) = mpsc::channel();
    let disable_bridge = Arc::clone(&bridge);
    let disable_display_id = secondary_display_id.clone();
    let disable = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            disable_bridge
                .set_display_enabled(disable_display_id, false)
                .await
        });
        disable_tx.send(result).unwrap();
    });
    assert!(
        first_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "first display mutation did not reach reconcile"
    );

    let (mirror_tx, mirror_rx) = mpsc::channel();
    let mirror_bridge = Arc::clone(&bridge);
    let mirror_display_id = secondary_display_id.clone();
    let mirror = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            mirror_bridge
                .set_display_mode(mirror_display_id, BridgeDisplayMode::Mirror)
                .await
        });
        mirror_tx.send(result).unwrap();
    });
    assert!(
        second_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "second display mutation did not reach reconcile"
    );

    first_block.release();
    disable_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("first mutation should finish after first reconcile releases")
        .unwrap();
    disable.join().unwrap();

    second_block.release();
    mirror_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("second mutation should finish after second reconcile releases")
        .unwrap();
    mirror.join().unwrap();

    let display = settings_row(&bridge, &secondary_display_id).await;
    assert!(
        display.enabled,
        "later mutation should leave display enabled"
    );
    assert_eq!(display.mode, BridgeDisplayMode::Mirror);
    assert_eq!(display.selected_mirror_target.as_deref(), Some("primary"));
    assert_latest_scene(&engine, 3, "100");
}

#[tokio::test]
async fn stale_display_completion_defers_repair_until_newer_reconcile_finishes() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_config_store(ConfigStore::open(root.path().to_path_buf()))
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge.bootstrap().await.unwrap();
    let calls_before = engine.calls().len();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    let first_block = engine.block_next_reconcile();
    let second_block = engine.block_next_reconcile();
    let (disable_tx, disable_rx) = mpsc::channel();
    let disable_bridge = Arc::clone(&bridge);
    let disable_display_id = secondary_display_id.clone();
    let disable = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            disable_bridge
                .set_display_enabled(disable_display_id, false)
                .await
        });
        disable_tx.send(result).unwrap();
    });
    assert!(
        first_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "first display mutation did not reach reconcile"
    );

    let (mirror_tx, mirror_rx) = mpsc::channel();
    let mirror_bridge = Arc::clone(&bridge);
    let mirror_display_id = secondary_display_id.clone();
    let mirror = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            mirror_bridge
                .set_display_mode(mirror_display_id, BridgeDisplayMode::Mirror)
                .await
        });
        mirror_tx.send(result).unwrap();
    });
    assert!(
        second_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "second display mutation did not reach reconcile"
    );

    first_block.release();
    disable_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("first mutation should finish after reconcile releases")
        .unwrap();
    disable.join().unwrap();

    second_block.release();
    mirror_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("newer mutation should finish after second reconcile releases")
        .unwrap();
    mirror.join().unwrap();

    wait_for_reconcile_count(&engine, calls_before + 3);

    let display = settings_row(&bridge, &secondary_display_id).await;
    assert!(display.enabled);
    assert_eq!(display.mode, BridgeDisplayMode::Mirror);
    assert_eq!(display.selected_mirror_target.as_deref(), Some("primary"));
    assert_eq!(
        engine.calls().len() - calls_before,
        3,
        "a stale completion should trigger a guarded restore after the newer reconcile finishes"
    );
}

#[tokio::test]
async fn stale_reconcile_failure_defers_repair_until_newer_reconcile_finishes() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_config_store(ConfigStore::open(root.path().to_path_buf()))
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge.bootstrap().await.unwrap();
    let calls_before = engine.calls().len();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    let first_block = engine.block_next_reconcile();
    let second_block = engine.block_next_reconcile();
    let (disable_tx, disable_rx) = mpsc::channel();
    let disable_bridge = Arc::clone(&bridge);
    let disable_display_id = secondary_display_id.clone();
    let disable = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            disable_bridge
                .set_display_enabled(disable_display_id, false)
                .await
        });
        disable_tx.send(result).unwrap();
    });
    assert!(
        first_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "first display mutation did not reach reconcile"
    );

    let (mirror_tx, mirror_rx) = mpsc::channel();
    let mirror_bridge = Arc::clone(&bridge);
    let mirror_display_id = secondary_display_id.clone();
    let mirror = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            mirror_bridge
                .set_display_mode(mirror_display_id, BridgeDisplayMode::Mirror)
                .await
        });
        mirror_tx.send(result).unwrap();
    });
    assert!(
        second_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "second display mutation did not reach reconcile"
    );

    engine.fail_reconcile_with("post-reconcile failure");
    first_block.release();
    disable_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("first mutation should report reconcile failure")
        .expect_err("stale mutation should return the renderer failure");
    disable.join().unwrap();

    second_block.release();
    mirror_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("second mutation should report reconcile failure")
        .expect_err("newer mutation should return the renderer failure");
    mirror.join().unwrap();

    wait_for_reconcile_count(&engine, calls_before + 3);

    let display = settings_row(&bridge, &secondary_display_id).await;
    assert!(display.enabled);
    assert_eq!(display.mode, BridgeDisplayMode::Standalone);
    assert_eq!(
        engine.calls().len() - calls_before,
        3,
        "stale failure should schedule a guarded repair after the newer failure"
    );
}

#[tokio::test]
async fn stale_reconcile_failure_repairs_even_while_newer_reconcile_is_pending() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_config_store(ConfigStore::open(root.path().to_path_buf()))
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge.bootstrap().await.unwrap();
    let calls_before = engine.calls().len();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    let first_block = engine.block_next_reconcile();
    let second_block = engine.block_next_reconcile();
    let repair_block = engine.block_next_reconcile();
    let (disable_tx, disable_rx) = mpsc::channel();
    let disable_bridge = Arc::clone(&bridge);
    let disable_display_id = secondary_display_id.clone();
    let disable = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            disable_bridge
                .set_display_enabled(disable_display_id, false)
                .await
        });
        disable_tx.send(result).unwrap();
    });
    assert!(
        first_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "first display mutation did not reach reconcile"
    );

    let (mirror_tx, mirror_rx) = mpsc::channel();
    let mirror_bridge = Arc::clone(&bridge);
    let mirror_display_id = secondary_display_id.clone();
    let mirror = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            mirror_bridge
                .set_display_mode(mirror_display_id, BridgeDisplayMode::Mirror)
                .await
        });
        mirror_tx.send(result).unwrap();
    });
    assert!(
        second_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "second display mutation did not reach reconcile"
    );

    engine.fail_reconcile_with("post-reconcile failure");
    first_block.release();
    disable_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("first mutation should report reconcile failure")
        .expect_err("stale mutation should return the renderer failure");
    disable.join().unwrap();
    assert!(
        repair_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "stale failure did not immediately schedule repair"
    );

    second_block.release();
    mirror_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("second mutation should report reconcile failure")
        .expect_err("newer mutation should return the renderer failure");
    mirror.join().unwrap();
    repair_block.release();
    wait_for_reconcile_count(&engine, calls_before + 3);

    let display = settings_row(&bridge, &secondary_display_id).await;
    assert!(display.enabled);
    assert_eq!(display.mode, BridgeDisplayMode::Standalone);
}

#[tokio::test]
async fn stale_restore_completion_triggers_another_guarded_repair() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = Arc::new(
        BridgeBuilder::new(engine.clone())
            .with_config_store(ConfigStore::open(root.path().to_path_buf()))
            .build()
            .expect("tokio runtime and config load for wallpaper bridge"),
    );
    bridge.bootstrap().await.unwrap();
    let calls_before = engine.calls().len();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    let stale_block = engine.block_next_reconcile();
    let newer_block = engine.block_next_reconcile();
    let repair_block = engine.block_next_reconcile();

    let (disable_tx, disable_rx) = mpsc::channel();
    let disable_bridge = Arc::clone(&bridge);
    let disable_display_id = secondary_display_id.clone();
    let disable = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            disable_bridge
                .set_display_enabled(disable_display_id, false)
                .await
        });
        disable_tx.send(result).unwrap();
    });
    assert!(
        stale_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "stale display mutation did not reach reconcile"
    );

    let (mirror_tx, mirror_rx) = mpsc::channel();
    let mirror_bridge = Arc::clone(&bridge);
    let mirror_display_id = secondary_display_id.clone();
    let mirror = thread::spawn(move || {
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            mirror_bridge
                .set_display_mode(mirror_display_id, BridgeDisplayMode::Mirror)
                .await
        });
        mirror_tx.send(result).unwrap();
    });
    assert!(
        newer_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "newer display mutation did not reach reconcile"
    );

    stale_block.release();
    disable_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("stale mutation should finish after stale reconcile releases")
        .unwrap();
    disable.join().unwrap();

    newer_block.release();
    mirror_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("newer mutation should finish after newer reconcile releases")
        .unwrap();
    mirror.join().unwrap();
    assert!(
        repair_block.wait_until_blocked(std::time::Duration::from_secs(2)),
        "deferred repair did not reach reconcile"
    );

    bridge
        .set_display_enabled(secondary_display_id.clone(), false)
        .await
        .expect("newer disable should commit while repair is blocked");

    repair_block.release();
    wait_for_reconcile_count(&engine, calls_before + 5);

    let display = settings_row(&bridge, &secondary_display_id).await;
    assert!(!display.enabled);
    assert_eq!(display.mode, BridgeDisplayMode::Mirror);
}

#[tokio::test]
async fn display_settings_commit_preserves_option_edits_while_rebasing_display_assignments() {
    let root = tempfile::tempdir().unwrap();
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1), display_snapshot(7)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_project_for_test(
            "100",
            "Scene",
            r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "enabled":{"type":"bool","text":"Enabled","value":false}
            }}
        }"#,
        )
        .await;

    bridge
        .set_display_config_enabled("100".to_string(), "7".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    bridge
        .edit_property(
            "100".to_string(),
            "enabled".to_string(),
            BridgePropertyValue::Bool { value: true },
        )
        .await
        .unwrap();
    bridge
        .set_volume("100".to_string(), 0.25)
        .await
        .expect("volume edit should be accepted");

    bridge
        .set_display_enabled("7".to_string(), false)
        .await
        .expect("display setting should commit");

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .expect("draft should still be available");
    assert!(
        options.properties[0].dirty,
        "property edit should survive display settings commit"
    );
    assert!(
        (options.volume - 0.25).abs() <= f32::EPSILON,
        "expected volume {} to be within f32::EPSILON of 0.25",
        options.volume
    );
    assert!(!options.display_configurations[0].enabled);
    assert!(!options.display_configurations[0].dirty);

    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert_display_enabled(&bridge, "7", false).await;
}

#[tokio::test]
async fn settings_reject_unknown_display_and_mirror_target_without_mutating_snapshot() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_display_for_test("1", "Vendor - Model (1)")
        .await;

    let unknown_display = bridge
        .set_display_enabled("999".to_string(), false)
        .await
        .expect_err("unknown display should be rejected");
    assert_eq!(unknown_display.kind(), BridgeErrorKind::InvalidInput);

    let unknown_target = bridge
        .set_mirror_target("1".to_string(), "999".to_string())
        .await
        .expect_err("unknown mirror target should be rejected");
    assert_eq!(unknown_target.kind(), BridgeErrorKind::InvalidInput);

    let snapshot = bridge.settings_snapshot().await.unwrap();
    assert_eq!(snapshot.displays[0].mode, BridgeDisplayMode::Standalone);
    assert!(snapshot.displays[0].enabled);
    assert_eq!(snapshot.displays[0].selected_mirror_target, None);
}

#[tokio::test]
async fn settings_snapshot_includes_launch_at_login_state_from_bridge_controller() {
    let bridge = BridgeBuilder::new(FakeEngineFacade::default())
        .with_state(crate::actor::state::BridgeActorState::default())
        .with_launch_at_login(LaunchAtLoginController::fake(
            LaunchAtLoginStatus::Available { enabled: true },
        ))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let snapshot = bridge.settings_snapshot().await.unwrap();

    assert!(snapshot.launch_at_login_available);
    assert!(snapshot.launch_at_login_enabled);
}

#[tokio::test]
async fn set_launch_at_login_is_rejected_when_app_is_not_installed() {
    let bridge = BridgeBuilder::new(FakeEngineFacade::default())
        .with_state(crate::actor::state::BridgeActorState::default())
        .with_launch_at_login(LaunchAtLoginController::fake(
            LaunchAtLoginStatus::Unavailable,
        ))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let error = bridge
        .set_launch_at_login(true)
        .await
        .expect_err("uninstalled app should not toggle launch at login");

    assert_eq!(error.kind(), BridgeErrorKind::InvalidInput);
    assert!(error.message().contains("Applications"));
    let snapshot = bridge.settings_snapshot().await.unwrap();
    assert!(!snapshot.launch_at_login_available);
    assert!(!snapshot.launch_at_login_enabled);
}

#[tokio::test]
async fn set_launch_at_login_updates_bridge_owned_state() {
    let bridge = BridgeBuilder::new(FakeEngineFacade::default())
        .with_state(crate::actor::state::BridgeActorState::default())
        .with_launch_at_login(LaunchAtLoginController::fake(
            LaunchAtLoginStatus::Available { enabled: false },
        ))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.set_launch_at_login(true).await.unwrap();
    assert!(
        bridge
            .settings_snapshot()
            .await
            .unwrap()
            .launch_at_login_enabled
    );

    bridge.set_launch_at_login(false).await.unwrap();
    assert!(
        !bridge
            .settings_snapshot()
            .await
            .unwrap()
            .launch_at_login_enabled
    );
}

#[tokio::test]
async fn mirror_mode_requires_target_and_commits_mode_and_target_atomically() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_display_for_test("1", "Vendor - Model (1)")
        .await;

    let no_target = bridge
        .set_display_mode("1".to_string(), BridgeDisplayMode::Mirror)
        .await
        .expect_err("mirror mode without a target should be rejected");
    assert_eq!(no_target.kind(), BridgeErrorKind::InvalidInput);
    assert_eq!(
        bridge.settings_snapshot().await.unwrap().displays[0].mode,
        BridgeDisplayMode::Standalone
    );

    bridge
        .inject_display_for_test("2", "Vendor - Model (2)")
        .await;
    bridge
        .set_display_mode("1".to_string(), BridgeDisplayMode::Mirror)
        .await
        .expect("mirror mode should select a target atomically");

    let display = bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == "1")
        .unwrap();
    assert_eq!(display.mode, BridgeDisplayMode::Mirror);
    assert_eq!(display.selected_mirror_target.as_deref(), Some("2"));
}

#[tokio::test]
async fn secondary_display_can_mirror_to_primary_display() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary.clone(), secondary.clone()]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    bridge
        .set_display_mode(secondary_display_id.clone(), BridgeDisplayMode::Mirror)
        .await
        .expect("secondary display should be able to mirror primary");

    let display = bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == secondary_display_id)
        .unwrap();
    assert_eq!(display.mode, BridgeDisplayMode::Mirror);
    assert_eq!(display.selected_mirror_target.as_deref(), Some("primary"));
}

#[tokio::test]
async fn mirror_targets_use_stable_selector_ids_and_labels_include_primary() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let tertiary = identified_display("tertiary", 5);
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary, tertiary]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let settings = bridge.settings_snapshot().await.unwrap();
    let tertiary_row = settings
        .displays
        .iter()
        .find(|display| display.title.contains("tertiary"))
        .expect("tertiary row");
    let secondary_row = settings
        .displays
        .iter()
        .find(|display| display.title.contains("secondary"))
        .expect("secondary row");

    assert!(tertiary_row.display_id.starts_with("identity:"));
    assert!(secondary_row.display_id.starts_with("identity:"));
    assert!(tertiary_row.mirror_targets.contains(&"primary".to_string()));
    assert!(
        tertiary_row
            .mirror_targets
            .iter()
            .any(|target| target.starts_with("identity:"))
    );
    assert!(
        tertiary_row
            .mirror_targets
            .contains(&secondary_row.display_id)
    );
    assert!(!tertiary_row.mirror_targets.contains(&"1".to_string()));
    assert!(!tertiary_row.mirror_targets.contains(&"3".to_string()));
}

#[tokio::test]
async fn settings_reject_self_mirror_without_mutating_snapshot() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_display_for_test("1", "Vendor - Model (1)")
        .await;

    let result = bridge
        .set_mirror_target("1".to_string(), "1".to_string())
        .await;

    assert!(result.is_err());
    let snapshot = bridge.settings_snapshot().await.unwrap();
    assert_eq!(snapshot.displays[0].mode, BridgeDisplayMode::Standalone);
}

#[tokio::test]
async fn settings_snapshot_exposes_single_locked_primary_row() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let primary_identity =
        SerializedSelector::from_selector(&DisplaySelector::Identity(primary.identity.clone()));
    let secondary_identity =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("300".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: primary_identity,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: SerializedSelector::LiveDisplayId { display_id: 1 },
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("999".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_identity,
                    enabled: false,
                    mode: "independent".to_string(),
                    wallpaper: Some("200".to_string()),
                    mirror_target: None,
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let settings = bridge.settings_snapshot().await.unwrap();

    assert_eq!(settings.displays.len(), 2);
    assert_eq!(settings.displays[0].display_id, "primary");
    assert!(settings.displays[0].title.contains("Primary"));
    assert!(settings.displays[0].enabled);
    assert_eq!(settings.displays[0].mode, BridgeDisplayMode::Standalone);
    assert!(settings.displays[1].display_id.starts_with("identity:"));
    assert!(
        !settings
            .displays
            .iter()
            .any(|display| display.display_id == "1")
    );
}

#[tokio::test]
async fn primary_display_settings_cannot_be_disabled_or_mirrored() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1), display_snapshot(3)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge
        .set_display_enabled("primary".to_string(), false)
        .await
        .expect("primary disable request should be ignored");
    let primary = bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == "primary")
        .expect("primary row");
    assert!(primary.enabled);
    assert_eq!(primary.mode, BridgeDisplayMode::Standalone);

    let mirror_result = bridge
        .set_display_mode("primary".to_string(), BridgeDisplayMode::Mirror)
        .await;
    assert_eq!(
        mirror_result.expect_err("primary cannot mirror").kind(),
        BridgeErrorKind::InvalidInput
    );
}

#[tokio::test]
async fn stale_live_display_id_does_not_block_connected_display_setting_changes() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 2 },
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("100".to_string()),
        mirror_target: None,
    });
    store.save_app_config(&config).unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1), display_snapshot(3)]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge
        .set_display_enabled("3".to_string(), false)
        .await
        .expect("connected display change should ignore stale display id 2");

    let display = bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == "3")
        .expect("secondary display");
    assert!(!display.enabled);
}

#[tokio::test]
async fn enabling_secondary_display_updates_identity_backed_settings_row() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let secondary_identity =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_identity,
                    enabled: false,
                    mode: "independent".to_string(),
                    wallpaper: None,
                    mirror_target: None,
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge.bootstrap().await.unwrap();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    bridge
        .set_display_enabled(secondary_display_id.clone(), true)
        .await
        .expect("secondary display should be enabled");

    assert_display_enabled(&bridge, &secondary_display_id, true).await;
    assert_latest_scene(&engine, 1, "100");
    assert_eq!(
        engine.calls().last().expect("reconcile call").len(),
        1,
        "enabling an unassigned secondary display should not close the active primary wallpaper"
    );
}

#[tokio::test]
async fn wallpaper_options_hide_disabled_global_displays() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let secondary_identity =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_identity,
                    enabled: false,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();

    assert_eq!(options.display_configurations.len(), 1);
    assert_eq!(options.display_configurations[0].display_id, "primary");
}

#[tokio::test]
async fn mirror_display_setting_sends_mirror_assignment_to_engine() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge.bootstrap().await.unwrap();

    bridge
        .set_display_mode("3".to_string(), BridgeDisplayMode::Mirror)
        .await
        .expect("secondary mirror should be applied");

    assert_latest_scene(&engine, secondary.desc.display_id, "100");
}

#[tokio::test]
async fn selecting_mirror_target_for_disabled_secondary_creates_render_window() {
    let primary = identified_display("primary", 1);
    let mut secondary = identified_display("secondary", 3);
    secondary.window_active = false;
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge.bootstrap().await.unwrap();
    let secondary_display_id = settings_row_id_by_title(&bridge, "secondary").await;

    bridge
        .set_mirror_target(secondary_display_id.clone(), "primary".to_string())
        .await
        .expect("mirror target should activate the secondary display");

    assert_display_enabled(&bridge, &secondary_display_id, true).await;
    assert_latest_scene(&engine, 3, "100");
}

#[tokio::test]
async fn bootstrap_restores_mirror_display_window_from_saved_settings() {
    let primary = identified_display("primary", 1);
    let mut secondary = identified_display("secondary", 3);
    secondary.window_active = false;
    let secondary_identity = secondary.identity.clone();
    let secondary_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary_identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_selector,
                    enabled: true,
                    mode: "mirror".to_string(),
                    wallpaper: None,
                    mirror_target: Some(SerializedSelector::Primary),
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    assert_latest_scene(&engine, 3, "100");
}

#[tokio::test]
async fn applying_wallpaper_options_restores_existing_mirror_window_after_reconcile() {
    let primary = identified_display("primary", 1);
    let mut secondary = identified_display("secondary", 3);
    secondary.window_active = true;
    let secondary_identity = secondary.identity.clone();
    let secondary_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary_identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_selector,
                    enabled: true,
                    mode: "mirror".to_string(),
                    wallpaper: None,
                    mirror_target: Some(SerializedSelector::Primary),
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge
        .set_display_config_enabled("100".to_string(), "1".to_string(), true)
        .await
        .unwrap();

    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(std::time::Duration::from_secs(2)));
    assert_latest_scene(&engine, 3, "100");
}

#[tokio::test]
async fn app_config_aliases_do_not_mark_unreconciled_wallpaper_cards_active() {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let a_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_a.identity.clone()));
    let b_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_b.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: a_selector,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("200".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: b_selector,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("300".to_string()),
                    mirror_target: None,
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    for wallpaper_id in ["100", "200", "300"] {
        store
            .save_wallpaper(&WallpaperConfig::new_for(wallpaper_id, "scene"))
            .unwrap();
    }

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_a, display_b]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Primary", BridgeWallpaperKind::ProjectScene)
        .await;
    bridge
        .inject_wallpaper_for_test("200", "A", BridgeWallpaperKind::ProjectScene)
        .await;
    bridge
        .inject_wallpaper_for_test("300", "B", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.refresh_displays().await.unwrap();

    let active = bridge.app_snapshot().await.unwrap().active_wallpaper_ids;
    assert_eq!(active, vec!["100".to_string(), "300".to_string()]);
    let active_cards = bridge
        .library_snapshot()
        .await
        .unwrap()
        .wallpapers
        .into_iter()
        .filter(|wallpaper| wallpaper.active)
        .map(|wallpaper| wallpaper.id)
        .collect::<Vec<_>>();
    assert_eq!(active_cards, vec!["100".to_string(), "300".to_string()]);
}

#[tokio::test]
async fn primary_wallpaper_stays_on_primary_after_primary_display_switch() {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("300".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("300", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![display_a.clone(), display_b.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();
    assert_latest_scene(&engine, 1, "300");

    engine.set_snapshot_after_refresh(vec![display_b, display_a]);
    bridge.refresh_displays().await.unwrap();

    assert_latest_scene(&engine, 3, "300");
    assert_eq!(
        engine.calls().last().expect("reconcile call").len(),
        1,
        "primary-only wallpaper should not be duplicated onto the old primary"
    );
}

#[tokio::test]
async fn eject_wallpaper_from_display_only_disables_that_monitor_assignment() {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let b_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_b.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: b_selector,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("200".to_string()),
                    mirror_target: None,
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("200", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_a, display_b]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Primary")
        .await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Secondary")
        .await;

    bridge
        .eject_wallpaper_from_display("primary".to_string(), "100".to_string())
        .await
        .expect("primary wallpaper should be ejected");

    let saved = store.load().unwrap().config;
    assert!(
        saved
            .monitors
            .iter()
            .any(|monitor| monitor.selector == SerializedSelector::Primary
                && monitor.enabled
                && monitor.wallpaper.is_none())
    );
    assert!(saved.monitors.iter().any(|monitor| {
        monitor.wallpaper.as_deref() == Some("200") && monitor.mode == "independent"
    }));
    assert_latest_scene(&engine, 3, "200");
    assert_eq!(
        engine.calls().last().expect("reconcile call").len(),
        1,
        "ejecting one monitor should preserve other active wallpapers"
    );

    let active = bridge.app_snapshot().await.unwrap().active_wallpaper_ids;
    assert_eq!(active, vec!["200".to_string()]);

    let info = bridge.monitor_information_snapshot().await.unwrap();
    assert!(info.rows.iter().all(|row| row.wallpaper_id != "100"));
    assert!(info.rows.iter().any(|row| row.wallpaper_id == "200"));
}

#[tokio::test]
async fn monitor_information_includes_mirror_rows_with_target_metadata() {
    let (bridge, _engine, secondary_display_id) = mirrored_wallpaper_bridge().await;

    let info = bridge.monitor_information_snapshot().await.unwrap();

    assert_eq!(info.rows.len(), 2);
    let primary = info
        .rows
        .iter()
        .find(|row| row.display_id == "primary")
        .expect("primary active wallpaper row");
    assert_eq!(primary.wallpaper_id, "100");
    assert_eq!(primary.mirror_target_display_id, None);
    assert_eq!(primary.mirror_target_title, None);

    let mirror = info
        .rows
        .iter()
        .find(|row| row.display_id == secondary_display_id)
        .expect("secondary mirror row");
    assert_eq!(mirror.wallpaper_id, "100");
    assert_eq!(mirror.mirror_target_display_id.as_deref(), Some("primary"));
    assert_eq!(
        mirror.mirror_target_title.as_deref(),
        Some("Display primary - Vendor 10 - Model 1 (1 - Primary)")
    );
}

#[tokio::test]
async fn applying_primary_wallpaper_keeps_identity_fallback_for_previous_primary() {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let a_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_a.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store.save_app_config(&AppConfig::default()).unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("300", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_a.clone(), display_b.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("300", "Scene")
        .await;
    bridge
        .set_display_config_enabled("300".to_string(), "1".to_string(), true)
        .await
        .unwrap();

    bridge
        .apply_wallpaper_options("300".to_string())
        .await
        .unwrap();

    let saved_config = store.load().unwrap().config;
    let physical_primary_monitor = saved_config
        .monitors
        .iter()
        .find(|monitor| monitor.selector == a_selector)
        .expect("physical primary identity fallback should be saved");
    assert_eq!(physical_primary_monitor.wallpaper.as_deref(), Some("300"));

    engine.set_snapshot(vec![display_b, display_a]);
    bridge.refresh_displays().await.unwrap();

    assert_latest_scene(&engine, 3, "300");
    assert_latest_scene(&engine, 1, "300");
    assert_eq!(
        engine.calls().last().expect("reconcile call").len(),
        2,
        "the copied physical fallback should activate when the old primary becomes secondary"
    );
}

#[tokio::test]
async fn wallpaper_options_report_display_disabled_when_another_wallpaper_owns_it() {
    let bridge = two_wallpaper_primary_bridge().await;

    bridge
        .set_display_config_enabled("100".to_string(), "primary".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();

    let set_a_before = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(set_a_before.display_configurations[0].enabled);

    bridge
        .set_display_config_enabled("200".to_string(), "primary".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options("200".to_string())
        .await
        .unwrap();

    let set_a_after = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(
        !set_a_after.display_configurations[0].enabled,
        "Set A must report Primary disabled when Primary is assigned to Set B"
    );
    assert!(
        !set_a_after.display_configurations[0].dirty,
        "reporting a conflicting global assignment should not create a Set A draft edit"
    );
    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["200".to_string()]
    );
}

#[tokio::test]
async fn switching_primary_wallpaper_uses_new_wallpaper_saved_scaling_mode() {
    let display = identified_display("primary", 1);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store.save_app_config(&AppConfig::default()).unwrap();
    let mut first = WallpaperConfig::new_for("100", "scene");
    first.monitors.push(MonitorRender {
        selector: SerializedSelector::Primary,
        scaling_mode: "fill".to_string(),
        ..MonitorRender::default()
    });
    let mut second = WallpaperConfig::new_for("200", "scene");
    second.monitors.push(MonitorRender {
        selector: SerializedSelector::Primary,
        scaling_mode: "stretch".to_string(),
        ..MonitorRender::default()
    });
    store.save_wallpaper(&first).unwrap();
    store.save_wallpaper(&second).unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Wallpaper Set A")
        .await;
    bridge.replace_wallpaper_config_for_test("100", first).await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Wallpaper Set B")
        .await;
    bridge
        .replace_wallpaper_config_for_test("200", second)
        .await;

    apply_primary_wallpaper(&bridge, "100").await;
    wait_for_reconcile_count(&engine, 1);
    assert_latest_scene(&engine, 1, "100");
    assert_eq!(
        engine.calls().last().expect("initial reconcile")[0].scaling_mode,
        wallpaper_core::project::ScalingMode::Fill
    );

    apply_primary_wallpaper(&bridge, "200").await;
    wait_for_reconcile_count(&engine, 2);

    let calls = engine.calls();
    let latest = calls.last().expect("switch reconcile");
    assert_eq!(latest.len(), 1);
    assert_eq!(
        latest[0].scaling_mode,
        wallpaper_core::project::ScalingMode::Stretch,
        "switching wallpapers must not reuse the previous wallpaper scaling mode"
    );
    assert_latest_scene(&engine, 1, "200");
}

#[tokio::test]
async fn wallpaper_options_keep_pending_enable_after_reselecting_wallpaper() {
    let bridge = two_wallpaper_primary_bridge().await;
    apply_primary_wallpaper(&bridge, "100").await;
    apply_primary_wallpaper(&bridge, "200").await;

    bridge
        .set_display_config_enabled("100".to_string(), "primary".to_string(), true)
        .await
        .unwrap();
    let set_a_pending = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(set_a_pending.display_configurations[0].enabled);
    assert!(
        set_a_pending.display_configurations[0].dirty,
        "turning Set A back on must become an explicit pending edit"
    );

    bridge.select_wallpaper("200".to_string()).await.unwrap();
    bridge.select_wallpaper("100".to_string()).await.unwrap();
    let set_a_reselected = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();
    assert!(set_a_reselected.display_configurations[0].enabled);
    assert!(
        set_a_reselected.dirty,
        "Set A should keep the user's pending enable draft after reselection"
    );
}

#[tokio::test]
async fn wallpaper_options_disable_draft_becomes_clean_when_another_wallpaper_owns_display() {
    let bridge = two_wallpaper_primary_bridge().await;
    apply_primary_wallpaper(&bridge, "100").await;
    apply_primary_wallpaper(&bridge, "200").await;
    bridge
        .set_display_config_enabled("100".to_string(), "primary".to_string(), true)
        .await
        .unwrap();

    bridge
        .set_display_config_enabled("200".to_string(), "primary".to_string(), false)
        .await
        .unwrap();
    bridge.select_wallpaper("100".to_string()).await.unwrap();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    bridge.select_wallpaper("200".to_string()).await.unwrap();
    let set_b_reselected = bridge
        .wallpaper_options_snapshot("200".to_string())
        .await
        .unwrap();
    assert!(!set_b_reselected.display_configurations[0].enabled);
    assert!(
        !set_b_reselected.dirty,
        "Set B should become clean when its pending disable matches active ownership"
    );
}

#[tokio::test]
async fn refresh_displays_does_not_empty_reconcile_primary_wallpaper_during_transient_empty_snapshot()
 {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("300".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("300", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![display_a.clone(), display_b.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();
    assert_latest_scene(&engine, 1, "300");
    let calls_after_bootstrap = engine.calls().len();

    engine.set_snapshot_after_refresh(vec![]);
    bridge.refresh_displays().await.unwrap();

    assert_eq!(
        engine.calls().len(),
        calls_after_bootstrap,
        "transient empty display snapshot must not issue reconcile_scenes([])"
    );

    engine.set_snapshot_after_refresh(vec![display_b, display_a]);
    bridge.refresh_displays().await.unwrap();

    assert_latest_scene(&engine, 3, "300");
}

#[tokio::test]
async fn clear_shader_cache_removes_cache_and_rebuilds_active_scenes() {
    let root = tempfile::tempdir().unwrap();
    let paths = BridgePaths::for_home(root.path());
    std::fs::create_dir_all(paths.shader_cache_root()).unwrap();
    std::fs::write(paths.shader_cache_root().join("stale-cache.bin"), [1, 2, 3]).unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display_snapshot(1)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_paths(paths.clone())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    apply_primary_wallpaper(&bridge, "100").await;
    wait_for_reconcile_count(&engine, 1);

    let snapshot = bridge.clear_shader_cache().await.unwrap();

    wait_for_reconcile_count(&engine, 2);
    assert!(paths.shader_cache_root().is_dir());
    assert!(!paths.shader_cache_root().join("stale-cache.bin").exists());
    assert_eq!(snapshot.storage.shader_cache_size_bytes, 0);
    assert_latest_scene(&engine, 1, "100");
    let calls = engine.calls();
    let refreshed_scene = calls
        .last()
        .expect("cache clear should reconcile scenes")
        .iter()
        .find(|scene| scene.display.display_id == 1)
        .expect("primary display should keep an active scene");
    assert!(
        refreshed_scene.force_shader_refresh,
        "cache clear must force active scenes to rebuild their shader cache"
    );
}

fn display_snapshot(display_id: u32) -> DisplaySnapshotEntry {
    DisplaySnapshotEntry {
        identity: DisplayIdentity::default(),
        desc: DisplayDesc::with_identity(
            display_id,
            DisplayIdentity::default(),
            0,
            0,
            1920,
            1080,
            1.0,
        ),
        handle: None,
        window_active: true,
        assignment: None,
    }
}

async fn assert_display_enabled(bridge: &WallpaperBridge, display_id: &str, expected: bool) {
    let display = bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == display_id)
        .unwrap_or_else(|| panic!("missing display {display_id}"));
    assert_eq!(display.enabled, expected);
}

async fn settings_row_id_by_title(bridge: &WallpaperBridge, title_part: &str) -> String {
    bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.title.contains(title_part))
        .unwrap_or_else(|| panic!("missing display row containing title {title_part}"))
        .display_id
}

async fn settings_row(
    bridge: &WallpaperBridge,
    display_id: &str,
) -> crate::BridgeDisplaySettingsRow {
    bridge
        .settings_snapshot()
        .await
        .unwrap()
        .displays
        .into_iter()
        .find(|display| display.display_id == display_id)
        .unwrap_or_else(|| panic!("missing settings row for display {display_id}"))
}

#[tokio::test]
async fn wallpaper_options_hide_mirror_display_configuration_rows() {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let secondary_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_selector,
                    enabled: true,
                    mode: "mirror".to_string(),
                    wallpaper: None,
                    mirror_target: Some(SerializedSelector::Primary),
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .unwrap();

    assert_eq!(options.display_configurations.len(), 1);
    assert_eq!(options.display_configurations[0].display_id, "primary");
}

#[tokio::test]
async fn mirror_display_settings_apply_to_concrete_mirror_scene() {
    let (bridge, engine, secondary_display_id) = mirrored_wallpaper_bridge().await;

    let mirror = latest_scene(&engine, 3);
    assert!(mirror.audio_response_enabled);
    assert_f32_close(f32::from(mirror.audio_volume), 0.2);
    assert!(mirror.audio_muted);
    assert_eq!(
        mirror.scaling_mode,
        wallpaper_core::project::ScalingMode::Fill
    );
    assert_f64_close(mirror.scaling_factor, 1.25);
    assert_eq!(mirror.fps, 30);
    assert!(!mirror.horizontal_flip);

    let calls_before = engine.calls().len();
    bridge
        .set_mirror_volume(secondary_display_id.clone(), 0.6)
        .await
        .unwrap();
    bridge
        .set_mirror_muted(secondary_display_id.clone(), false)
        .await
        .unwrap();
    bridge
        .set_mirror_scaling_mode(secondary_display_id.clone(), BridgeScalingMode::Stretch)
        .await
        .unwrap();
    bridge
        .set_mirror_scaling_factor(secondary_display_id.clone(), 1.5)
        .await
        .unwrap();
    bridge
        .set_mirror_target_fps(secondary_display_id.clone(), 45)
        .await
        .unwrap();

    assert_eq!(
        engine.calls().len(),
        calls_before,
        "mirror display-local controls must not reconstruct scenes"
    );
    assert_eq!(
        engine.audio_volume_calls().last(),
        Some(&(SceneHandle::new(2), 0.6))
    );
    assert_eq!(
        engine.audio_muted_calls().last(),
        Some(&(SceneHandle::new(2), false))
    );
    assert_eq!(
        engine.scaling_mode_calls().last(),
        Some(&(
            SceneHandle::new(2),
            wallpaper_core::project::ScalingMode::Stretch
        ))
    );
    assert_eq!(
        engine.scaling_factor_calls().last(),
        Some(&(SceneHandle::new(2), 1.5))
    );
    assert_eq!(engine.fps_calls().last(), Some(&(SceneHandle::new(2), 45)));

    let row = settings_row(&bridge, &secondary_display_id).await;
    assert_f32_close(row.volume, 0.6);
    assert!(!row.muted);
    assert_eq!(row.scaling_mode, BridgeScalingMode::Stretch);
    assert_f64_close(row.scaling_factor, 1.5);
    assert_eq!(row.target_fps, 45);
}

#[tokio::test]
async fn display_horizontal_flip_rebuilds_scene_and_updates_settings_row() {
    let (bridge, engine, _secondary_display_id) = mirrored_wallpaper_bridge().await;

    bridge
        .set_display_horizontal_flip("primary".to_string(), true)
        .await
        .unwrap();

    let primary = latest_scene(&engine, 1);
    assert!(primary.horizontal_flip);

    let row = settings_row(&bridge, "primary").await;
    assert!(row.horizontal_flip);
}

#[tokio::test]
async fn active_display_live_scaling_update_does_not_reconcile_on_next_refresh() {
    let primary = identified_display("primary", 1);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![MonitorCfg {
                selector: SerializedSelector::Primary,
                enabled: true,
                mode: "independent".to_string(),
                wallpaper: Some("100".to_string()),
                mirror_target: None,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![primary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge.refresh_displays().await.unwrap();
    let initial_scene = engine.calls()[0][0].clone();
    let active_snapshot = vec![DisplaySnapshotEntry {
        identity: initial_scene.display.identity.clone(),
        desc: initial_scene.display.clone(),
        handle: Some(SceneHandle::new(1)),
        window_active: true,
        assignment: Some(wallpaper_core::WallpaperAssignment::Direct(
            wallpaper_core::project::SceneTemplate::from_scene_desc(&initial_scene),
        )),
    }];
    engine.set_snapshot(active_snapshot.clone());
    engine.set_snapshot_after_refresh(active_snapshot);
    bridge
        .set_scaling_mode(
            "100".to_string(),
            "primary".to_string(),
            BridgeScalingMode::Stretch,
        )
        .await
        .unwrap();
    let calls_before = engine.calls().len();

    bridge.refresh_displays().await.unwrap();

    assert_eq!(
        engine.calls().len(),
        calls_before,
        "refresh after an active live scaling update must not rebuild the scene"
    );
}

#[tokio::test]
async fn mirror_scene_tracks_source_rebuild_settings_except_monitor_overrides() {
    let (bridge, engine, secondary_display_id) = mirrored_wallpaper_bridge().await;

    bridge
        .set_mirror_volume(secondary_display_id, 0.6)
        .await
        .unwrap();
    bridge
        .inject_scene_project_for_test(
            "100",
            "Scene",
            r#"{
            "type":"scene",
            "title":"Scene",
            "general":{"properties":{
                "enabled":{"type":"bool","text":"Enabled","value":false}
            }}
        }"#,
        )
        .await;
    bridge
        .set_scaling_mode(
            "100".to_string(),
            "primary".to_string(),
            BridgeScalingMode::Stretch,
        )
        .await
        .unwrap();
    bridge
        .edit_scaling_factor("100".to_string(), "primary".to_string(), 1.75)
        .await
        .unwrap();
    bridge
        .edit_property(
            "100".to_string(),
            "enabled".to_string(),
            BridgePropertyValue::Bool { value: true },
        )
        .await
        .unwrap();
    let done = engine.wait_for_next_reconcile();
    bridge
        .apply_wallpaper_options("100".to_string())
        .await
        .unwrap();
    assert!(done.wait(std::time::Duration::from_secs(2)));
    let primary = latest_scene(&engine, 1);
    let mirror = latest_scene(&engine, 3);
    assert_eq!(
        primary.scaling_mode,
        wallpaper_core::project::ScalingMode::Stretch
    );
    assert_f64_close(primary.scaling_factor, 1.75);
    assert_eq!(
        mirror.scaling_mode,
        wallpaper_core::project::ScalingMode::Fill,
        "mirror display-local scaling mode must override the source display"
    );
    assert_f64_close(mirror.scaling_factor, 1.25);
    assert_eq!(
        primary.property_override_json.as_deref(),
        Some(r#"{"enabled":true}"#)
    );
    assert_eq!(
        mirror.property_override_json.as_deref(),
        Some(r#"{"enabled":true}"#)
    );
    assert_f32_close(f32::from(mirror.audio_volume), 0.6);
    assert!(mirror.audio_muted);
}

#[tokio::test]
async fn audio_response_general_setting_updates_active_mirror_scenes() {
    let primary = active_display("primary", 1, 41, "100");
    let secondary = active_display("secondary", 3, 42, "100");
    let secondary_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_selector,
                    enabled: true,
                    mode: "mirror".to_string(),
                    wallpaper: None,
                    mirror_target: Some(SerializedSelector::Primary),
                },
            ],
            ..AppConfig::default()
        })
        .unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;

    bridge
        .set_audio_response_enabled("100".to_string(), true)
        .await
        .unwrap();

    let mut expected_calls = vec![(SceneHandle::new(41), true), (SceneHandle::new(42), true)];
    expected_calls.sort_by_key(|(handle, enabled)| (handle.raw(), *enabled));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let mut calls = engine.audio_capture_calls();
        calls.sort_by_key(|(handle, enabled)| (handle.raw(), *enabled));
        if calls == expected_calls {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected audio capture calls {expected_calls:?}, got {calls:?}"
        );
        thread::sleep(std::time::Duration::from_millis(10));
    }
}

async fn two_wallpaper_primary_bridge() -> WallpaperBridge {
    let display = identified_display("primary", 1);
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    store.save_app_config(&AppConfig::default()).unwrap();
    for wallpaper_id in ["100", "200"] {
        store
            .save_wallpaper(&WallpaperConfig::new_for(wallpaper_id, "scene"))
            .unwrap();
    }

    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![display]);
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Wallpaper Set A")
        .await;
    bridge
        .inject_scene_wallpaper_config_for_test("200", "Wallpaper Set B")
        .await;
    bridge
}

async fn mirrored_wallpaper_bridge() -> (WallpaperBridge, FakeEngineFacade, String) {
    let primary = identified_display("primary", 1);
    let secondary = identified_display("secondary", 3);
    let secondary_selector =
        SerializedSelector::from_selector(&DisplaySelector::Identity(secondary.identity.clone()));
    let secondary_display_id = secondary_selector.id();
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut wallpaper = WallpaperConfig::new_for("100", "scene");
    wallpaper.audio.response_enabled = true;
    wallpaper.audio.volume = 0.8;
    store
        .save_app_config(&AppConfig {
            monitors: vec![
                MonitorCfg {
                    selector: SerializedSelector::Primary,
                    enabled: true,
                    mode: "independent".to_string(),
                    wallpaper: Some("100".to_string()),
                    mirror_target: None,
                },
                MonitorCfg {
                    selector: secondary_selector.clone(),
                    enabled: true,
                    mode: "mirror".to_string(),
                    wallpaper: None,
                    mirror_target: Some(SerializedSelector::Primary),
                },
            ],
            monitor_settings: vec![MonitorSettingsCfg {
                selector: secondary_selector,
                scaling_mode: "fill".to_string(),
                scaling_factor: 1.25,
                target_fps: 30,
                volume: 0.2,
                muted: true,
                horizontal_flip: false,
            }],
            ..AppConfig::default()
        })
        .unwrap();
    store.save_wallpaper(&wallpaper).unwrap();

    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![primary, secondary]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    engine.set_snapshot(vec![
        active_display("primary", 1, 1, "100"),
        active_display("secondary", 3, 2, "100"),
    ]);

    (bridge, engine, secondary_display_id)
}

async fn apply_primary_wallpaper(bridge: &WallpaperBridge, wallpaper_id: &str) {
    bridge
        .set_display_config_enabled(wallpaper_id.to_string(), "primary".to_string(), true)
        .await
        .unwrap();
    bridge
        .apply_wallpaper_options(wallpaper_id.to_string())
        .await
        .unwrap();
}

fn wait_for_reconcile_count(engine: &FakeEngineFacade, expected: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let calls = engine.calls().len();
        if calls == expected {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected {expected} reconcile calls, got {calls}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn identified_display(uuid: &str, display_id: u32) -> DisplaySnapshotEntry {
    let identity = DisplayIdentity {
        uuid: Some(uuid.to_string()),
        vendor_id: Some(10),
        model_id: Some(display_id),
        serial_number: Some(100 + display_id),
        unit_number: Some(display_id),
        name: Some(format!("Display {uuid}")),
    };
    DisplaySnapshotEntry {
        identity: identity.clone(),
        desc: DisplayDesc::with_identity(display_id, identity, 0, 0, 1920, 1080, 2.0),
        handle: None,
        window_active: true,
        assignment: None,
    }
}

fn active_display(
    uuid: &str,
    display_id: u32,
    handle: u64,
    wallpaper_id: &str,
) -> DisplaySnapshotEntry {
    let mut display = identified_display(uuid, display_id);
    display.handle = Some(SceneHandle::new(handle));
    display.assignment = Some(wallpaper_core::WallpaperAssignment::Direct(
        wallpaper_core::project::SceneTemplate::builder(format!(
            "/workshop/content/431960/{wallpaper_id}/project.json"
        ))
        .build()
        .expect("test scene template should be valid"),
    ));
    display
}

fn latest_scene(engine: &FakeEngineFacade, display_id: u32) -> wallpaper_core::project::SceneDesc {
    engine
        .calls()
        .last()
        .expect("reconcile call")
        .iter()
        .find(|scene| scene.display.display_id == display_id)
        .unwrap_or_else(|| panic!("missing scene for display {display_id}"))
        .clone()
}

fn assert_latest_scene(engine: &FakeEngineFacade, display_id: u32, workshop_id: &str) {
    let calls = engine.calls();
    let scenes = calls.last().expect("reconcile call");
    let scene = scenes
        .iter()
        .find(|scene| scene.display.display_id == display_id)
        .unwrap_or_else(|| panic!("missing scene for display {display_id}"));
    assert!(
        scene.scene_path.contains(&format!("/{workshop_id}/")),
        "display {display_id} should use wallpaper {workshop_id}, got {}",
        scene.scene_path
    );
}

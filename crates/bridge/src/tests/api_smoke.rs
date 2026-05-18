use wallpaper_core::{
    DisplayDesc, DisplayIdentity, DisplaySelector, DisplaySnapshotEntry, WallpaperAssignment,
};

use crate::{
    BridgeDisplayMode, BridgeErrorKind, BridgePlaybackState, BridgeScalingMode,
    BridgeWallpaperKind, WallpaperBridge,
    actor::state::BridgeActorState,
    api::BridgeBuilder,
    config::{AppConfig, ConfigStore, MonitorCfg, SerializedSelector, WallpaperConfig},
    engine::FakeEngineFacade,
};

#[tokio::test]
async fn bridge_starts_with_playing_state_and_empty_snapshots() {
    let bridge = WallpaperBridge::new_for_test();

    let app = bridge
        .app_snapshot()
        .await
        .expect("app snapshot should be available");
    let library = bridge
        .library_snapshot()
        .await
        .expect("library snapshot should be available");

    assert_eq!(app.playback_state, BridgePlaybackState::Playing);
    assert!(app.active_wallpaper_ids.is_empty());
    assert!(library.wallpapers.is_empty());
    assert_eq!(library.scan_status.total, 0);
}

#[tokio::test]
async fn actor_bridge_starts_with_playing_state_and_empty_snapshots() {
    let bridge = WallpaperBridge::new_for_test();

    let app = bridge
        .app_snapshot()
        .await
        .expect("app snapshot should be available");
    let library = bridge
        .library_snapshot()
        .await
        .expect("library snapshot should be available");

    assert_eq!(app.playback_state, BridgePlaybackState::Playing);
    assert!(app.active_wallpaper_ids.is_empty());
    assert!(library.wallpapers.is_empty());
    assert_eq!(library.scan_status.total, 0);
}

#[tokio::test]
async fn actor_snapshots_reflect_actor_state_mutations() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("wallpaper-1", "Wallpaper 1", BridgeWallpaperKind::Video)
        .await;
    bridge
        .select_wallpaper("wallpaper-1".to_string())
        .await
        .expect("actor select should work");

    let app = bridge
        .app_snapshot()
        .await
        .expect("app snapshot should be available");
    let library = bridge
        .library_snapshot()
        .await
        .expect("library snapshot should be available");

    assert_eq!(app.selected_wallpaper_id.as_deref(), Some("wallpaper-1"));
    assert_eq!(library.wallpapers.len(), 1);
    assert!(library.wallpapers[0].selected);
}

#[tokio::test]
async fn wallpaper_options_preserves_invalid_input_errors() {
    let bridge = WallpaperBridge::new_for_test();

    let error = bridge
        .wallpaper_options_snapshot("missing".to_string())
        .await
        .expect_err("actor domain error should be returned");

    assert_eq!(error.kind(), BridgeErrorKind::InvalidInput);
}

#[tokio::test]
async fn settings_snapshot_uses_stable_identity_mirror_target() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![
        identified_display("primary", 1),
        DisplaySnapshotEntry {
            assignment: Some(WallpaperAssignment::Mirror(DisplaySelector::Primary)),
            ..identified_display("secondary", 7)
        },
    ]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let snapshot = bridge
        .settings_snapshot()
        .await
        .expect("settings snapshot should be available");

    assert_eq!(snapshot.displays.len(), 2);
    let secondary = snapshot
        .displays
        .iter()
        .find(|display| display.title.contains("secondary"))
        .expect("secondary display row should exist");
    assert_eq!(secondary.mode, BridgeDisplayMode::Mirror);
    assert_eq!(secondary.selected_mirror_target.as_deref(), Some("primary"));
}

#[tokio::test]
async fn monitor_information_snapshot_includes_configured_metadata() {
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
    engine.set_snapshot(vec![identified_display_with_refresh("primary", 1, 90)]);
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Configured Scene")
        .await;
    bridge
        .set_audio_response_enabled("100".to_string(), true)
        .await
        .unwrap();
    bridge
        .set_scaling_mode(
            "100".to_string(),
            "primary".to_string(),
            BridgeScalingMode::Fill,
        )
        .await
        .unwrap();
    bridge
        .set_target_fps("100".to_string(), "primary".to_string(), 144)
        .await
        .unwrap();

    let snapshot = bridge
        .monitor_information_snapshot()
        .await
        .expect("monitor snapshot should be available");

    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].display_id, "primary");
    assert_eq!(snapshot.rows[0].wallpaper_id, "100");
    assert_eq!(snapshot.rows[0].wallpaper_title, "Configured Scene");
    assert_eq!(snapshot.rows[0].scaling_mode, "Fill");
    assert_eq!(snapshot.rows[0].target_fps, "90");
    assert!(snapshot.rows[0].audio_response);
}

#[tokio::test]
async fn wallpaper_options_snapshot_includes_display_config_rows() {
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(vec![
        identified_display_with_refresh("primary", 1, 60),
        identified_display_with_refresh("secondary", 7, 75),
    ]);
    let bridge = BridgeBuilder::new(engine)
        .with_state(BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_scene_wallpaper_config_for_test("100", "Scene")
        .await;
    bridge
        .set_display_config_enabled("100".to_string(), "primary".to_string(), true)
        .await
        .unwrap();
    bridge
        .set_display_config_enabled(
            "100".to_string(),
            bridge
                .settings_snapshot()
                .await
                .unwrap()
                .displays
                .into_iter()
                .find(|display| display.title.contains("secondary"))
                .unwrap_or_else(|| panic!("missing display row containing title secondary"))
                .display_id,
            false,
        )
        .await
        .unwrap();
    bridge
        .set_scaling_mode(
            "100".to_string(),
            "primary".to_string(),
            BridgeScalingMode::Fill,
        )
        .await
        .unwrap();
    bridge
        .set_target_fps("100".to_string(), "primary".to_string(), 144)
        .await
        .unwrap();

    let snapshot = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .expect("wallpaper options should be available");

    assert_eq!(snapshot.display_configurations.len(), 2);
    let primary = snapshot
        .display_configurations
        .iter()
        .find(|row| row.display_id == "primary")
        .expect("primary display config row should exist");
    assert!(primary.enabled);
    assert_eq!(primary.scaling_mode, BridgeScalingMode::Fill);
    assert_eq!(primary.target_fps, 60);
    let secondary = snapshot
        .display_configurations
        .iter()
        .find(|row| row.title.contains("secondary"))
        .expect("secondary display config row should exist");
    assert!(!secondary.enabled);
}

#[tokio::test]
async fn snapshot_bundle_records_are_constructible() {
    let bridge = WallpaperBridge::new_for_test();
    let app = bridge.app_snapshot().await.unwrap();
    let library = bridge.library_snapshot().await.unwrap();
    let monitor_information = bridge.monitor_information_snapshot().await.unwrap();
    let settings = bridge.settings_snapshot().await.unwrap();
    let bundle = crate::BridgeSnapshotBundle {
        app,
        library,
        wallpaper_options: None,
        monitor_information,
        settings,
    };

    assert!(bundle.wallpaper_options.is_none());
}

fn identified_display(uuid: &str, display_id: u32) -> DisplaySnapshotEntry {
    identified_display_with_refresh(uuid, display_id, 60)
}

fn identified_display_with_refresh(
    uuid: &str,
    display_id: u32,
    refresh_rate_hz: u32,
) -> DisplaySnapshotEntry {
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
        desc: DisplayDesc::with_identity(display_id, identity, 0, 0, 1920, 1080, 2.0)
            .with_refresh_rate(refresh_rate_hz),
        handle: None,
        window_active: true,
        assignment: None,
    }
}

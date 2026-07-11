use wallpaper_core::{
    DisplayDesc, DisplayIdentity, DisplaySelector, DisplaySnapshotEntry, WallpaperAssignment,
    project::{ScalingMode, SceneTemplate},
};

use crate::{
    BridgeDisplayMode, BridgeErrorKind, BridgeWallpaperKind, WallpaperBridge,
    api::BridgeBuilder,
    config::{AppConfig, ConfigStore, MonitorCfg, SerializedSelector, WallpaperConfig},
    engine::FakeEngineFacade,
};

#[tokio::test]
async fn selecting_wallpaper_updates_snapshot_selection() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("100", "Scene One", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let app = bridge.app_snapshot().await.unwrap();
    let library = bridge.library_snapshot().await.unwrap();
    assert_eq!(app.selected_wallpaper_id.as_deref(), Some("100"));
    assert!(
        library
            .wallpapers
            .iter()
            .any(|entry| entry.id == "100" && entry.selected)
    );
}

#[tokio::test]
async fn selecting_wallpaper_returns_options_for_inspector() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("100", "Scene One", BridgeWallpaperKind::ProjectScene)
        .await;

    let bundle = bridge.select_wallpaper("100".to_string()).await.unwrap();

    let options = bundle
        .wallpaper_options
        .expect("selection bundle should include inspector options");
    assert_eq!(options.wallpaper_id, "100");
    assert_eq!(options.title, "Scene One");
}

#[tokio::test]
async fn library_snapshot_counts_all_wallpaper_kinds() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("scene", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;
    bridge
        .inject_wallpaper_for_test("video", "Video", BridgeWallpaperKind::Video)
        .await;
    bridge
        .inject_wallpaper_for_test("web", "Web", BridgeWallpaperKind::Webpage)
        .await;
    bridge
        .inject_wallpaper_for_test("unknown", "Unknown", BridgeWallpaperKind::Unknown)
        .await;

    let library = bridge.library_snapshot().await.unwrap();

    assert_eq!(library.scene_count, 1);
    assert_eq!(library.video_count, 1);
    assert_eq!(library.webpage_count, 1);
    assert_eq!(library.unknown_count, 1);
    assert_eq!(library.wallpapers.len(), 4);
}

#[tokio::test]
async fn selecting_unknown_wallpaper_keeps_existing_selection() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("100", "Scene One", BridgeWallpaperKind::ProjectScene)
        .await;
    bridge.select_wallpaper("100".to_string()).await.unwrap();

    let err = bridge
        .select_wallpaper("missing".to_string())
        .await
        .expect_err("unknown wallpaper should fail");

    assert_eq!(err.kind(), BridgeErrorKind::InvalidInput);
    assert!(err.message().contains("missing"));
    assert_eq!(
        bridge
            .app_snapshot()
            .await
            .unwrap()
            .selected_wallpaper_id
            .as_deref(),
        Some("100")
    );
}

#[tokio::test]
async fn wallpaper_options_snapshot_returns_entry_defaults() {
    let bridge = WallpaperBridge::new_for_test();
    bridge
        .inject_wallpaper_for_test("100", "Scene One", BridgeWallpaperKind::ProjectScene)
        .await;

    let options = bridge
        .wallpaper_options_snapshot("100".to_string())
        .await
        .expect("options should exist");

    assert_eq!(options.wallpaper_id, "100");
    assert_eq!(options.title, "Scene One");
    assert_eq!(options.kind, BridgeWallpaperKind::ProjectScene);
    assert!(options.supported);
    assert!(!options.dirty);
    assert!(options.display_configurations.is_empty());
    assert!(!options.audio_response_enabled);
    assert!(!options.muted);
    assert!(
        (options.volume - 1.0).abs() <= f32::EPSILON,
        "expected volume {} to be within f32::EPSILON of 1.0",
        options.volume
    );
}

#[tokio::test]
async fn wallpaper_options_snapshot_rejects_unknown_wallpaper_with_id() {
    let bridge = WallpaperBridge::new_for_test();

    let err = bridge
        .wallpaper_options_snapshot("missing".to_string())
        .await
        .expect_err("unknown wallpaper should fail");

    assert_eq!(err.kind(), BridgeErrorKind::InvalidInput);
    assert!(err.message().contains("missing"));
}

#[tokio::test]
async fn display_snapshots_are_empty_without_engine_displays() {
    let bridge = WallpaperBridge::new_for_test();

    assert!(
        bridge
            .monitor_information_snapshot()
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(
        bridge
            .settings_snapshot()
            .await
            .unwrap()
            .displays
            .is_empty()
    );
}

#[tokio::test]
async fn display_snapshots_are_built_from_engine_state() {
    let internal = DisplayIdentity {
        vendor_id: Some(123),
        model_id: Some(456),
        name: Some("Internal Display".to_string()),
        ..DisplayIdentity::default()
    };
    let external = DisplayIdentity {
        vendor_id: Some(789),
        model_id: Some(101),
        name: Some("Studio Display".to_string()),
        ..DisplayIdentity::default()
    };
    let template = SceneTemplate::builder("/workshop/content/431960/100/project.json")
        .fps(90)
        .scaling_mode(ScalingMode::Fit)
        .audio_response_enabled(true)
        .build()
        .unwrap();
    let snapshot = vec![
        DisplaySnapshotEntry {
            identity: internal.clone(),
            desc: DisplayDesc::with_identity(1, internal.clone(), 0, 0, 3024, 1964, 2.0),
            handle: None,
            window_active: true,
            assignment: Some(WallpaperAssignment::Direct(template)),
        },
        DisplaySnapshotEntry {
            identity: external.clone(),
            desc: DisplayDesc::with_identity(2, external, 3024, 0, 5120, 2880, 2.0),
            handle: None,
            window_active: false,
            assignment: Some(WallpaperAssignment::Mirror(DisplaySelector::LiveDisplayId(
                1,
            ))),
        },
    ];
    let engine = FakeEngineFacade::default();
    engine.set_snapshot(snapshot);
    let bridge = BridgeBuilder::new(engine)
        .with_state(crate::actor::state::BridgeActorState::default())
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    let monitor = bridge.monitor_information_snapshot().await.unwrap();
    assert!(
        monitor.rows.is_empty(),
        "display page should list configured active wallpapers, not raw renderer assignments"
    );

    let settings = bridge.settings_snapshot().await.unwrap();
    assert_eq!(settings.displays.len(), 2);
    assert_eq!(settings.displays[0].display_id, "primary");
    assert!(settings.displays[0].enabled);
    assert_eq!(settings.displays[0].mode, BridgeDisplayMode::Standalone);
    assert!(!settings.displays[1].enabled);
    assert_eq!(settings.displays[1].mode, BridgeDisplayMode::Mirror);
    assert_eq!(
        settings.displays[1].selected_mirror_target.as_deref(),
        Some("primary")
    );
    assert_eq!(settings.bridge_version, "0.1.2");
    assert_eq!(settings.core_version, "0.2.0");
    assert_eq!(settings.web_version, "0.1.0");
    assert_eq!(settings.shader_pipeline_version, "0.1.0");
}

#[tokio::test]
async fn monitor_information_lists_only_active_wallpaper_displays_with_metadata() {
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
    let bridge = BridgeBuilder::new(engine)
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Configured Scene", BridgeWallpaperKind::ProjectScene)
        .await;

    let monitor = bridge.monitor_information_snapshot().await.unwrap();

    assert_eq!(monitor.rows.len(), 1);
    assert_eq!(monitor.rows[0].display_id, "primary");
    assert_eq!(monitor.rows[0].wallpaper_id, "100");
    assert_eq!(monitor.rows[0].wallpaper_title, "Configured Scene");
    assert!(monitor.rows[0].title.contains("(1 - Primary - 100)"));
}

#[tokio::test]
async fn bootstrap_refreshes_displays_and_reconciles_configured_wallpapers() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 7 },
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
    engine.set_snapshot_after_refresh(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();

    assert_eq!(bridge.settings_snapshot().await.unwrap().displays.len(), 1);
    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["100".to_string()]
    );
    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].display.display_id, 7);
}

#[tokio::test]
async fn refresh_displays_reconciles_configured_wallpapers() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 7 },
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
    engine.set_snapshot_after_refresh(vec![display_snapshot(7, 75)]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.refresh_displays().await.unwrap();

    assert_eq!(bridge.settings_snapshot().await.unwrap().displays.len(), 1);
    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["100".to_string()]
    );
    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].display.display_id, 7);
}

#[tokio::test]
async fn refresh_displays_skips_reconcile_when_configured_scenes_are_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::Primary,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("100".to_string()),
        mirror_target: None,
    });
    store.save_app_config(&config).unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let display = display_snapshot(7, 75);
    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![display.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.refresh_displays().await.unwrap();
    let calls = engine.calls();
    assert_eq!(calls.len(), 1);

    let rendered_scene = calls[0][0].clone();
    engine.set_snapshot_after_refresh(vec![DisplaySnapshotEntry {
        identity: display.identity,
        desc: display.desc,
        handle: Some(wallpaper_core::project::SceneHandle::new(1)),
        window_active: true,
        assignment: Some(WallpaperAssignment::Direct(SceneTemplate::from_scene_desc(
            &rendered_scene,
        ))),
    }]);

    bridge.refresh_displays().await.unwrap();

    assert_eq!(
        engine.calls().len(),
        1,
        "refreshing unchanged active display state must not rebuild the scene"
    );
}

#[tokio::test]
async fn refresh_displays_skips_reconcile_when_existing_window_is_temporarily_inactive() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 7 },
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("100".to_string()),
        mirror_target: None,
    });
    store.save_app_config(&config).unwrap();
    store
        .save_wallpaper(&WallpaperConfig::new_for("100", "scene"))
        .unwrap();

    let display = display_snapshot(7, 75);
    let engine = FakeEngineFacade::default();
    engine.set_snapshot_after_refresh(vec![display.clone()]);
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.refresh_displays().await.unwrap();
    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    let rendered_scene = calls[0][0].clone();

    engine.set_snapshot_after_refresh(vec![DisplaySnapshotEntry {
        identity: display.identity,
        desc: display.desc,
        handle: Some(wallpaper_core::project::SceneHandle::new(1)),
        window_active: false,
        assignment: Some(WallpaperAssignment::Direct(SceneTemplate::from_scene_desc(
            &rendered_scene,
        ))),
    }]);

    bridge.refresh_displays().await.unwrap();

    assert_eq!(
        engine.calls().len(),
        1,
        "a minimized or temporarily inactive wallpaper window must not reconstruct an unchanged \
         scene"
    );
}

#[tokio::test]
async fn refresh_displays_reconciles_configured_wallpaper_after_missing_display_returns() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 7 },
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
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");
    bridge
        .inject_wallpaper_for_test("100", "Scene", BridgeWallpaperKind::ProjectScene)
        .await;

    bridge.bootstrap().await.unwrap();
    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["100".to_string()]
    );

    engine.set_snapshot_after_refresh(vec![display_snapshot(7, 75)]);
    bridge.refresh_displays().await.unwrap();

    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["100".to_string()]
    );
    let calls = engine.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].display.display_id, 7);
}

#[tokio::test]
async fn bootstrap_does_not_mark_configured_wallpaper_active_without_matching_display() {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::open(root.path().to_path_buf());
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::LiveDisplayId { display_id: 7 },
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
    let bridge = BridgeBuilder::new(engine.clone())
        .with_config_store(ConfigStore::open(root.path().to_path_buf()))
        .build()
        .expect("tokio runtime and config load for wallpaper bridge");

    bridge.bootstrap().await.unwrap();

    assert_eq!(
        bridge.app_snapshot().await.unwrap().active_wallpaper_ids,
        vec!["100".to_string()]
    );
    assert!(engine.calls().is_empty());
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
        window_active: true,
        assignment: None,
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

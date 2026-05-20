use std::collections::BTreeMap;

use wallpaper_core::{DisplayDesc, DisplayIdentity, DisplaySelector, DisplaySnapshotEntry};

use crate::{
    config::{AppConfig, MonitorCfg, SerializedSelector, WallpaperConfig},
    engine::ActivationInputs,
    paths::BridgePaths,
};

#[test]
fn activation_plan_marks_scenes_paused_when_global_playback_is_paused() {
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::Primary,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("100".to_string()),
        mirror_target: None,
    });
    let mut wallpapers = BTreeMap::new();
    wallpapers.insert("100".to_string(), WallpaperConfig::new_for("100", "scene"));
    let display = DisplayDesc::with_identity(1, DisplayIdentity::default(), 0, 0, 1920, 1080, 2.0);
    let displays = vec![DisplaySnapshotEntry {
        identity: DisplayIdentity::default(),
        desc: display,
        handle: None,
        window_active: true,
        assignment: None,
    }];
    let paths = BridgePaths::for_home("/Users/example");

    let scenes = ActivationInputs {
        app_config: &config,
        wallpapers: &wallpapers,
        displays: &displays,
        paused: true,
        paths: &paths,
        force_shader_refresh: false,
    }
    .build()
    .unwrap();

    assert_eq!(scenes.len(), 1);
    assert!(scenes[0].paused);
}

#[test]
fn activation_plan_gives_primary_wallpaper_to_current_primary_display() {
    let display_a = identified_display("a", 1);
    let display_b = identified_display("b", 3);
    let selector_a =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_a.identity.clone()));
    let selector_b =
        SerializedSelector::from_selector(&DisplaySelector::Identity(display_b.identity.clone()));
    let mut config = AppConfig::default();
    config.monitors.push(MonitorCfg {
        selector: selector_a,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("100".to_string()),
        mirror_target: None,
    });
    config.monitors.push(MonitorCfg {
        selector: selector_b,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("200".to_string()),
        mirror_target: None,
    });
    config.monitors.push(MonitorCfg {
        selector: SerializedSelector::Primary,
        enabled: true,
        mode: "independent".to_string(),
        wallpaper: Some("300".to_string()),
        mirror_target: None,
    });
    let mut wallpapers = BTreeMap::new();
    for id in ["100", "200", "300"] {
        wallpapers.insert(id.to_string(), WallpaperConfig::new_for(id, "scene"));
    }

    let displays = vec![display_a.clone(), display_b.clone()];
    let paths = BridgePaths::for_home("/Users/example");
    let scenes = ActivationInputs {
        app_config: &config,
        wallpapers: &wallpapers,
        displays: &displays,
        paused: false,
        paths: &paths,
        force_shader_refresh: false,
    }
    .build()
    .unwrap();
    assert_eq!(scenes.len(), 2);
    assert_scene(&scenes, 1, "300");
    assert_scene(&scenes, 3, "200");

    let displays = vec![display_b, display_a];
    let scenes = ActivationInputs {
        app_config: &config,
        wallpapers: &wallpapers,
        displays: &displays,
        paused: false,
        paths: &paths,
        force_shader_refresh: false,
    }
    .build()
    .unwrap();
    assert_eq!(scenes.len(), 2);
    assert_scene(&scenes, 3, "300");
    assert_scene(&scenes, 1, "100");
}

fn assert_scene(scenes: &[wallpaper_core::project::SceneDesc], display_id: u32, workshop_id: &str) {
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

use std::future::Future;

use crate::{
    DisplayConfig, DisplayDesc, DisplayIdentity, DisplaySelector, EngineError, WallpaperAssignment,
    WallpaperEngine, WallpaperEngineConfig,
    media::audio::AudioVolume,
    project::{ScalingMode, SceneDesc, SceneHandle},
};

#[test]
pub fn case_rust_api_boundary() {
    let constructor: fn() -> Result<WallpaperEngine, EngineError> = WallpaperEngine::new;

    let identity = DisplayIdentity {
        uuid: Some("display-uuid".to_string()),
        vendor_id: Some(100),
        model_id: Some(200),
        serial_number: Some(300),
        unit_number: Some(1),
        name: Some("Studio Display".to_string()),
    };
    let display_with_identity =
        DisplayDesc::with_identity(7, identity.clone(), -1440, 0, 2880, 1800, 2.0);
    assert_eq!(display_with_identity.identity, identity.clone());

    let config = WallpaperEngineConfig {
        displays: vec![DisplayConfig {
            selector: DisplaySelector::Identity(identity),
            window_active: true,
            wallpaper: None,
        }],
    };
    assert_eq!(config.displays.len(), 1);

    let mirror = WallpaperAssignment::Mirror(DisplaySelector::Primary);
    let config_constructor: fn(WallpaperEngineConfig) -> Result<WallpaperEngine, EngineError> =
        WallpaperEngine::with_config;

    let scene = SceneDesc::builder(
        DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0),
        "/tmp/project.json",
    )
    .assets_path("/tmp/assets")
    .fps(60)
    .paused(false)
    .build()
    .expect("scene descriptor should build");

    assert_eq!(scene.display.display_id, 1);
    assert_eq!(scene.fps, 60);
    assert_eq!(scene.audio_volume, AudioVolume::try_from(1.0).unwrap());
    assert!(!scene.audio_muted);
    assert_ne!(SceneHandle::new(1).raw(), 0);

    let async_api_check: fn(
        &WallpaperEngine,
        DisplaySelector,
        WallpaperAssignment,
        SceneHandle,
        ScalingMode,
        AudioVolume,
        Vec<SceneDesc>,
    ) = |engine, selector, wallpaper, handle, scaling_mode, audio_volume, scenes| {
        assert_async_result(engine.refresh_displays());
        assert_async_result(engine.reconcile_scenes(scenes));
        assert_async_result(engine.create_window_for_display(selector.clone()));
        assert_async_result(engine.destroy_window_for_display(selector.clone()));
        assert_async_result(engine.set_wallpaper_for_display(selector, wallpaper));
        assert_async_result(engine.set_scaling_mode(handle, scaling_mode));
        assert_async_result(engine.set_scaling_factor(handle, 1.25));
        assert_async_result(engine.set_fps(handle, 60));
        assert_async_result(engine.set_render_resolution(handle, 1920, 1080));
        assert_async_result(engine.set_mouse_position(handle, 0.25, 0.75));
        assert_async_result(engine.set_mouse_button(handle, 0, true));
        assert_async_result(engine.set_mouse_entered(handle, true));
        assert_async_result(engine.set_audio_response_enabled(handle, true));
        assert_async_result(engine.set_audio_volume(handle, audio_volume));
        assert_async_result(engine.set_audio_muted(handle, true));
        assert_async_result(engine.set_property_override(handle, "{}".to_string()));
        assert_async_result(engine.reset_property_override(handle));
        assert_async_result(engine.close_all_scenes());
    };
    std::hint::black_box((
        constructor,
        config_constructor,
        mirror,
        async_api_check,
        config,
    ));
}

#[test]
pub fn case_scene_builder_rejects_empty_scene_path() {
    let error = SceneDesc::builder(display(), "")
        .build()
        .expect_err("empty scene_path should fail");

    assert_invalid_input(error, "scene_path must not be empty");
}

#[test]
pub fn case_scene_builder_rejects_zero_fps() {
    let error = SceneDesc::builder(display(), "/tmp/project.json")
        .fps(0)
        .build()
        .expect_err("zero fps should fail");

    assert_invalid_input(error, "fps must be greater than zero");
}

#[test]
pub fn case_scene_builder_preserves_defaults() {
    let scene = SceneDesc::builder(display(), "/tmp/project.json")
        .build()
        .expect("scene descriptor should build");

    assert_eq!(scene.assets_path, "");
    assert_eq!(scene.fps, 60);
    assert!(!scene.paused);
    assert!(!scene.audio_response_enabled);
    assert_eq!(scene.audio_volume, AudioVolume::try_from(1.0).unwrap());
    assert!(!scene.audio_muted);
    assert_eq!(scene.property_override_json, None);
    assert_eq!(scene.shader_cache_path, None);
    assert!(!scene.force_shader_refresh);
}

fn display() -> DisplayDesc {
    DisplayDesc::new(1, 0, 0, 1920, 1080, 1.0)
}

fn assert_async_result<Fut, T>(_future: Fut)
where
    Fut: Future<Output = Result<T, EngineError>>,
{
}

fn assert_invalid_input(error: EngineError, expected: &str) {
    match error {
        EngineError::InvalidInput(message) => assert_eq!(message, expected),
        other => panic!("expected invalid input, got {other:?}"),
    }
}

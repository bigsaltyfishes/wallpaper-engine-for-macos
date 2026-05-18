use std::future::Future;

use wallpaper_core::{
    DisplayDesc, EngineError, WallpaperEngine,
    media::audio::{AudioCaptureError, AudioFrameConsumer, AudioVolume, InterleavedStereoF32},
    project::{SceneDesc, SceneHandle},
};

#[test]
fn scene_desc_defaults_audio_response_to_disabled() {
    let scene = SceneDesc::new(
        DisplayDesc::new(1, 0, 0, 2560, 1440, 2.0),
        "/tmp/project.json",
        "/tmp/assets",
        60,
        false,
    );

    assert!(!scene.audio_response_enabled);
    assert_eq!(scene.audio_volume, AudioVolume::try_from(1.0).unwrap());
    assert!(!scene.audio_muted);
}

#[test]
fn scene_desc_allows_audio_response_to_be_enabled() {
    let scene = SceneDesc::new(
        DisplayDesc::new(1, 0, 0, 2560, 1440, 2.0),
        "/tmp/project.json",
        "/tmp/assets",
        60,
        false,
    )
    .with_audio_response_enabled(true);

    assert!(scene.audio_response_enabled);
}

#[test]
fn scene_desc_allows_audio_volume_and_mute_configuration() {
    for volume in [0.0, 0.5, 1.0] {
        let scene = SceneDesc::builder(
            DisplayDesc::new(1, 0, 0, 2560, 1440, 2.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .audio_volume(volume)
        .audio_muted(true)
        .build()
        .expect("scene descriptor should build");

        assert_eq!(scene.audio_volume, AudioVolume::try_from(volume).unwrap());
        assert!(scene.audio_muted);
    }
}

#[test]
fn scene_desc_keeps_override_and_shader_cache_configuration() {
    let scene = SceneDesc::new(
        DisplayDesc::new(1, 0, 0, 2560, 1440, 2.0),
        "/tmp/project.json",
        "/tmp/assets",
        60,
        false,
    )
    .with_property_override_json(r#"{"example":{"inner":true}}"#)
    .with_shader_cache_path("/tmp/we-cache")
    .with_force_shader_refresh(true);

    assert_eq!(
        scene.property_override_json.as_deref(),
        Some(r#"{"example":{"inner":true}}"#)
    );
    assert_eq!(scene.shader_cache_path.as_deref(), Some("/tmp/we-cache"));
    assert!(scene.force_shader_refresh);
}

#[test]
#[should_panic]
fn scene_desc_rejects_invalid_audio_volume() {
    for volume in [f32::NAN, -0.01, 1.01, f32::INFINITY, f32::NEG_INFINITY] {
        let error = SceneDesc::builder(
            DisplayDesc::new(1, 0, 0, 2560, 1440, 2.0),
            "/tmp/project.json",
        )
        .assets_path("/tmp/assets")
        .audio_volume(volume)
        .build()
        .expect_err("invalid audio volume should fail");

        match error {
            EngineError::InvalidInput(message) => {
                assert_eq!(message, "audio_volume must be finite and within [0.0, 1.0]");
            }
            other => panic!("expected invalid input, got {other:?}"),
        }
    }
}

#[test]
fn interleaved_stereo_frames_validate_format_and_count_frames() {
    let samples = [0.0f32, 0.1, 0.2, 0.3, 0.4, 0.5];
    let frames = InterleavedStereoF32::new(48_000, &samples).expect("valid stereo frames");

    assert_eq!(frames.sample_rate(), 48_000);
    assert_eq!(frames.frame_count(), 3);
    assert_eq!(frames.samples(), &samples);
}

#[test]
fn interleaved_stereo_frames_reject_invalid_inputs() {
    assert!(InterleavedStereoF32::new(0, &[0.0, 1.0]).is_err());
    assert!(InterleavedStereoF32::new(48_000, &[]).is_err());
    assert!(InterleavedStereoF32::new(48_000, &[0.0, 1.0, 2.0]).is_err());
}

#[test]
fn macos_engine_exposes_audio_response_methods() {
    fn assert_async_audio_response_api(
        engine: &WallpaperEngine,
        handle: SceneHandle,
        volume: AudioVolume,
    ) {
        assert_async_result(engine.set_audio_response_enabled(handle, true));
        assert_async_result(engine.set_audio_volume(handle, volume));
        assert_async_result(engine.set_audio_muted(handle, true));
    }

    let _async_audio_api: fn(&WallpaperEngine, SceneHandle, AudioVolume) =
        assert_async_audio_response_api;
    let submit: fn(&WallpaperEngine, InterleavedStereoF32<'_>) -> Result<(), AudioCaptureError> =
        <WallpaperEngine as AudioFrameConsumer>::submit_audio_frames;

    let _ = submit;
}

#[test]
fn macos_engine_exposes_property_override_methods() {
    fn assert_async_property_api(engine: &WallpaperEngine, handle: SceneHandle) {
        assert_async_result(engine.set_property_override(handle, "{}"));
        assert_async_result(engine.reset_property_override(handle));
    }

    let _async_property_api: fn(&WallpaperEngine, SceneHandle) = assert_async_property_api;
}

fn assert_async_result<Fut, T>(_future: Fut)
where
    Fut: Future<Output = Result<T, EngineError>>,
{
}

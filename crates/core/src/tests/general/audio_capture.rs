use std::sync::{Arc, Mutex};

use crate::{
    media::audio::{
        AudioCaptureBackend, AudioCaptureError, AudioFrameConsumer, AudioInputError,
        AudioResponseController, AudioResponseEngine, AudioResponseResampler, InterleavedStereoF32,
        MonoPcmF32, PlatformAudioCaptureBackend,
    },
    project::SceneHandle,
};

#[test]
pub fn case_audio_capture_controller_starts_and_stops_backend() {
    let engine = Arc::new(FakeEngine::default());
    let backend = FakeBackend::default();
    let mut controller = AudioResponseController::new(engine, backend);
    let handle = SceneHandle::new(42);

    controller
        .set_scene_enabled(handle, true)
        .expect("enabling audio response should start capture");
    assert!(controller.is_capturing());
    assert_eq!(controller.active_scene_count(), 1);

    controller
        .set_scene_enabled(handle, false)
        .expect("disabling final scene should stop capture");
    assert!(!controller.is_capturing());
    assert_eq!(controller.active_scene_count(), 0);
}

#[test]
pub fn case_interleaved_stereo_rejects_invalid_buffers() {
    assert!(matches!(
        InterleavedStereoF32::new(0, &[0.0, 0.0]),
        Err(AudioInputError::InvalidSampleRate)
    ));
    assert!(matches!(
        InterleavedStereoF32::new(48_000, &[]),
        Err(AudioInputError::EmptyInput)
    ));
    assert!(matches!(
        InterleavedStereoF32::new(48_000, &[0.0]),
        Err(AudioInputError::OddSampleCount(1))
    ));
}

#[test]
pub fn case_mono_pcm_rejects_invalid_buffers() {
    assert!(matches!(
        MonoPcmF32::borrowed(0, &[0.0]),
        Err(AudioInputError::InvalidSampleRate)
    ));
    assert!(matches!(
        MonoPcmF32::borrowed(12_000, &[]),
        Err(AudioInputError::EmptyInput)
    ));
}

#[test]
pub fn case_audio_response_resampler_preserves_12khz_mono() {
    let mut resampler = AudioResponseResampler::new();
    let input = vec![0.5f32; 200];
    let input = MonoPcmF32::borrowed(12_000, &input).expect("valid mono input");

    let blocks = resampler.push(&input);

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].sample_rate(), 12_000);
    assert_eq!(blocks[0].frame_count(), 200);
    assert_eq!(blocks[0].samples(), &[0.5f32; 200]);
    assert!(resampler.pending_mono_for_testing().is_empty());
}

#[test]
pub fn case_audio_response_resampler_converts_48khz_mono_to_12khz() {
    let mut resampler = AudioResponseResampler::new();
    #[allow(clippy::cast_precision_loss)]
    let input = (0..800).map(|frame| frame as f32).collect::<Vec<_>>();
    let input = MonoPcmF32::borrowed(48_000, &input).expect("valid mono input");

    let blocks = resampler.push(&input);

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].sample_rate(), 12_000);
    assert_eq!(blocks[0].frame_count(), 200);
    assert_eq!(blocks[0].samples().len(), 200);
    assert!(blocks[0].samples()[0] <= blocks[0].samples()[1]);
}

#[test]
pub fn case_audio_response_resampler_buffers_partial_blocks() {
    let mut resampler = AudioResponseResampler::new();
    let first = vec![1.0f32; 100];
    let second = vec![1.0f32; 100];

    let first = MonoPcmF32::borrowed(12_000, &first).expect("valid first chunk");
    let blocks = resampler.push(&first);
    assert!(blocks.is_empty());

    let second = MonoPcmF32::borrowed(12_000, &second).expect("valid second chunk");
    let blocks = resampler.push(&second);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].frame_count(), 200);
    assert!(resampler.pending_mono_for_testing().is_empty());
}

#[test]
pub fn case_interleaved_stereo_fallback_downmixes_to_mono() {
    let input =
        InterleavedStereoF32::new(12_000, &[1.0, 0.0, 0.25, 0.75]).expect("valid stereo input");

    let mono = MonoPcmF32::from_interleaved_stereo(&input);

    assert_eq!(mono.sample_rate(), 12_000);
    assert_eq!(mono.samples(), &[0.5, 0.5]);
}

#[test]
pub fn case_platform_capture_uses_mono_global_tap() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(manifest_dir.join("src/media/audio/capture.rs"))
        .expect("capture source should be readable");

    assert!(source.contains("initMonoGlobalTapButExcludeProcesses"));
    assert!(!source.contains("initStereoGlobalTapButExcludeProcesses"));
}

#[test]
pub fn case_platform_audio_backend_constructs_or_reports_unsupported() {
    match PlatformAudioCaptureBackend::new() {
        Ok(backend) => assert!(!backend.is_running()),
        Err(AudioCaptureError::UnsupportedPlatform) => {}
        Err(error) => panic!("unexpected platform backend error: {error}"),
    }
}

#[derive(Default)]
struct FakeEngine {
    frames: Mutex<usize>,
}

impl AudioFrameConsumer for FakeEngine {
    fn submit_audio_frames(
        &self,
        frames: InterleavedStereoF32<'_>,
    ) -> Result<(), AudioCaptureError> {
        *self.frames.lock().expect("frames lock should be valid") += frames.frame_count() as usize;
        Ok(())
    }
}

impl AudioResponseEngine for FakeEngine {
    fn set_audio_response_enabled(
        &self,
        _handle: SceneHandle,
        _enabled: bool,
    ) -> Result<(), AudioCaptureError> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeBackend {
    running: bool,
}

impl AudioCaptureBackend for FakeBackend {
    fn has_permission(&self) -> Result<bool, AudioCaptureError> {
        Ok(true)
    }

    fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
        Ok(true)
    }

    fn start(&mut self, _consumer: Arc<dyn AudioFrameConsumer>) -> Result<(), AudioCaptureError> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioCaptureError> {
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }
}

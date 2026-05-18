use std::sync::{Arc, Mutex};

use wallpaper_core::{
    media::audio::{
        AudioCaptureBackend, AudioCaptureError, AudioFrameConsumer, AudioResponseController,
        AudioResponseEngine, InterleavedStereoF32,
    },
    project::SceneHandle,
};

#[derive(Default)]
struct FakeEngine {
    toggles: Mutex<Vec<(SceneHandle, bool)>>,
    submitted_frames: Mutex<Vec<(u32, u32)>>,
}

impl AudioFrameConsumer for FakeEngine {
    fn submit_audio_frames(
        &self,
        frames: InterleavedStereoF32<'_>,
    ) -> Result<(), AudioCaptureError> {
        self.submitted_frames
            .lock()
            .unwrap()
            .push((frames.sample_rate(), frames.frame_count()));
        Ok(())
    }
}

impl AudioResponseEngine for FakeEngine {
    fn set_audio_response_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), AudioCaptureError> {
        self.toggles.lock().unwrap().push((handle, enabled));
        Ok(())
    }
}

#[derive(Default)]
struct FakeBackend {
    permission_granted: bool,
    running: bool,
    start_count: usize,
    stop_count: usize,
    request_count: usize,
}

impl AudioCaptureBackend for FakeBackend {
    fn has_permission(&self) -> Result<bool, AudioCaptureError> {
        Ok(self.permission_granted)
    }

    fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
        self.request_count += 1;
        self.permission_granted = true;
        Ok(true)
    }

    fn start(&mut self, _consumer: Arc<dyn AudioFrameConsumer>) -> Result<(), AudioCaptureError> {
        self.start_count += 1;
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioCaptureError> {
        self.stop_count += 1;
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }
}

#[test]
fn enabling_first_scene_starts_capture_when_permission_exists() {
    let engine = Arc::new(FakeEngine::default());
    let backend = FakeBackend {
        permission_granted: true,
        ..FakeBackend::default()
    };
    let mut controller = AudioResponseController::new(engine.clone(), backend);
    let handle = SceneHandle::new(7);

    controller.set_scene_enabled(handle, true).unwrap();

    assert_eq!(engine.toggles.lock().unwrap().as_slice(), &[(handle, true)]);
    assert!(controller.is_capturing());
    assert_eq!(controller.backend().start_count, 1);
}

#[test]
fn disabling_last_scene_stops_capture() {
    let engine = Arc::new(FakeEngine::default());
    let backend = FakeBackend {
        permission_granted: true,
        ..FakeBackend::default()
    };
    let mut controller = AudioResponseController::new(engine, backend);
    let handle = SceneHandle::new(9);

    controller.set_scene_enabled(handle, true).unwrap();
    controller.set_scene_enabled(handle, false).unwrap();

    assert!(!controller.is_capturing());
    assert_eq!(controller.backend().stop_count, 1);
}

#[test]
fn requesting_permission_starts_capture_for_pending_scene() {
    let engine = Arc::new(FakeEngine::default());
    let backend = FakeBackend::default();
    let mut controller = AudioResponseController::new(engine, backend);

    controller
        .set_scene_enabled(SceneHandle::new(11), true)
        .unwrap();
    assert!(!controller.is_capturing());

    assert!(controller.request_permission().unwrap());
    assert!(controller.is_capturing());
    assert_eq!(controller.backend().request_count, 1);
    assert_eq!(controller.backend().start_count, 1);
}

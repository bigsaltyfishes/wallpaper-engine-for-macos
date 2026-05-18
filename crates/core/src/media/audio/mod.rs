mod capture;

use std::{
    borrow::Cow,
    collections::HashSet,
    error::Error,
    fmt::{Display, Formatter},
    sync::Arc,
};

pub use capture::{DefaultAudioResponseController, PlatformAudioCaptureBackend};

use crate::project::SceneHandle;

/// Safe audio volume wrapper
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioVolume(f32);

impl TryFrom<f32> for AudioVolume {
    type Error = AudioInputError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(AudioInputError::InvalidAudioVolume(value));
        }
        Ok(Self(value))
    }
}

impl From<AudioVolume> for f32 {
    fn from(value: AudioVolume) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterleavedStereoF32<'a> {
    sample_rate: u32,
    samples: &'a [f32],
}

impl<'a> InterleavedStereoF32<'a> {
    /// Creates an interleaved stereo PCM view.
    ///
    /// # Errors
    ///
    /// Returns [`AudioInputError::InvalidSampleRate`] when `sample_rate` is
    /// zero, [`AudioInputError::EmptyInput`] when `samples` is empty, or
    /// [`AudioInputError::OddSampleCount`] when the buffer does not contain
    /// complete left/right sample pairs.
    pub fn new(sample_rate: u32, samples: &'a [f32]) -> Result<Self, AudioInputError> {
        if sample_rate == 0 {
            return Err(AudioInputError::InvalidSampleRate);
        }
        if samples.is_empty() {
            return Err(AudioInputError::EmptyInput);
        }
        if !samples.len().is_multiple_of(2) {
            return Err(AudioInputError::OddSampleCount(samples.len()));
        }

        Ok(Self {
            sample_rate,
            samples,
        })
    }

    #[must_use]
    pub fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn frame_count(self) -> u32 {
        u32::try_from(self.samples.len() / 2).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn samples(self) -> &'a [f32] {
        self.samples
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonoPcmF32<'a> {
    sample_rate: u32,
    samples: Cow<'a, [f32]>,
}

impl<'a> MonoPcmF32<'a> {
    /// Creates a borrowed mono PCM view.
    ///
    /// # Errors
    ///
    /// Returns [`AudioInputError::InvalidSampleRate`] when `sample_rate` is
    /// zero or [`AudioInputError::EmptyInput`] when `samples` is empty.
    pub fn borrowed(sample_rate: u32, samples: &'a [f32]) -> Result<Self, AudioInputError> {
        if sample_rate == 0 {
            return Err(AudioInputError::InvalidSampleRate);
        }
        if samples.is_empty() {
            return Err(AudioInputError::EmptyInput);
        }
        Ok(Self {
            sample_rate,
            samples: Cow::Borrowed(samples),
        })
    }

    /// Creates an owned mono PCM buffer.
    ///
    /// # Errors
    ///
    /// Returns [`AudioInputError::InvalidSampleRate`] when `sample_rate` is
    /// zero or [`AudioInputError::EmptyInput`] when `samples` is empty.
    pub fn owned(sample_rate: u32, samples: Vec<f32>) -> Result<Self, AudioInputError> {
        if sample_rate == 0 {
            return Err(AudioInputError::InvalidSampleRate);
        }
        if samples.is_empty() {
            return Err(AudioInputError::EmptyInput);
        }
        Ok(Self {
            sample_rate,
            samples: Cow::Owned(samples),
        })
    }

    #[must_use]
    pub fn from_interleaved_stereo(frames: &InterleavedStereoF32<'_>) -> MonoPcmF32<'static> {
        let samples = frames
            .samples()
            .chunks_exact(2)
            .map(|pair| 0.5 * (pair[0] + pair[1]))
            .collect::<Vec<_>>();
        MonoPcmF32 {
            sample_rate: frames.sample_rate(),
            samples: Cow::Owned(samples),
        }
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn frame_count(&self) -> u32 {
        u32::try_from(self.samples.len()).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
}

#[derive(Debug)]
pub struct AudioResponseResampler {
    pending_mono: Vec<f32>,
    source_position: f64,
    previous_mono: Option<f32>,
}

impl Default for AudioResponseResampler {
    fn default() -> Self {
        Self {
            pending_mono: Vec::new(),
            source_position: 0.0,
            previous_mono: None,
        }
    }
}

impl AudioResponseResampler {
    pub const TARGET_SAMPLE_RATE: u32 = 12_000;
    pub const BLOCK_FRAMES: usize = 200;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends mono PCM and returns any complete fixed-size response blocks.
    ///
    /// # Panics
    ///
    /// Panics only if the fixed-size block emitted internally is rejected as an
    /// invalid mono PCM buffer, which would indicate a broken resampler
    /// invariant.
    #[must_use]
    pub fn push(&mut self, frames: &MonoPcmF32<'_>) -> Vec<MonoPcmF32<'static>> {
        if frames.sample_rate() == Self::TARGET_SAMPLE_RATE {
            self.pending_mono.extend_from_slice(frames.samples());
        } else {
            self.append_resampled(frames.sample_rate(), frames.samples());
        }

        let mut blocks = Vec::new();
        while self.pending_mono.len() >= Self::BLOCK_FRAMES {
            let samples = self
                .pending_mono
                .drain(..Self::BLOCK_FRAMES)
                .collect::<Vec<_>>();
            blocks.push(
                MonoPcmF32::owned(Self::TARGET_SAMPLE_RATE, samples)
                    .expect("resampler emits non-empty fixed-size blocks"),
            );
        }
        blocks
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn append_resampled(&mut self, source_sample_rate: u32, mono: &[f32]) {
        let step = f64::from(source_sample_rate) / f64::from(Self::TARGET_SAMPLE_RATE);
        let mut extended =
            Vec::with_capacity(mono.len() + usize::from(self.previous_mono.is_some()));
        if let Some(previous) = self.previous_mono {
            extended.push(previous);
        }
        extended.extend_from_slice(mono);

        while self.source_position + 1.0 < extended.len() as f64 {
            let index = self.source_position.floor() as usize;
            let fraction = (self.source_position - index as f64) as f32;
            let current = extended[index];
            let next = extended[index + 1];
            self.pending_mono
                .push(current + ((next - current) * fraction));
            self.source_position += step;
        }

        if !extended.is_empty() {
            self.previous_mono = extended.last().copied();
            self.source_position = (self.source_position - (extended.len() - 1) as f64).max(0.0);
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn pending_mono_for_testing(&self) -> &[f32] {
        &self.pending_mono
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioInputError {
    InvalidSampleRate,
    EmptyInput,
    InvalidAudioVolume(f32),
    OddSampleCount(usize),
}

impl Display for AudioInputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSampleRate => write!(f, "sample_rate must be greater than zero"),
            Self::EmptyInput => write!(f, "samples must not be empty"),
            Self::OddSampleCount(count) => {
                write!(f, "samples must contain stereo pairs, got {count} values")
            }
            Self::InvalidAudioVolume(volume) => {
                write!(f, "audio volume must be between 0.0 and 1.0, got {volume}")
            }
        }
    }
}

impl Error for AudioInputError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioCaptureError {
    UnsupportedPlatform,
    PermissionDenied(String),
    Platform(String),
    Engine(String),
}

impl Display for AudioCaptureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(f, "system audio capture is not supported on this platform")
            }
            Self::PermissionDenied(message) | Self::Platform(message) | Self::Engine(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl Error for AudioCaptureError {}

pub trait AudioFrameConsumer: Send + Sync {
    /// Accepts interleaved stereo `float32` PCM in LRLR order.
    /// The sample rate is supplied by the caller and is not resampled by this
    /// API.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the consumer cannot forward or
    /// process the supplied frames.
    fn submit_audio_frames(
        &self,
        frames: InterleavedStereoF32<'_>,
    ) -> Result<(), AudioCaptureError>;

    /// Accepts mono `float32` PCM.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the consumer cannot forward or
    /// process the supplied frames.
    ///
    /// # Panics
    ///
    /// Panics if duplicating a validated mono buffer into stereo somehow
    /// creates an invalid stereo buffer.
    fn submit_mono_audio_frames(&self, frames: MonoPcmF32<'_>) -> Result<(), AudioCaptureError> {
        let mut stereo = Vec::with_capacity(frames.samples().len() * 2);
        for sample in frames.samples() {
            stereo.push(*sample);
            stereo.push(*sample);
        }
        let frames = InterleavedStereoF32::new(frames.sample_rate(), &stereo)
            .expect("duplicated mono frames should be valid stereo");
        self.submit_audio_frames(frames)
    }
}

pub trait AudioResponseEngine: AudioFrameConsumer {
    /// Enables or disables audio response for one scene.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the engine cannot update the scene.
    fn set_audio_response_enabled(
        &self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), AudioCaptureError>;
}

pub trait AudioCaptureBackend {
    /// Returns whether system audio capture permission is currently available.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the platform permission check fails.
    fn has_permission(&self) -> Result<bool, AudioCaptureError>;
    /// Requests system audio capture permission.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the platform permission prompt or
    /// status check fails.
    fn request_permission(&mut self) -> Result<bool, AudioCaptureError>;
    /// Starts audio capture and forwards captured frames to `consumer`.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when platform capture setup fails.
    fn start(&mut self, consumer: Arc<dyn AudioFrameConsumer>) -> Result<(), AudioCaptureError>;
    /// Stops audio capture.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when platform teardown fails.
    fn stop(&mut self) -> Result<(), AudioCaptureError>;
    #[must_use]
    fn is_running(&self) -> bool;
}

pub struct AudioCaptureController<B: AudioCaptureBackend> {
    consumer: Arc<dyn AudioFrameConsumer>,
    backend: B,
    enabled_handles: HashSet<SceneHandle>,
}

impl<B: AudioCaptureBackend> AudioCaptureController<B> {
    #[must_use]
    pub fn new(consumer: Arc<dyn AudioFrameConsumer>, backend: B) -> Self {
        Self {
            consumer,
            backend,
            enabled_handles: HashSet::new(),
        }
    }

    /// Returns whether the backend currently has capture permission.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the backend permission check fails.
    pub fn has_permission(&self) -> Result<bool, AudioCaptureError> {
        self.backend.has_permission()
    }

    /// Requests capture permission and starts capture if scenes are already
    /// enabled.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when permission or capture startup fails.
    pub fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
        let granted = self.backend.request_permission()?;
        if granted {
            self.sync_capture_state()?;
        }
        Ok(granted)
    }

    /// Enables or disables capture for one scene.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when synchronizing backend capture state
    /// fails.
    pub fn set_scene_capturing(
        &mut self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), AudioCaptureError> {
        if enabled {
            self.enabled_handles.insert(handle);
        } else {
            self.enabled_handles.remove(&handle);
        }
        self.sync_capture_state()
    }

    #[must_use]
    pub fn is_capturing(&self) -> bool {
        self.backend.is_running()
    }

    #[must_use]
    pub fn active_scene_count(&self) -> usize {
        self.enabled_handles.len()
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn sync_capture_state(&mut self) -> Result<(), AudioCaptureError> {
        let should_run = !self.enabled_handles.is_empty();
        if should_run {
            if !self.backend.is_running() && self.backend.has_permission()? {
                self.backend.start(Arc::clone(&self.consumer))?;
            }
        } else if self.backend.is_running() {
            self.backend.stop()?;
        }
        Ok(())
    }
}

impl<B: AudioCaptureBackend> Drop for AudioCaptureController<B> {
    fn drop(&mut self) {
        if self.backend.is_running() {
            let _ = self.backend.stop();
        }
    }
}

/// macOS hosts using the built-in backend must provide
/// `NSAudioCaptureUsageDescription` in the embedding app bundle. The native
/// backend captures system output audio and forwards interleaved stereo
/// `float32` PCM while preserving the source sample rate.
pub struct AudioResponseController<E: AudioResponseEngine + 'static, B: AudioCaptureBackend> {
    engine: Arc<E>,
    backend: B,
    enabled_handles: HashSet<SceneHandle>,
}

impl<E: AudioResponseEngine + 'static, B: AudioCaptureBackend> AudioResponseController<E, B> {
    #[must_use]
    pub fn new(engine: Arc<E>, backend: B) -> Self {
        Self {
            engine,
            backend,
            enabled_handles: HashSet::new(),
        }
    }

    /// Returns whether the backend currently has capture permission.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the backend permission check fails.
    pub fn has_permission(&self) -> Result<bool, AudioCaptureError> {
        self.backend.has_permission()
    }

    /// Requests capture permission and starts capture if scenes are already
    /// enabled.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when permission or capture startup fails.
    pub fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
        let granted = self.backend.request_permission()?;
        if granted {
            self.sync_capture_state()?;
        }
        Ok(granted)
    }

    /// Enables or disables audio response for one scene and syncs capture
    /// state.
    ///
    /// # Errors
    ///
    /// Returns [`AudioCaptureError`] when the engine update or capture state
    /// sync fails.
    pub fn set_scene_enabled(
        &mut self,
        handle: SceneHandle,
        enabled: bool,
    ) -> Result<(), AudioCaptureError> {
        self.engine.set_audio_response_enabled(handle, enabled)?;
        if enabled {
            self.enabled_handles.insert(handle);
        } else {
            self.enabled_handles.remove(&handle);
        }
        self.sync_capture_state()
    }

    #[must_use]
    pub fn is_capturing(&self) -> bool {
        self.backend.is_running()
    }

    #[must_use]
    pub fn active_scene_count(&self) -> usize {
        self.enabled_handles.len()
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn sync_capture_state(&mut self) -> Result<(), AudioCaptureError> {
        let should_run = !self.enabled_handles.is_empty();
        if should_run {
            if !self.backend.is_running() && self.backend.has_permission()? {
                self.backend.start(self.engine.clone())?;
            }
        } else if self.backend.is_running() {
            self.backend.stop()?;
        }
        Ok(())
    }
}

impl<E: AudioResponseEngine + 'static, B: AudioCaptureBackend> Drop
    for AudioResponseController<E, B>
{
    fn drop(&mut self) {
        if self.backend.is_running() {
            let _ = self.backend.stop();
        }
    }
}

#[cfg(test)]
mod capture_controller_tests {
    use super::*;

    #[derive(Default)]
    struct TestConsumer;

    impl AudioFrameConsumer for TestConsumer {
        fn submit_audio_frames(
            &self,
            _frames: InterleavedStereoF32<'_>,
        ) -> Result<(), AudioCaptureError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestBackend {
        running: bool,
    }

    impl AudioCaptureBackend for TestBackend {
        fn has_permission(&self) -> Result<bool, AudioCaptureError> {
            Ok(true)
        }

        fn request_permission(&mut self) -> Result<bool, AudioCaptureError> {
            Ok(true)
        }

        fn start(
            &mut self,
            _consumer: Arc<dyn AudioFrameConsumer>,
        ) -> Result<(), AudioCaptureError> {
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

    #[test]
    fn capture_controller_tracks_enabled_handles_without_renderer_mutation() {
        let consumer = Arc::new(TestConsumer);
        let backend = TestBackend::default();
        let mut controller = AudioCaptureController::new(consumer, backend);
        let handle = SceneHandle::new(7);

        controller.set_scene_capturing(handle, true).unwrap();

        assert!(controller.is_capturing());
        assert_eq!(controller.active_scene_count(), 1);
    }
}

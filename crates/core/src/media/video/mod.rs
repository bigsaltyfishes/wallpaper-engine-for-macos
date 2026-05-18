mod decoder;
mod frame;

pub use decoder::VideoDecoder;
pub use frame::{VideoFrame, VideoFrameUploadFormat};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackClock {
    duration_seconds: f64,
    looping: bool,
    position_seconds: f64,
}

impl PlaybackClock {
    #[must_use]
    pub fn new(duration_seconds: f64, looping: bool) -> Self {
        let duration_seconds = if duration_seconds.is_finite() {
            duration_seconds.max(0.0)
        } else {
            0.0
        };
        Self {
            duration_seconds,
            looping,
            position_seconds: 0.0,
        }
    }

    pub fn advance(&mut self, delta_seconds: f64) {
        if !delta_seconds.is_finite() {
            return;
        }

        let next_position = (self.position_seconds + delta_seconds).max(0.0);
        if self.looping && self.duration_seconds > 0.0 {
            self.position_seconds = next_position % self.duration_seconds;
        } else {
            self.position_seconds = next_position.min(self.duration_seconds);
        }
    }

    #[must_use]
    pub fn position_seconds(&self) -> f64 {
        self.position_seconds
    }
}

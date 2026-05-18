pub struct VideoFrame {
    width: u32,
    height: u32,
    storage: VideoFrameStorage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoFrameUploadFormat {
    Bgra8,
}

enum VideoFrameStorage {
    Software(SoftwareVideoFrame),

    #[allow(dead_code)]
    IoSurface(IoSurfaceVideoFrame),
}

struct SoftwareVideoFrame {
    upload_format: VideoFrameUploadFormat,
    bytes_per_row: usize,
    bytes: Vec<u8>,
}

struct IoSurfaceVideoFrame {
    io_surface_id: u32,
}

impl VideoFrame {
    /// # Errors
    ///
    /// Returns an error if dimensions are zero, stride is too small, or the
    /// byte buffer is shorter than `height * bytes_per_row`.
    pub fn from_bgra_bytes(
        width: u32,
        height: u32,
        bytes_per_row: usize,
        bytes: Vec<u8>,
    ) -> Result<Self, crate::EngineError> {
        let min_bytes_per_row = width as usize * 4;
        if bytes_per_row < min_bytes_per_row {
            return Err(crate::EngineError::InvalidInput(format!(
                "video frame stride {bytes_per_row} is smaller than BGRA row size \
                 {min_bytes_per_row}"
            )));
        }

        let required_len = bytes_per_row
            .checked_mul(height as usize)
            .ok_or_else(|| crate::EngineError::InvalidInput("video frame is too large".into()))?;
        if bytes.len() < required_len {
            return Err(crate::EngineError::InvalidInput(format!(
                "video frame buffer has {} bytes but requires {required_len}",
                bytes.len()
            )));
        }

        Ok(Self {
            width,
            height,
            storage: VideoFrameStorage::Software(SoftwareVideoFrame {
                upload_format: VideoFrameUploadFormat::Bgra8,
                bytes_per_row,
                bytes,
            }),
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn upload_format(&self) -> Option<VideoFrameUploadFormat> {
        match &self.storage {
            VideoFrameStorage::Software(frame) => Some(frame.upload_format),

            VideoFrameStorage::IoSurface(_) => None,
        }
    }

    #[must_use]
    pub fn bytes_per_row(&self) -> Option<usize> {
        match &self.storage {
            VideoFrameStorage::Software(frame) => Some(frame.bytes_per_row),

            VideoFrameStorage::IoSurface(_) => None,
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.storage {
            VideoFrameStorage::Software(frame) => Some(&frame.bytes),

            VideoFrameStorage::IoSurface(_) => None,
        }
    }

    #[must_use]
    pub fn io_surface_id(&self) -> Option<u32> {
        match &self.storage {
            VideoFrameStorage::Software(_) => None,
            VideoFrameStorage::IoSurface(frame) => Some(frame.io_surface_id),
        }
    }

    #[cfg(test)]
    #[must_use]
    /// # Panics
    ///
    /// Panics if the generated test frame is invalid.
    pub fn software_for_testing(width: u32, height: u32) -> Self {
        let bytes_per_row = width as usize * 4;
        let bytes = vec![0; bytes_per_row * height as usize];
        Self::from_bgra_bytes(width, height, bytes_per_row, bytes)
            .expect("test video frame should be valid")
    }
}

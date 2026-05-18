//! `FFmpeg`-backed software video decoder.
//!
//! The decoder is used by Rust smoke coverage and by the media layer when a
//! frame needs to be uploaded through Rust-owned texture paths. Frames are
//! converted to BGRA so downstream upload code has one predictable pixel
//! format.

use std::{path::Path, sync::Once};

use ffmpeg_next as ffmpeg;

use super::VideoFrame;

/// Stateful decoder for one video file.
pub struct VideoDecoder {
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    scaler: Option<ffmpeg::software::scaling::Context>,
    eof_sent: bool,
}

impl VideoDecoder {
    /// Opens a video file and selects the best video stream.
    ///
    /// # Errors
    ///
    /// Returns an error if `FFmpeg` initialization/opening fails, no video
    /// stream exists, or decoder setup fails.
    ///
    /// # Panics
    ///
    /// Panics if global `FFmpeg` initialization fails.
    pub fn open<T: AsRef<Path>>(path: T) -> Result<Self, crate::EngineError> {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            ffmpeg::init().expect("ffmpeg should initialize");
        });
        let path = path.as_ref();
        let input = ffmpeg::format::input(path).map_err(|error| {
            crate::EngineError::Platform(format!("failed to open video: {error}"))
        })?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| {
                crate::EngineError::InvalidInput("video stream not found".to_string())
            })?;
        let stream_index = stream.index();
        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|error| {
                crate::EngineError::Platform(format!(
                    "failed to read video codec parameters: {error}"
                ))
            })?;
        let decoder = context.decoder().video().map_err(|error| {
            crate::EngineError::Platform(format!("failed to open video decoder: {error}"))
        })?;

        Ok(Self {
            input,
            stream_index,
            decoder,
            scaler: None,
            eof_sent: false,
        })
    }

    /// Decodes and returns the next BGRA upload frame.
    ///
    /// Returns `Ok(None)` after the decoder reaches EOF and all buffered frames
    /// have been drained.
    ///
    /// # Errors
    ///
    /// Returns an error if `FFmpeg` decoding or frame conversion fails.
    pub fn decode_next(&mut self) -> Result<Option<VideoFrame>, crate::EngineError> {
        loop {
            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => return self.convert_to_upload_frame(&decoded).map(Some),
                Err(ffmpeg::Error::Eof) if self.eof_sent => return Ok(None),
                Err(ffmpeg::Error::Other { errno }) if errno == libc::EAGAIN => {}
                Err(error) => {
                    return Err(crate::EngineError::Platform(format!(
                        "failed to decode video frame: {error}"
                    )));
                }
            }

            if self.eof_sent {
                return Ok(None);
            }

            let packet = {
                let mut packets = self.input.packets();
                packets.next()
            };
            match packet {
                Some((stream, packet)) if stream.index() == self.stream_index => {
                    self.decoder.send_packet(&packet).map_err(|error| {
                        crate::EngineError::Platform(format!(
                            "failed to send video packet to decoder: {error}"
                        ))
                    })?;
                }
                Some(_) => {}
                None => {
                    self.decoder.send_eof().map_err(|error| {
                        crate::EngineError::Platform(format!(
                            "failed to flush video decoder: {error}"
                        ))
                    })?;
                    self.eof_sent = true;
                }
            }
        }
    }

    fn convert_to_upload_frame(
        &mut self,
        decoded: &ffmpeg::util::frame::video::Video,
    ) -> Result<VideoFrame, crate::EngineError> {
        let width = decoded.width();
        let height = decoded.height();
        let format = decoded.format();

        let rebuild_scaler = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.input();
            let output = scaler.output();
            input.format != format
                || input.width != width
                || input.height != height
                || output.format != ffmpeg::format::Pixel::BGRA
                || output.width != width
                || output.height != height
        });

        if rebuild_scaler {
            self.scaler = Some(
                ffmpeg::software::scaling::Context::get(
                    format,
                    width,
                    height,
                    ffmpeg::format::Pixel::BGRA,
                    width,
                    height,
                    ffmpeg::software::scaling::Flags::BILINEAR,
                )
                .map_err(|error| {
                    crate::EngineError::Platform(format!(
                        "failed to create video frame converter: {error}"
                    ))
                })?,
            );
        }

        let scaler = self
            .scaler
            .as_mut()
            .expect("video scaler should exist after initialization");
        let mut output = ffmpeg::util::frame::video::Video::empty();
        scaler.run(decoded, &mut output).map_err(|error| {
            crate::EngineError::Platform(format!("failed to convert video frame: {error}"))
        })?;

        VideoFrame::from_bgra_bytes(
            output.width(),
            output.height(),
            output.stride(0),
            output.data(0).to_vec(),
        )
    }
}

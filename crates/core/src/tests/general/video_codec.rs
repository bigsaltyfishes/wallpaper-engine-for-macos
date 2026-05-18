use crate::media::video::{VideoDecoder, VideoFrame, VideoFrameUploadFormat};

const GENERATED_PPM: &[u8] = b"P6\n2 2\n255\n\
    \xff\0\0\
    \0\xff\0\
    \0\0\xff\
    \xff\xff\xff";

#[test]
pub fn case_video_frame_exposes_dimensions() {
    let frame = VideoFrame::software_for_testing(64, 32);
    assert_eq!(frame.width(), 64);
    assert_eq!(frame.height(), 32);
}

#[test]
pub fn case_video_frame_exposes_software_upload() {
    let frame = VideoFrame::software_for_testing(64, 32);
    assert_eq!(frame.upload_format(), Some(VideoFrameUploadFormat::Bgra8));
    assert_eq!(frame.bytes_per_row(), Some(64 * 4));
    assert_eq!(frame.as_bytes().map(<[u8]>::len), Some(64 * 32 * 4));

    assert_eq!(frame.io_surface_id(), None);
}

#[test]
pub fn case_video_decoder_decodes_generated_fixture() -> Result<(), crate::EngineError> {
    let temp = tempfile::tempdir().map_err(|error| {
        crate::EngineError::Platform(format!("failed to create video fixture tempdir: {error}"))
    })?;
    let fixture = temp.path().join("video-codec.ppm");
    std::fs::write(&fixture, GENERATED_PPM).map_err(|error| {
        crate::EngineError::Platform(format!("failed to write video fixture: {error}"))
    })?;

    let mut decoder = VideoDecoder::open(fixture)?;
    let frame = decoder
        .decode_next()?
        .expect("generated fixture should contain one video frame");

    assert_eq!(frame.width(), 2);
    assert_eq!(frame.height(), 2);
    assert_eq!(frame.upload_format(), Some(VideoFrameUploadFormat::Bgra8));

    let bytes_per_row = frame
        .bytes_per_row()
        .expect("software decoded frame should expose row stride");
    assert!(bytes_per_row >= frame.width() as usize * 4);
    assert!(
        frame
            .as_bytes()
            .expect("software decoded frame should expose upload bytes")
            .len()
            >= bytes_per_row * frame.height() as usize
    );
    assert!(
        frame
            .as_bytes()
            .expect("software decoded frame should expose upload bytes")
            .iter()
            .any(|sample| *sample != 0)
    );

    Ok(())
}

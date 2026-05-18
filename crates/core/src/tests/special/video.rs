#[test]
pub fn case_video_playback_clock() {
    let mut clock = crate::media::video::PlaybackClock::new(10.0, true);
    clock.advance(11.0);
    assert!(clock.position_seconds() < 10.0);
}

//! A file whose demuxer is not installed. GStreamer's WAV reader is taken
//! out of this test process's plugin registry (rank none: never picked), so
//! a WAV meets the same path a file with an uninstalled codec does. Its own
//! test binary, because the change is process-wide.

use std::time::{Duration, Instant};

use files_preview::media::{AudioOut, MediaEvent, Player};
use gstreamer as gst;
use gstreamer::prelude::*;

#[test]
fn a_missing_codec_is_reported_by_name() {
    gst::init().unwrap();
    let wavparse = gst::ElementFactory::find("wavparse").expect("the good plugins are installed");
    wavparse.set_rank(gst::Rank::NONE);

    let folder = tempfile::tempdir().unwrap();
    let wav = folder.path().join("tone.wav");
    // A valid WAV header and a tenth of a second of silence.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36u32 + 1600).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&1600u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 1600]);
    std::fs::write(&wav, bytes).unwrap();

    let mut player = Player::open(&wav, AudioOut::Discard).unwrap();
    let started = Instant::now();
    let mut events = Vec::new();
    while started.elapsed() < Duration::from_secs(10)
        && !events
            .iter()
            .any(|event| matches!(event, MediaEvent::Failed(_)))
    {
        events.extend(player.poll());
        std::thread::sleep(Duration::from_millis(20));
    }
    let missing = events
        .iter()
        .find_map(|event| match event {
            MediaEvent::MissingCodec(names) => Some(names.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no missing codec in {events:?}"));
    assert!(
        missing
            .iter()
            .any(|name| name.to_lowercase().contains("wav")),
        "{missing:?}"
    );
}

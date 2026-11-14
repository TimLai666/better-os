//! The preview's player against real GStreamer, with files made here: a WAV
//! written byte by byte and a short AVI encoded by GStreamer itself. Sound
//! goes to a fake sink, never to the speakers. Needs GStreamer's base and
//! good plugins (CI installs them).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use files_preview::media::{AudioOut, MediaEvent, Player, clock};
use gstreamer as gst;
use gstreamer::prelude::*;

/// A mono 16-bit WAV of `seconds` of a quiet square wave.
fn write_wav(path: &Path, seconds: f32) {
    let rate = 8_000u32;
    let samples = (rate as f32 * seconds) as u32;
    let mut data = Vec::new();
    for index in 0..samples {
        let value: i16 = if index / 20 % 2 == 0 { 2_000 } else { -2_000 };
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(path, wav).unwrap();
}

/// A one-second 160x120 AVI (motion JPEG), encoded by GStreamer.
fn write_avi(path: &Path) {
    gst::init().unwrap();
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc num-buffers=10 ! video/x-raw,width=160,height=120,framerate=10/1 \
         ! jpegenc ! avimux ! filesink location={}",
        path.display()
    ))
    .expect("GStreamer's base and good plugins are installed");
    pipeline.set_state(gst::State::Playing).unwrap();
    let bus = pipeline.bus().unwrap();
    for message in bus.iter_timed(gst::ClockTime::from_seconds(20)) {
        match message.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Error(error) => panic!("encoding failed: {}", error.error()),
            _ => {}
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();
}

/// Polls until `done` says so or ten seconds pass; returns every event.
fn run_until(
    player: &mut Player,
    mut done: impl FnMut(&Player, &[MediaEvent]) -> bool,
) -> Vec<MediaEvent> {
    let started = Instant::now();
    let mut events = Vec::new();
    while started.elapsed() < Duration::from_secs(10) {
        events.extend(player.poll());
        if done(player, &events) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    events
}

static FOLDER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Points GStreamer's plugin registry (and any other cache) at a folder of
/// this test process's own, so running the tests never writes to the
/// person's `~/.cache/gstreamer-1.0`, and removes that folder when the
/// tests are done. Runs once, before GStreamer starts.
fn isolate() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let folder = FOLDER
            .get_or_init(|| {
                tempfile::Builder::new()
                    .prefix("better-files-gstreamer-test-")
                    .tempdir()
                    .expect("a temporary folder")
                    .keep()
            })
            .clone();
        // Removed when the test process exits.
        extern "C" fn remove_folder() {
            if let Some(folder) = FOLDER.get() {
                let _ = std::fs::remove_dir_all(folder);
            }
        }
        // SAFETY: registers a plain function with no arguments, once.
        unsafe {
            libc::atexit(remove_folder);
        }
        // SAFETY: every test calls this before anything else, `Once` makes
        // the others wait until it returns, and nothing in these tests reads
        // the environment from another thread meanwhile.
        unsafe {
            std::env::set_var("GST_REGISTRY", folder.join("registry.bin"));
            std::env::set_var("XDG_CACHE_HOME", &folder);
        }
    });
}

fn scratch(name: &str) -> (tempfile::TempDir, PathBuf) {
    isolate();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join(name);
    (folder, path)
}

#[test]
fn a_wav_plays_to_its_end_and_knows_its_length() {
    let (_folder, wav) = scratch("tone.wav");
    write_wav(&wav, 0.6);
    let mut player = Player::open(&wav, AudioOut::Discard, (1920, 1080)).unwrap();
    run_until(&mut player, |player, _| player.duration().is_some());
    let length = player.duration().expect("the length is known once paused");
    assert!(
        (550..=650).contains(&length.as_millis()),
        "length {length:?}"
    );
    assert!(!player.is_playing());

    player.play();
    assert!(player.is_playing());
    let events = run_until(&mut player, |_, events| {
        events.contains(&MediaEvent::Finished)
    });
    assert!(events.contains(&MediaEvent::Finished), "{events:?}");
    assert!(!player.is_playing(), "the end leaves it paused");
    // No video in a WAV, so no frame.
    assert!(player.take_frame().is_none());
}

#[test]
fn pause_holds_the_position_and_seek_moves_it() {
    let (_folder, wav) = scratch("long.wav");
    write_wav(&wav, 3.0);
    let mut player = Player::open(&wav, AudioOut::Discard, (1920, 1080)).unwrap();
    run_until(&mut player, |player, _| player.duration().is_some());

    player.seek(Duration::from_millis(2_000));
    run_until(&mut player, |player, _| {
        player
            .position()
            .is_some_and(|at| at >= Duration::from_millis(1_900))
    });
    let at = player.position().unwrap();
    assert!(
        (1_900..=2_100).contains(&at.as_millis()),
        "seek landed at {at:?}"
    );

    player.play();
    std::thread::sleep(Duration::from_millis(300));
    player.pause();
    let paused_at = player.position().unwrap();
    assert!(paused_at > at, "it played on from the seek: {paused_at:?}");
    std::thread::sleep(Duration::from_millis(300));
    let later = player.position().unwrap();
    assert!(
        later.abs_diff(paused_at) < Duration::from_millis(50),
        "paused stays put: {paused_at:?} then {later:?}"
    );

    // A seek past the end is held at the end.
    player.seek(Duration::from_secs(60));
    run_until(&mut player, |player, _| {
        player
            .position()
            .is_some_and(|at| at >= Duration::from_millis(2_500))
    });
    assert!(player.position().unwrap() <= Duration::from_millis(3_000));
}

#[test]
fn volume_is_kept_between_silent_and_as_recorded() {
    let (_folder, wav) = scratch("tone.wav");
    write_wav(&wav, 0.3);
    let mut player = Player::open(&wav, AudioOut::Discard, (1920, 1080)).unwrap();
    player.set_volume(0.25);
    assert!((player.volume() - 0.25).abs() < 1e-6);
    player.set_volume(7.0);
    assert!((player.volume() - 1.0).abs() < 1e-6);
    player.set_volume(-1.0);
    assert!(player.volume().abs() < 1e-6);
}

#[test]
fn a_video_shows_its_first_frame_paused_and_new_frames_while_playing() {
    let (_folder, avi) = scratch("clip.avi");
    write_avi(&avi);
    let mut player = Player::open(&avi, AudioOut::Discard, (1920, 1080)).unwrap();

    let mut first = None;
    run_until(&mut player, |player, _| {
        first = player.take_frame();
        first.is_some()
    });
    let first = first.expect("the paused player shows the first frame");
    assert_eq!((first.width, first.height), (160, 120));
    assert_eq!(first.bgra.len(), 160 * 120 * 4);
    // videotestsrc's bars, in BGRA: white first, then yellow (no blue).
    // JPEG shifts the levels a little, hence the margins.
    let pixel = |x: usize| &first.bgra[x * 4..x * 4 + 4];
    assert!(pixel(4).iter().all(|&level| level > 200), "{:?}", pixel(4));
    let yellow = pixel(32);
    assert!(
        yellow[0] < 60 && yellow[1] > 180 && yellow[2] > 180,
        "{yellow:?}"
    );
    assert_eq!(yellow[3], 255);

    player.play();
    let mut frames = 0;
    let events = run_until(&mut player, |player, events| {
        if player.take_frame().is_some() {
            frames += 1;
        }
        events.contains(&MediaEvent::Finished)
    });
    assert!(events.contains(&MediaEvent::Finished), "{events:?}");
    assert!(frames >= 3, "only {frames} frames arrived while playing");

    // Play after the end starts over.
    player.play();
    run_until(&mut player, |player, _| {
        player
            .position()
            .is_some_and(|at| at < Duration::from_millis(900))
    });
    assert!(player.is_playing());
}

#[test]
fn a_seek_lands_where_asked_even_between_key_frames() {
    // Three seconds of VP8 with one key frame, at the start: a seek to the
    // nearest key frame would land back at zero.
    let (_folder, webm) = scratch("sparse.webm");
    gst::init().unwrap();
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc num-buffers=30 ! video/x-raw,width=160,height=120,framerate=10/1 \
         ! vp8enc keyframe-max-dist=1000 deadline=1 ! webmmux ! filesink location={}",
        webm.display()
    ))
    .expect("GStreamer's good plugins (vpx, matroska) are installed");
    pipeline.set_state(gst::State::Playing).unwrap();
    for message in pipeline
        .bus()
        .unwrap()
        .iter_timed(gst::ClockTime::from_seconds(20))
    {
        match message.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Error(error) => panic!("encoding failed: {}", error.error()),
            _ => {}
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();

    let mut player = Player::open(&webm, AudioOut::Discard, (1920, 1080)).unwrap();
    run_until(&mut player, |player, _| player.duration().is_some());
    player.seek(Duration::from_millis(2_000));
    run_until(&mut player, |player, _| {
        player
            .position()
            .is_some_and(|at| at >= Duration::from_millis(1_500))
    });
    let at = player.position().unwrap();
    assert!(
        (1_900..=2_100).contains(&at.as_millis()),
        "seek landed at {at:?}"
    );
}

#[test]
fn a_file_that_is_not_media_fails_with_a_message() {
    let (_folder, junk) = scratch("noise.mp4");
    std::fs::write(&junk, vec![0x5a; 4096]).unwrap();
    match Player::open(&junk, AudioOut::Discard, (1920, 1080)) {
        Err(error) => assert!(!error.to_string().is_empty()),
        Ok(mut player) => {
            let events = run_until(&mut player, |_, events| {
                events
                    .iter()
                    .any(|event| matches!(event, MediaEvent::Failed(_)))
            });
            assert!(
                events.iter().any(
                    |event| matches!(event, MediaEvent::Failed(message) if !message.is_empty())
                ),
                "{events:?}"
            );
        }
    }
}

#[test]
fn clock_reads_minutes_and_hours() {
    assert_eq!(clock(Duration::from_secs(0)), "0:00");
    assert_eq!(clock(Duration::from_secs(65)), "1:05");
    assert_eq!(clock(Duration::from_secs(3_725)), "1:02:05");
}

#[test]
fn a_large_video_is_scaled_down_to_the_preview_and_a_small_one_is_left_alone() {
    let (_folder, avi) = scratch("clip.avi");
    write_avi(&avi);
    let first_frame = |max: (u32, u32)| {
        let mut player = Player::open(&avi, AudioOut::Discard, max).unwrap();
        let mut frame = None;
        run_until(&mut player, |player, _| {
            frame = player.take_frame();
            frame.is_some()
        });
        let frame = frame.expect("a first frame");
        (frame.width, frame.height)
    };
    // 160x120 into an 80-pixel-wide preview: halved, aspect kept.
    assert_eq!(first_frame((80, 600)), (80, 60));
    // Never scaled up.
    assert_eq!(first_frame((1920, 1080)), (160, 120));
}

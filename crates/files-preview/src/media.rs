//! Audio and video playback for the large preview, through GStreamer.
//!
//! `playbin` reads and decodes the file with whatever GStreamer plugins are
//! installed. Video frames arrive at an `appsink` as tightly packed BGRA,
//! which is what a GPUI image holds, and the newest one waits in [`Player`]
//! until the window takes it. Nothing here touches GPUI.
//!
//! A file whose codec is not installed is not an error to hide: GStreamer
//! names the missing piece, and [`Player::poll`] reports it as
//! [`MediaEvent::MissingCodec`] so the window can say so in plain words.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

/// One decoded video frame: `width * height * 4` bytes of BGRA, row after
/// row with no padding.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Where the sound goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioOut {
    /// The desktop's sound output.
    Speakers,
    /// Nowhere, at the speed it would play: for tests.
    Discard,
}

/// What [`Player::poll`] found since it was last called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediaEvent {
    /// Playback reached the end. The player is paused there.
    Finished,
    /// A decoder or demuxer this file needs is not installed. Carries the
    /// names GStreamer gave for what is missing (it can be empty when
    /// GStreamer gave none). Playback may go on without that stream (a
    /// video without its sound), or stop with [`MediaEvent::Failed`].
    MissingCodec(Vec<String>),
    /// The file could not be played. Carries GStreamer's message.
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("GStreamer is not available: {0}")]
    Unavailable(String),
    #[error("{0}")]
    Pipeline(String),
}

pub struct Player {
    playbin: gst::Element,
    frame: Arc<Mutex<Option<Frame>>>,
    playing: bool,
    finished: bool,
    missing: Vec<String>,
}

/// Starts GStreamer once per process.
fn init() -> Result<(), MediaError> {
    gst::init().map_err(|error| MediaError::Unavailable(error.to_string()))
}

impl Player {
    /// Opens `path` paused on its first frame. Playback starts with
    /// [`Player::play`].
    pub fn open(path: &Path, audio: AudioOut) -> Result<Self, MediaError> {
        init()?;
        let uri = gst::glib::filename_to_uri(path, None)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let playbin = gst::ElementFactory::make("playbin")
            .property("uri", uri.as_str())
            .build()
            .map_err(|_| MediaError::Unavailable("playbin".into()))?;

        let frame = Arc::new(Mutex::new(None));
        let caps = gst_video::VideoCapsBuilder::new()
            .format(gst_video::VideoFormat::Bgra)
            .pixel_aspect_ratio(gst::Fraction::new(1, 1))
            .build();
        let sink = gst_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(1)
            .drop(true)
            .build();
        let latest = frame.clone();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_preroll({
                    let latest = latest.clone();
                    move |sink| {
                        if let Ok(sample) = sink.pull_preroll() {
                            store(&sample, &latest);
                        }
                        Ok(gst::FlowSuccess::Ok)
                    }
                })
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    store(&sample, &latest);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        playbin.set_property("video-sink", &sink);
        if audio == AudioOut::Discard {
            let discard = gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()
                .map_err(|_| MediaError::Unavailable("fakesink".into()))?;
            playbin.set_property("audio-sink", &discard);
        }
        playbin
            .set_state(gst::State::Paused)
            .map_err(|_| MediaError::Pipeline("the file could not be opened".into()))?;
        Ok(Self {
            playbin,
            frame,
            playing: false,
            finished: false,
            missing: Vec::new(),
        })
    }

    pub fn play(&mut self) {
        if self.finished {
            self.seek(Duration::ZERO);
            self.finished = false;
        }
        if self.playbin.set_state(gst::State::Playing).is_ok() {
            self.playing = true;
        }
    }

    pub fn pause(&mut self) {
        let _ = self.playbin.set_state(gst::State::Paused);
        self.playing = false;
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Moves to `to` (clamped to the length when it is known).
    pub fn seek(&mut self, to: Duration) {
        let to = match self.duration() {
            Some(length) => to.min(length),
            None => to,
        };
        // Accurate, not to the nearest key frame: a video with key frames
        // seconds apart would otherwise jump back to the last one.
        let _ = self.playbin.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::ClockTime::from_nseconds(to.as_nanos() as u64),
        );
        self.finished = false;
    }

    /// 0.0 (silent) to 1.0 (as recorded).
    pub fn set_volume(&mut self, volume: f64) {
        self.playbin.set_property("volume", volume.clamp(0.0, 1.0));
    }

    pub fn volume(&self) -> f64 {
        self.playbin.property::<f64>("volume")
    }

    pub fn position(&self) -> Option<Duration> {
        self.playbin
            .query_position::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    pub fn duration(&self) -> Option<Duration> {
        self.playbin
            .query_duration::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    /// The newest frame not yet taken, if any arrived.
    pub fn take_frame(&self) -> Option<Frame> {
        self.frame.lock().ok()?.take()
    }

    /// What happened since the last call: the end, a missing codec, a
    /// failure. Call it every frame.
    pub fn poll(&mut self) -> Vec<MediaEvent> {
        let mut events = Vec::new();
        let Some(bus) = self.playbin.bus() else {
            return events;
        };
        while let Some(message) = bus.pop() {
            use gst::MessageView;
            match message.view() {
                MessageView::Eos(_) => {
                    let _ = self.playbin.set_state(gst::State::Paused);
                    self.playing = false;
                    self.finished = true;
                    events.push(MediaEvent::Finished);
                }
                MessageView::Element(element) => {
                    if let Some(name) = element
                        .structure()
                        .filter(|structure| structure.name() == "missing-plugin")
                        .map(missing_name)
                        && !self.missing.contains(&name)
                    {
                        self.missing.push(name);
                        events.push(MediaEvent::MissingCodec(self.missing.clone()));
                    }
                }
                MessageView::Error(error) => {
                    let _ = self.playbin.set_state(gst::State::Null);
                    self.playing = false;
                    let cause = error.error();
                    if is_missing_codec(&cause) {
                        events.push(MediaEvent::MissingCodec(self.missing.clone()));
                    }
                    events.push(MediaEvent::Failed(cause.message().to_string()));
                }
                _ => {}
            }
        }
        events
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

/// Whether GStreamer's error is about a decoder or demuxer it does not have.
pub fn is_missing_codec(error: &gst::glib::Error) -> bool {
    error.matches(gst::CoreError::MissingPlugin) || error.matches(gst::StreamError::CodecNotFound)
}

/// The readable name in a `missing-plugin` message ("WAV demuxer"), or the
/// raw detail (the media type) when it has none.
fn missing_name(structure: &gst::StructureRef) -> String {
    structure
        .get::<String>("name")
        .ok()
        .filter(|name| !name.is_empty())
        .or_else(|| structure.get::<String>("detail").ok())
        .or_else(|| {
            structure
                .get::<gst::Caps>("detail")
                .ok()
                .and_then(|caps| caps.structure(0).map(|s| s.name().to_string()))
        })
        .unwrap_or_default()
}

/// Copies a sample's picture into a packed [`Frame`] and makes it the newest.
fn store(sample: &gst::Sample, latest: &Mutex<Option<Frame>>) {
    if let Some(frame) = packed(sample)
        && let Ok(mut slot) = latest.lock()
    {
        *slot = Some(frame);
    }
}

fn packed(sample: &gst::Sample) -> Option<Frame> {
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    let (width, height) = (info.width(), info.height());
    let stride = usize::try_from(*info.stride().first()?).ok()?;
    let row = width as usize * 4;
    let data = map.as_slice();
    let mut bgra = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        bgra.extend_from_slice(data.get(y * stride..y * stride + row)?);
    }
    Some(Frame {
        width,
        height,
        bgra,
    })
}

/// "1:05" or "1:02:05".
pub fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

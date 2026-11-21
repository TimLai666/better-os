//! Recording (spec R1–R8): the live outputs under an area, copied over
//! `ext-image-copy-capture-v1` as the compositor redraws them, composed into
//! the area, and encoded by GStreamer into WebM VP8 with GNOME's own VP8
//! settings (screencastService.js `swenc-memfd-vp8-vp8enc`).
//!
//! The ScreenCast portal is not used: `xdg-desktop-portal-cosmic` shows its
//! own source picker on every `SelectSources` without restore data
//! (`src/screencast.rs` 235), which would be a second dialog after Better
//! Screenshot's. It copies through this same protocol underneath.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use screenshot_core::compose::{Frame, Placed, compose};
use screenshot_core::geometry::Rect;
use screenshot_core::recording::{Ending, frame_interval, is_out_of_space};

use crate::CaptureError;
use crate::wayland::{COPY_TIMEOUT, Client};

/// The GStreamer elements a recording needs, all in `gstreamer1.0-plugins-good`
/// and `-base` on Ubuntu 22.04 and 24.04.
pub const ELEMENTS: [&str; 5] = ["appsrc", "videoconvert", "vp8enc", "webmmux", "filesink"];

/// Whether this machine can record: GStreamer starts and has every element.
pub fn supported() -> Result<(), String> {
    gst::init().map_err(|error| error.to_string())?;
    for element in ELEMENTS {
        if gst::ElementFactory::find(element).is_none() {
            return Err(format!("GStreamer has no {element}"));
        }
    }
    Ok(())
}

/// How `appsrc` takes frames: live, timestamped by the recorder, and holding
/// at most [`QUEUED_FRAMES`] frames; when the encoder falls behind, the oldest
/// waiting frame is dropped rather than kept (`leaky-type=downstream`,
/// GStreamer 1.20 and later, the version on Ubuntu 22.04). GNOME's
/// `pipewiresrc` has a fixed number of buffers and so drops frames the same
/// way; without a bound, a 2880 × 1800 output at 30 frames a second queues
/// about 600 MB a second the encoder cannot take.
pub const SOURCE_PROPERTIES: &str = "is-live=true format=time do-timestamp=false max-buffers=2 max-bytes=0 max-time=0 leaky-type=downstream";
/// The frames `appsrc` may hold.
pub const QUEUED_FRAMES: u32 = 2;

/// GNOME's pipeline after the source, with its thread count.
pub fn encoder_description(threads: usize) -> String {
    let threads = threads.clamp(1, 64);
    format!(
        "videoconvert chroma-mode=none dither=none matrix-mode=output-only n-threads={threads} ! \
         queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 ! \
         vp8enc cpu-used=16 max-quantizer=17 deadline=1 keyframe-mode=disabled threads={threads} static-threshold=1000 buffer-size=20000 ! \
         queue ! \
         webmmux"
    )
}

/// What to record.
#[derive(Clone, Debug)]
pub struct RecordingRequest {
    /// The area in logical pixels.
    pub area: Rect,
    pub pointer: bool,
    /// The file to write, which exists and is empty.
    pub path: std::path::PathBuf,
}

/// A recording in progress.
#[derive(Debug)]
pub struct Recording {
    stop: Arc<AtomicBool>,
    started: Instant,
    thread: Option<JoinHandle<Ending>>,
}

impl Recording {
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Whether it ended by itself (a failure).
    pub fn has_ended(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stops it, waits for the file to be finished, and says how it ended.
    pub fn stop(mut self) -> Ending {
        self.stop.store(true, Ordering::SeqCst);
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok(ending)) => ending,
            Some(Err(_)) => Ending::Failed("the recorder stopped unexpectedly".into()),
            None => Ending::Failed("the recorder was not running".into()),
        }
    }
}

/// A recording dropped without [`Recording::stop`] (a stop that came while
/// it was starting, a process that ends) still stops: the thread is told
/// and waited for, so nothing keeps copying the screen into a file nobody
/// will be told about.
impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts recording. It fails at once when the encoder cannot be built or
/// the compositor will not copy; later failures end the recording, and
/// [`Recording::stop`] reports them.
pub fn start(request: RecordingRequest) -> Result<Recording, Ending> {
    supported().map_err(Ending::FailedToStart)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), Ending>>();
    let thread = std::thread::Builder::new()
        .name("recorder".into())
        .spawn(move || run(request, flag, ready_tx))
        .map_err(|error| Ending::FailedToStart(error.to_string()))?;
    match ready_rx.recv_timeout(COPY_TIMEOUT * 2) {
        Ok(Ok(())) => Ok(Recording {
            stop,
            started: Instant::now(),
            thread: Some(thread),
        }),
        Ok(Err(ending)) => {
            let _ = thread.join();
            Err(ending)
        }
        Err(_) => {
            // Told to stop and waited for, so it cannot create the file again
            // after the caller has deleted it.
            stop.store(true, Ordering::SeqCst);
            let _ = thread.join();
            Err(Ending::FailedToStart(
                "the compositor did not give the first frame in time".into(),
            ))
        }
    }
}

struct Source {
    logical: Rect,
    job: usize,
    latest: Option<Frame>,
}

fn run(
    request: RecordingRequest,
    stop: Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<Result<(), Ending>>,
) -> Ending {
    let fail_start = |reason: String| {
        let _ = ready.send(Err(Ending::FailedToStart(reason.clone())));
        Ending::FailedToStart(reason)
    };
    let mut client = match Client::connect(false) {
        Ok(client) => client,
        Err(error) => return fail_start(error.to_string()),
    };

    // A session on every output the area touches.
    let infos = client.output_infos();
    let mut sources = Vec::new();
    for (index, info) in infos.iter().enumerate() {
        let Ok(info) = info else { continue };
        if info.logical.intersection(&request.area).is_none() {
            continue;
        }
        let Some(output) = client.state.outputs[index].output.clone() else {
            continue;
        };
        let source = client
            .globals
            .output_sources
            .create_source(&output, &client.handle, ());
        let job = client.start_job(&source, request.pointer);
        sources.push(Source {
            logical: info.logical,
            job,
            latest: None,
        });
    }
    if sources.is_empty() {
        return fail_start("the area is on no output".into());
    }

    // The first frame of each, which also fixes the picture's size.
    let jobs: Vec<usize> = sources.iter().map(|source| source.job).collect();
    if let Err(error) = client.run_jobs(&jobs) {
        return fail_start(error.to_string());
    }
    for source in &mut sources {
        match read_and_keep(&mut client, source.job) {
            Ok(frame) => source.latest = Some(frame),
            Err(error) => return fail_start(error.to_string()),
        }
    }
    let Some(first) = compose_now(&request.area, &sources) else {
        return fail_start("the area is empty".into());
    };

    if let Err(error) = gst::init() {
        return fail_start(error.to_string());
    }
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let description = format!(
        "appsrc name=source {} ! {} ! filesink name=sink",
        SOURCE_PROPERTIES,
        encoder_description(threads)
    );
    let pipeline = match gst::parse::launch(&description)
        .map_err(|error| error.to_string())
        .and_then(|element| {
            element
                .downcast::<gst::Pipeline>()
                .map_err(|_| "not a pipeline".to_string())
        }) {
        Ok(pipeline) => pipeline,
        Err(error) => return fail_start(error),
    };
    let sink = pipeline.by_name("sink").expect("the description names it");
    sink.set_property("location", request.path.to_string_lossy().to_string());
    let appsrc = pipeline
        .by_name("source")
        .expect("the description names it")
        .downcast::<gst_app::AppSrc>()
        .expect("an appsrc");
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "BGRx")
        .field("width", first.width as i32)
        .field("height", first.height as i32)
        .field("framerate", gst::Fraction::new(0, 1))
        .build();
    appsrc.set_caps(Some(&caps));
    if let Err(error) = pipeline.set_state(gst::State::Playing) {
        return fail_start(error.to_string());
    }
    let bus = pipeline.bus().expect("a pipeline has a bus");
    let _ = ready.send(Ok(()));

    let started = Instant::now();
    let mut ending = None;
    let push = |frame: &Frame, at: Duration| -> Result<(), String> {
        let mut buffer = gst::Buffer::from_slice(frame.to_bgra());
        {
            let buffer = buffer.get_mut().expect("a new buffer");
            buffer.set_pts(gst::ClockTime::from_nseconds(at.as_nanos() as u64));
        }
        appsrc
            .push_buffer(buffer)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    };
    if let Err(error) = push(&first, Duration::ZERO) {
        ending = Some(Ending::Failed(error));
    }
    for source in &sources {
        if let Err(error) = refire(&mut client, source.job) {
            ending = Some(Ending::Failed(error.to_string()));
        }
    }

    let mut last_push = Instant::now();
    let mut changed = false;
    while ending.is_none() && !stop.load(Ordering::SeqCst) {
        // Whatever the compositor redrew.
        let tick = last_push + frame_interval();
        let deadline = tick.max(Instant::now() + Duration::from_millis(1));
        let watched: Vec<usize> = sources.iter().map(|source| source.job).collect();
        match client.dispatch_until(deadline, move |state| {
            watched.iter().any(|job| state.jobs[*job].finished())
        }) {
            Ok(()) | Err(CaptureError::Timeout) => {}
            Err(error) => {
                ending = Some(Ending::Failed(error.to_string()));
                break;
            }
        }
        for source in &mut sources {
            let job = &client.state.jobs[source.job];
            if job.stopped {
                ending = Some(Ending::Failed("the compositor stopped the copy".into()));
                break;
            }
            if let Some(reason) = &job.failed {
                ending = Some(Ending::Failed(reason.clone()));
                break;
            }
            if job.ready {
                match read_and_keep(&mut client, source.job) {
                    Ok(frame) => {
                        source.latest = Some(frame);
                        changed = true;
                    }
                    Err(error) => {
                        ending = Some(Ending::Failed(error.to_string()));
                        break;
                    }
                }
                if let Err(error) = refire(&mut client, source.job) {
                    ending = Some(Ending::Failed(error.to_string()));
                    break;
                }
            }
        }
        if changed && Instant::now() >= tick {
            if let Some(frame) = compose_now(&request.area, &sources)
                && let Err(error) = push(&frame, started.elapsed())
            {
                ending = Some(Ending::Failed(error));
            }
            last_push = Instant::now();
            changed = false;
        }
        if let Some(error) = bus_error(&bus) {
            ending = Some(error);
        }
    }

    // The last picture again at the stop time, so the video lasts as long as
    // the recording did even when the screen was still at its end.
    if ending.is_none()
        && let Some(frame) = compose_now(&request.area, &sources)
    {
        let _ = push(&frame, started.elapsed());
    }
    let _ = appsrc.end_of_stream();
    let mut ending = ending.unwrap_or(Ending::Stopped);
    // Wait for the muxer to finish the file.
    for message in bus.iter_timed(gst::ClockTime::from_seconds(10)) {
        match message.view() {
            gst::MessageView::Eos(_) => break,
            gst::MessageView::Error(error) => {
                ending = error_ending(&error.error().to_string());
                break;
            }
            _ => {}
        }
    }
    let _ = pipeline.set_state(gst::State::Null);
    for source in &sources {
        let _ = client.take_frame(source.job);
    }
    let _ = client.connection.flush();
    ending
}

fn error_ending(message: &str) -> Ending {
    if is_out_of_space(message) {
        Ending::OutOfSpace
    } else {
        Ending::Failed(message.to_string())
    }
}

fn bus_error(bus: &gst::Bus) -> Option<Ending> {
    while let Some(message) = bus.pop() {
        if let gst::MessageView::Error(error) = message.view() {
            return Some(error_ending(&format!(
                "{} ({})",
                error.error(),
                error.debug().unwrap_or_default()
            )));
        }
    }
    None
}

fn compose_now(area: &Rect, sources: &[Source]) -> Option<Frame> {
    let placed: Vec<Placed<'_>> = sources
        .iter()
        .filter_map(|source| {
            source.latest.as_ref().map(|frame| Placed {
                logical: source.logical,
                frame,
            })
        })
        .collect();
    compose(*area, &placed)
}

/// The job's pixels, keeping its session and buffer for the next frame.
fn read_and_keep(client: &mut Client, index: usize) -> Result<Frame, CaptureError> {
    client.read_job_frame(index)
}

/// Asks for the next frame into the same buffer.
fn refire(client: &mut Client, index: usize) -> Result<(), CaptureError> {
    let handle = client.handle.clone();
    let job = &mut client.state.jobs[index];
    if let Some(frame) = job.frame.take() {
        frame.destroy();
    }
    let (Some(session), Some(buffer), Some((width, height))) =
        (job.session.clone(), job.buffer.clone(), job.buffer_size)
    else {
        return Err(CaptureError::Failed("the copy has no session".into()));
    };
    job.ready = false;
    let frame = session.create_frame(&handle, index);
    frame.attach_buffer(&buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();
    job.frame = Some(frame);
    client
        .connection
        .flush()
        .map_err(|error| CaptureError::Protocol(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ELEMENTS, encoder_description};

    #[test]
    fn the_source_holds_two_frames_and_drops_the_oldest() {
        use super::{QUEUED_FRAMES, SOURCE_PROPERTIES};
        use gstreamer::prelude::*;
        assert!(SOURCE_PROPERTIES.contains("leaky-type=downstream"));
        assert!(SOURCE_PROPERTIES.contains(&format!("max-buffers={QUEUED_FRAMES}")));
        // GStreamer accepts the whole description.
        test_prerequisites::require_that!(
            "GStreamer with appsrc and vp8enc",
            gstreamer::init().is_ok()
                && gstreamer::ElementFactory::find("appsrc").is_some()
                && gstreamer::ElementFactory::find("vp8enc").is_some()
        );
        let description = format!(
            "appsrc name=source {SOURCE_PROPERTIES} ! {} ! fakesink",
            super::encoder_description(2)
        );
        let pipeline = gstreamer::parse::launch(&description).unwrap();
        let pipeline = pipeline.downcast::<gstreamer::Bin>().unwrap();
        let source = pipeline.by_name("source").unwrap();
        assert_eq!(source.property::<u64>("max-buffers"), 2);
    }

    /// Behind a consumer that takes 50 ms a frame, 40 frames pushed at once
    /// leave the source holding no more than its two: the rest are dropped,
    /// not queued.
    #[test]
    fn a_slow_encoder_makes_the_source_drop_frames_rather_than_queue_them() {
        use super::SOURCE_PROPERTIES;
        use gstreamer::prelude::*;
        test_prerequisites::require_that!(
            "GStreamer with appsrc",
            gstreamer::init().is_ok() && gstreamer::ElementFactory::find("appsrc").is_some()
        );
        let description = format!(
            "appsrc name=source {SOURCE_PROPERTIES} caps=video/x-raw,format=BGRx,width=64,height=64,framerate=30/1 \
             ! identity sleep-time=50000 ! fakesink sync=false"
        );
        let pipeline = gstreamer::parse::launch(&description)
            .unwrap()
            .downcast::<gstreamer::Pipeline>()
            .unwrap();
        let source = pipeline
            .by_name("source")
            .unwrap()
            .downcast::<gstreamer_app::AppSrc>()
            .unwrap();
        pipeline.set_state(gstreamer::State::Playing).unwrap();
        let mut most = 0u64;
        for index in 0..40u64 {
            let mut buffer = gstreamer::Buffer::with_size(64 * 64 * 4).unwrap();
            buffer
                .get_mut()
                .unwrap()
                .set_pts(gstreamer::ClockTime::from_mseconds(index * 33));
            source.push_buffer(buffer).unwrap();
            most = most.max(source.property::<u64>("current-level-buffers"));
        }
        pipeline.set_state(gstreamer::State::Null).unwrap();
        assert!(most <= 2, "the source held {most} frames");
    }

    #[test]
    fn dropping_a_recording_stops_its_thread_and_waits_for_it() {
        use super::Recording;
        use screenshot_core::recording::Ending;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (flag, done) = (stop.clone(), finished.clone());
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            done.store(true, Ordering::SeqCst);
            Ending::Stopped
        });
        let recording = Recording {
            stop,
            started: Instant::now(),
            thread: Some(thread),
        };
        drop(recording);
        assert!(
            finished.load(Ordering::SeqCst),
            "the thread was not waited for"
        );
    }

    #[test]
    fn the_encoder_is_gnomes_vp8_into_webm() {
        let description = encoder_description(4);
        assert!(description.contains("vp8enc cpu-used=16 max-quantizer=17 deadline=1"));
        assert!(description.contains("threads=4"));
        assert!(description.ends_with("webmmux"));
        assert!(encoder_description(0).contains("threads=1"));
        assert!(ELEMENTS.contains(&"vp8enc"));
    }
}

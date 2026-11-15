//! What the pass-through adds to each touchpad frame, in this process.
//!
//! Two figures: the scaling of one two-finger frame (`Scaler::frame`), and one
//! whole turn of the forwarding loop over a fake device and a fake clone —
//! frame assembly, scaling, and the hand-off to the clone, everything the
//! service does between the kernel's `read` and its `write` except the two
//! system calls. The kernel's own evdev-to-uinput hop is not in either figure;
//! the service logs its real per-read cost on the owner's hardware every
//! minute (see engineering/performance/touchpad-scroll-performance.md).
//!
//! No harness dependency, matching the other touchpad benches. Run with
//! `cargo bench -p touchpad-scroll`.

use std::time::{Duration, Instant};

use touchpad_scroll::caps::fixtures::multitouch_touchpad;
use touchpad_scroll::codes::*;
use touchpad_scroll::passthrough::fake::{FakeFactory, FakeSource, SharedJournal};
use touchpad_scroll::{Factors, InputEvent, Passthrough, Scaler};

fn frame(y: i32) -> Vec<InputEvent> {
    vec![
        InputEvent::new(EV_ABS, ABS_MT_SLOT, 0),
        InputEvent::new(EV_ABS, ABS_MT_POSITION_X, 1000),
        InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, y),
        InputEvent::new(EV_ABS, ABS_MT_SLOT, 1),
        InputEvent::new(EV_ABS, ABS_MT_POSITION_X, 1400),
        InputEvent::new(EV_ABS, ABS_MT_POSITION_Y, y),
        InputEvent::new(EV_ABS, ABS_X, 1000),
        InputEvent::new(EV_ABS, ABS_Y, y),
        InputEvent::new(EV_MSC, 5, y * 7000),
        InputEvent::syn(),
    ]
}

fn start() -> Vec<InputEvent> {
    vec![
        InputEvent::new(EV_ABS, ABS_MT_SLOT, 0),
        InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, 1),
        InputEvent::new(EV_ABS, ABS_MT_SLOT, 1),
        InputEvent::new(EV_ABS, ABS_MT_TRACKING_ID, 2),
        InputEvent::new(EV_KEY, BTN_TOUCH, 1),
        InputEvent::new(EV_KEY, BTN_TOOL_DOUBLETAP, 1),
        InputEvent::syn(),
    ]
}

fn percentile(samples: &mut [Duration], fraction: f64) -> Duration {
    samples.sort();
    samples[((samples.len() - 1) as f64 * fraction) as usize]
}

fn report(name: &str, samples: &mut [Duration]) {
    let total: Duration = samples.iter().sum();
    let mean = total / samples.len() as u32;
    println!(
        "{name}: mean {:?}, p50 {:?}, p99 {:?}, max {:?} over {} frames",
        mean,
        percentile(samples, 0.5),
        percentile(samples, 0.99),
        percentile(samples, 1.0),
        samples.len()
    );
}

fn main() {
    let single_pass = std::env::args().any(|argument| argument == "--test");
    let frames = if single_pass { 100 } else { 200_000 };
    let factors = Factors::new(2.0, 1.5).unwrap();

    let device = multitouch_touchpad();
    let mut scaler = Scaler::new(&device);
    let mut out = Vec::with_capacity(32);
    scaler.frame(&start(), factors, &mut out);
    let mut samples = Vec::with_capacity(frames);
    for index in 0..frames {
        let input = frame(500 + (index % 1000) as i32);
        out.clear();
        let started = Instant::now();
        scaler.frame(&input, factors, &mut out);
        samples.push(started.elapsed());
        std::hint::black_box(&out);
    }
    report("scale one two-finger frame", &mut samples);

    let journal = SharedJournal::default();
    let source = FakeSource::new(device, journal.clone());
    let mut passthrough = Passthrough::new(
        source,
        FakeFactory {
            journal: journal.clone(),
        },
    );
    passthrough.step(factors, Duration::ZERO).unwrap();
    passthrough.source_mut().batches.push_back(start());
    passthrough.step(factors, Duration::ZERO).unwrap();
    let mut samples = Vec::with_capacity(frames);
    for index in 0..frames {
        passthrough
            .source_mut()
            .batches
            .push_back(frame(500 + (index % 1000) as i32));
        let started = Instant::now();
        passthrough.step(factors, Duration::ZERO).unwrap();
        samples.push(started.elapsed());
        if index % 4096 == 0 {
            journal.lock().unwrap().emitted.clear();
        }
    }
    report(
        "forward one frame through the loop (fake device and clone)",
        &mut samples,
    );
}

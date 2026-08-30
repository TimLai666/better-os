//! What recognition costs, and what the frame stream looked like while it was
//! measured.
//!
//! Issue #3 asks for gesture-recognition latency to be measured and for dropped
//! or reordered gesture frames to be counted. Both are here, and both are
//! honest about their limits: these are replayed synthetic frames, not frames
//! from a touchpad, because no backend in this build produces the latter. What
//! this measures is the cost of the decision — the part that would sit between
//! a compositor's frame and an action — and the counting is of a stream this
//! benchmark deliberately damages.
//!
//! No benchmark harness dependency, matching `touchpad-core/benches`.
//!
//! Run with `cargo bench -p touchpad-gestures`.

use std::time::{Duration, Instant};

use touchpad_gestures::{
    GestureDefinition, Recognizer, RecognizerScale, TouchFrame, mac_style, synthetic,
};

const ROUNDS: usize = 5_000;

fn main() {
    let single_pass = std::env::args().any(|argument| argument == "--test");
    let rounds = if single_pass { 1 } else { ROUNDS };

    per_frame_latency("four-finger swipe", &gesture("overview"), rounds);
    per_frame_latency("thumb and three pinch", &gesture("launcher"), rounds);
    whole_gesture(
        "four-finger swipe, begin to complete",
        &gesture("overview"),
        rounds,
    );
    frame_health();
}

fn gesture(id: &str) -> GestureDefinition {
    mac_style()
        .gestures
        .into_iter()
        .find(|gesture| gesture.id.as_str() == id)
        .expect("the preset holds this gesture")
}

/// The cost of one frame, which is the number that matters: it is what would be
/// spent per compositor event if a backend were delivering them.
fn per_frame_latency(name: &str, gesture: &GestureDefinition, rounds: usize) {
    let frames = synthetic::complete(gesture, RecognizerScale::default());
    let mut recognizer = Recognizer::new(mac_style().gestures);
    // One untimed pass so a cold allocation is not counted as the cost.
    recognizer.replay(&frames);

    let mut recognizer = Recognizer::new(mac_style().gestures);
    let started = Instant::now();
    let mut observed = 0usize;
    for round in 0..rounds {
        for frame in &frames {
            // Shift the stream forward so the cooldown never swallows a round.
            let frame = TouchFrame::new(
                frame.at_ms + (round as u64) * 10_000,
                frame.contacts.clone(),
            );
            std::hint::black_box(recognizer.observe(&frame));
            observed += 1;
        }
    }
    let elapsed = started.elapsed();
    println!(
        "{name}: {observed} frames in {elapsed:?}, {:?} per frame",
        elapsed / observed.max(1) as u32
    );
    assert!(elapsed < Duration::from_secs(60), "{name} did not finish");
}

/// Begin to complete, which is what a user would call the latency of the
/// gesture: how long the decision takes over a whole stream.
fn whole_gesture(name: &str, gesture: &GestureDefinition, rounds: usize) {
    let frames = synthetic::complete(gesture, RecognizerScale::default());
    let started = Instant::now();
    for _ in 0..rounds {
        let mut recognizer = Recognizer::new(mac_style().gestures);
        std::hint::black_box(recognizer.replay(&frames));
    }
    let elapsed = started.elapsed();
    println!(
        "{name}: {rounds} gestures in {elapsed:?}, {:?} each",
        elapsed / rounds.max(1) as u32
    );
}

/// The counting half. A clean stream, a stalled one, and a reordered one, so
/// the reported numbers are of something whose answer is known.
fn frame_health() {
    let scale = RecognizerScale::default();
    let overview = gesture("overview");

    let clean = synthetic::complete(&overview, scale);
    let mut recognizer = Recognizer::new(mac_style().gestures);
    recognizer.replay(&clean);
    println!("clean stream: {:?}", recognizer.health());
    assert!(recognizer.health().is_clean());

    let mut stalled = synthetic::perform(&overview, 1.0, scale);
    for frame in stalled.iter_mut().skip(4) {
        frame.at_ms += 250;
    }
    let mut recognizer = Recognizer::new(mac_style().gestures);
    recognizer.replay(&synthetic::lift(stalled));
    println!("stream with a 250 ms stall: {:?}", recognizer.health());
    assert!(recognizer.health().dropped > 0);

    let mut reordered = synthetic::perform(&overview, 1.0, scale);
    let ended_at = reordered.last().expect("a stream has frames").at_ms;
    let stale = reordered[2].clone();
    reordered.push(stale);
    reordered.push(TouchFrame::lifted(ended_at + 16));
    let mut recognizer = Recognizer::new(mac_style().gestures);
    recognizer.replay(&reordered);
    println!("stream with one reordered frame: {:?}", recognizer.health());
    assert_eq!(recognizer.health().reordered, 1);
}

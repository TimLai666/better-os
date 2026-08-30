//! What the decision layer costs.
//!
//! Issue #3 asks for configuration apply time to be measured. Applying has two
//! halves — deciding what to write, and writing it — and only the first one
//! lives here. The second is measured against the real dconf service by
//! `touchpad-platform`'s `apply_latency` test, because a benchmark of a D-Bus
//! round trip against a mock would measure nothing.
//!
//! No benchmark harness dependency, matching `launcher-core/benches`.
//!
//! Run with `cargo bench -p touchpad-core`.

use std::time::{Duration, Instant};

use touchpad_core::{
    Capabilities, Reading, RestoreScope, ScrollFactor, Sensitivity, SettingId, SettingValue,
    TouchpadConfig, TouchpadState,
};

const ROUNDS: usize = 2_000;

fn main() {
    let single_pass = std::env::args().any(|argument| argument == "--test");
    let rounds = if single_pass { 1 } else { ROUNDS };

    report("stage one slider move", rounds, stage_one_change);
    report("build an apply plan", rounds, build_plan);
    report("capture before the first write", rounds, capture);
    report("build a full restore plan", rounds, restore_plan);
    report("migrate a version 1 file", rounds, migrate);
    report(
        "serialize and read back a configuration",
        rounds,
        round_trip,
    );
}

fn report(name: &str, rounds: usize, mut work: impl FnMut()) {
    // One untimed pass so a cold allocation is not counted as the cost.
    work();
    let started = Instant::now();
    for _ in 0..rounds {
        work();
    }
    let elapsed = started.elapsed();
    println!(
        "{name}: {rounds} rounds in {:?}, {:?} each",
        elapsed,
        elapsed / rounds as u32
    );
    assert!(elapsed < Duration::from_secs(60), "{name} did not finish");
}

fn prepared() -> TouchpadState {
    let mut state = TouchpadState::new(
        TouchpadConfig::default(),
        Capabilities::everything_immediate(),
    );
    state.adopt_readings(
        SettingId::ALL
            .into_iter()
            .map(|setting| {
                (
                    setting,
                    Reading::value(TouchpadConfig::default().value(setting)),
                )
            })
            .collect(),
    );
    state
}

fn stage_one_change() {
    let mut state = prepared();
    state
        .stage(
            SettingId::PointerSensitivity,
            SettingValue::sensitivity(Sensitivity::new(0.72).unwrap()),
        )
        .unwrap();
    std::hint::black_box(state.has_pending());
}

fn build_plan() {
    let mut state = prepared();
    state
        .stage(
            SettingId::VerticalScrollFactor,
            SettingValue::factor(ScrollFactor::new(1.6).unwrap()),
        )
        .unwrap();
    std::hint::black_box(state.apply_plan());
}

fn capture() {
    let mut state = prepared();
    state
        .stage(
            SettingId::PointerSensitivity,
            SettingValue::sensitivity(Sensitivity::new(0.72).unwrap()),
        )
        .unwrap();
    let plan = state.apply_plan();
    std::hint::black_box(state.capture_before(&plan, "gnome", 0));
}

fn restore_plan() {
    let mut state = prepared();
    for setting in SettingId::ALL {
        let _ = state.stage(setting, TouchpadConfig::default().value(setting));
    }
    let plan = state.apply_plan();
    state.capture_before(&plan, "gnome", 0);
    std::hint::black_box(state.restore_plan(RestoreScope::All));
}

const V1: &str = r#"{
    "schema_version": 1,
    "enabled": true,
    "selected_device": "auto",
    "backend": "gnome",
    "pointer": { "sensitivity": 0.55, "acceleration_profile": "adaptive" },
    "scrolling": { "factor": 0.65, "natural": true },
    "clicking": { "tap_to_click": false }
}"#;

fn migrate() {
    std::hint::black_box(TouchpadConfig::from_json(V1).unwrap());
}

fn round_trip() {
    let config = TouchpadConfig::default();
    std::hint::black_box(TouchpadConfig::from_json(&config.to_json()).unwrap());
}

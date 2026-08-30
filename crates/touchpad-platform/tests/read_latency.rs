//! What reading the effective values costs.
//!
//! Verification is a second read, so read cost is paid on every apply and on
//! every refresh. This measures it against the recorded dconf database rather
//! than the developer's own, so the number means the same thing on every
//! machine and the test mutates nothing.
//!
//! The measured figures are recorded in `docs/touchpad-sensitivity-mapping.md`.

use std::time::Instant;

use touchpad_platform::{GnomeBackend, TouchpadBackend};

const ROUNDS: usize = 200;

fn fixture() -> GnomeBackend {
    GnomeBackend::read_only(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dconf/user"),
        None,
    )
}

#[test]
fn reading_every_setting_back_is_fast_enough_to_verify_after_every_write() {
    let backend = fixture();
    // One untimed pass so a cold page fault is not counted as the cost.
    let _ = backend.read_all();

    let started = Instant::now();
    for _ in 0..ROUNDS {
        std::hint::black_box(backend.read_all());
    }
    let each = started.elapsed() / ROUNDS as u32;
    println!("read every setting back: {each:?} per round over {ROUNDS} rounds");

    // A verifying read happens after every apply, so it has to be far below
    // anything a person notices. This bound is deliberately loose — it is a
    // regression guard, not the published figure.
    assert!(
        each < std::time::Duration::from_millis(20),
        "a full read took {each:?}, which would be felt after every change"
    );
}

#[test]
fn reading_one_setting_is_not_meaningfully_dearer_than_reading_all_of_them() {
    // The database is parsed once per read call whatever is asked for, so a
    // caller that reads settings one at a time pays for each parse. Knowing
    // that is what stops the GUI from looping over `read_one`.
    let backend = fixture();
    let _ = backend.read_all();

    let started = Instant::now();
    for _ in 0..ROUNDS {
        std::hint::black_box(backend.read_one(touchpad_core::SettingId::TapToClick));
    }
    let one = started.elapsed() / ROUNDS as u32;

    let started = Instant::now();
    for _ in 0..ROUNDS {
        std::hint::black_box(backend.read_all());
    }
    let all = started.elapsed() / ROUNDS as u32;

    println!("one setting: {one:?}, all thirteen: {all:?}");
    assert!(all < one * 4, "reading all of them should not cost 4x one");
}

//! The one test that changes a real setting on the machine it runs on.
//!
//! Everything else in this crate is proven against recorded input, because a
//! live host proves nothing repeatable. This test proves the opposite thing:
//! that the recorded input describes the real dconf service, that the change
//! set this crate encodes is one the service accepts, and that the change is
//! visible to a second read.
//!
//! It is guarded twice, and both guards are deliberate:
//!
//! - It is `#[ignore]`d, so `cargo test` never runs it.
//! - It also refuses to run unless `BETTER_TOUCHPAD_LIVE=1` is set, so
//!   `cargo test -- --ignored` on a developer's own desktop does not quietly
//!   change their touchpad either.
//!
//! Run it deliberately, inside a real GNOME session:
//!
//! ```text
//! BETTER_TOUCHPAD_LIVE=1 cargo test -p touchpad-platform --test live_apply \
//!     -- --ignored --nocapture
//! ```
//!
//! It captures the setting first, changes it, verifies it, and puts the
//! captured state back — including the case where the key held nothing, which
//! is restored by removing the key rather than by writing a value.

use std::time::Instant;

use touchpad_core::{
    ApplyPlan, ApplyStep, ClickMethod, Reading, RestorePlan, RestoreScope, RestoreStep, RunState,
    Sensitivity, SessionEffect, SettingId, SettingValue,
};
use touchpad_platform::{GnomeBackend, TouchpadBackend};

/// The three settings cover the three GVariant shapes this backend writes: a
/// boolean, a double, and an enumerated string. Drag lock and the click method
/// are not needed to operate the desktop, and pointer speed is put straight
/// back, so a failure mid-test cannot leave the machine hard to use.
const SUBJECTS: [SettingId; 3] = [
    SettingId::DragLock,
    SettingId::PointerSensitivity,
    SettingId::ClickMethod,
];

fn opted_in() -> bool {
    std::env::var("BETTER_TOUCHPAD_LIVE").as_deref() == Ok("1")
}

/// A value that is definitely not the one the setting holds now.
fn something_else(current: Option<SettingValue>) -> SettingValue {
    match current {
        Some(SettingValue::Toggle { value }) => SettingValue::toggle(!value),
        Some(SettingValue::Click { value }) => {
            SettingValue::click(if value == ClickMethod::Areas {
                ClickMethod::Fingers
            } else {
                ClickMethod::Areas
            })
        }
        Some(SettingValue::Sensitivity { value }) => SettingValue::sensitivity(
            Sensitivity::new(if value.get() > 0.5 { 0.35 } else { 0.65 }).unwrap(),
        ),
        Some(other) => other,
        None => SettingValue::toggle(true),
    }
}

fn default_for(setting: SettingId) -> SettingValue {
    match setting {
        SettingId::DragLock => SettingValue::toggle(true),
        SettingId::PointerSensitivity => SettingValue::sensitivity(Sensitivity::new(0.65).unwrap()),
        SettingId::ClickMethod => SettingValue::click(ClickMethod::Areas),
        other => panic!("{other} is not one of the live subjects"),
    }
}

#[test]
#[ignore = "changes real GNOME settings; needs BETTER_TOUCHPAD_LIVE=1 and a session bus"]
fn a_real_dconf_write_takes_effect_and_is_put_back() {
    if !opted_in() {
        panic!("set BETTER_TOUCHPAD_LIVE=1 to allow this test to change a real setting");
    }

    let mut backend = GnomeBackend::connect(None);
    assert!(
        backend.status().reachable,
        "the dconf service is not reachable: {:?}",
        backend.status()
    );

    for subject in SUBJECTS {
        assert!(
            backend.capabilities().is_available(subject),
            "{subject} is not available on this session"
        );

        // 1. Capture, before anything is written.
        let captured = backend.read_one(subject);
        println!("captured {subject} = {captured:?}");
        assert!(
            captured.is_determinate(),
            "refusing to change a setting whose current value could not be read"
        );

        // 2. Ask for something the setting does not already hold.
        let requested = match captured.as_value() {
            Some(value) => something_else(Some(value)),
            None => default_for(subject),
        };

        let started = Instant::now();
        let report = backend.apply(&ApplyPlan {
            steps: vec![ApplyStep {
                setting: subject,
                requested,
                captured: captured.clone(),
                effect: SessionEffect::Immediate,
            }],
            skipped: Vec::new(),
        });
        println!(
            "{subject}: apply and verify round trip: {:?}",
            started.elapsed()
        );

        assert_eq!(
            report.state(),
            RunState::Applied,
            "the change to {subject} did not take: {report:?}"
        );
        assert_eq!(backend.read_one(subject), Reading::value(requested));

        // 3. Put back exactly what was there — including "there was nothing".
        let restore_step = match &captured {
            Reading::Value { value } => RestoreStep::Write {
                setting: subject,
                value: *value,
            },
            Reading::SessionDefault { .. } => RestoreStep::Reset { setting: subject },
            other => panic!("a capture that is not restorable got past the guard: {other:?}"),
        };
        let started = Instant::now();
        let restored = backend.restore(&RestorePlan {
            scope: RestoreScope::All,
            steps: vec![restore_step],
        });
        println!(
            "{subject}: restore and verify round trip: {:?}",
            started.elapsed()
        );

        assert_eq!(
            restored.state(),
            RunState::Applied,
            "{subject} was not put back: {restored:?}"
        );
        assert_eq!(
            backend.read_one(subject),
            captured,
            "{subject} did not end up where it started"
        );
    }
}

#[test]
#[ignore = "reads the running session; needs BETTER_TOUCHPAD_LIVE=1"]
fn the_running_session_reports_the_controls_this_backend_owns() {
    if !opted_in() {
        panic!("set BETTER_TOUCHPAD_LIVE=1 to allow this test to read the running session");
    }
    let backend = GnomeBackend::connect(None);
    println!("backend status: {:?}", backend.status());
    for (setting, reading) in backend.read_all() {
        println!(
            "{setting}: support={:?} reading={reading:?}",
            backend.capabilities().support(setting)
        );
    }
    assert!(!backend.capabilities().available().is_empty());
}

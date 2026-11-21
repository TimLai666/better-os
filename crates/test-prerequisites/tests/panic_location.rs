//! Under CI a missing prerequisite fails the test at the test's own line,
//! so the failure names the test that needs it, not this crate (ticket 177's
//! review). It is the only test in this binary, so nothing else reads the
//! environment or the panic hook while they change.

use std::sync::{Arc, Mutex};

#[test]
fn a_missing_prerequisite_under_ci_fails_at_the_tests_own_line() {
    // SAFETY: the only test in this binary, before anything reads `CI`.
    unsafe { std::env::set_var("CI", "true") };
    let seen: Arc<Mutex<Option<(String, u32)>>> = Arc::default();
    let record = Arc::clone(&seen);
    std::panic::set_hook(Box::new(move |info| {
        if let Some(location) = info.location() {
            *record.lock().unwrap() = Some((location.file().to_string(), location.line()));
        }
    }));
    let expected_line = line!() + 2;
    let outcome = std::panic::catch_unwind(|| {
        test_prerequisites::available("gjs", Err::<(), _>("gjs is not installed"))
    });
    let _ = std::panic::take_hook();
    assert!(outcome.is_err(), "CI must fail on a missing prerequisite");
    let (file, line) = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the panic had a location");
    assert!(file.ends_with("tests/panic_location.rs"), "{file}");
    assert_eq!(line, expected_line);

    let seen_again: Arc<Mutex<Option<String>>> = Arc::default();
    let record = Arc::clone(&seen_again);
    std::panic::set_hook(Box::new(move |info| {
        *record.lock().unwrap() = info.location().map(|location| location.file().to_string());
    }));
    let outcome =
        std::panic::catch_unwind(|| test_prerequisites::holds("a user other than root", false));
    let _ = std::panic::take_hook();
    assert!(outcome.is_err());
    let file = seen_again
        .lock()
        .unwrap()
        .clone()
        .expect("the panic had a location");
    assert!(file.ends_with("tests/panic_location.rs"), "{file}");
}

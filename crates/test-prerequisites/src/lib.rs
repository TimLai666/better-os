//! What a test does when something it needs is not on this machine
//! (ticket 177).
//!
//! Some tests need a program (`dbus-daemon`, `dconf-service`, `gjs`), a
//! locale, a filesystem feature, or a user other than root. On a developer's
//! machine a missing prerequisite is a skip: the test says so on standard
//! error with [`SKIP_MARKER`] and returns. Under CI it is a failure, because
//! CI installs every prerequisite and a test that did nothing there would be
//! a check nobody runs while the run stays green.
//!
//! CI is any `CI` variable that is set, not empty, and not `false` or `0`
//! (GitHub Actions sets `CI=true`). The CI workflow runs the tests with their
//! output shown and fails when [`SKIP_MARKER`] appears, so a skip that reaches
//! the log through a misread `CI` variable still turns the run red.
//!
//! Every test that can skip goes through [`require!`] or [`require_that!`];
//! `tests/one_skip_rule.rs` fails when a test prints a skip of its own.
//!
//! ```ignore
//! let bus = test_prerequisites::require!("dbus-daemon", PrivateBus::start());
//! test_prerequisites::require_that!("a user other than root", !running_as_root());
//! ```

use std::fmt::Display;

/// The start of the line a skipped test writes to standard error. CI fails
/// a run whose test output contains it.
pub const SKIP_MARKER: &str = "better-os-test-skipped:";

/// Whether these tests run under CI, read from the `CI` variable.
pub fn in_ci() -> bool {
    is_ci(std::env::var("CI").ok().as_deref())
}

/// Whether a value of `CI` names a CI run.
fn is_ci(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "false" && value != "0")
}

/// The value `attempt` produced, or `None` after saying why the test cannot
/// run here. Under CI a failed attempt fails the test instead.
#[track_caller]
pub fn available<T, E: Display>(prerequisite: &str, attempt: Result<T, E>) -> Option<T> {
    decide(in_ci(), prerequisite, attempt)
        .map_err(|line| eprintln!("{line}"))
        .ok()
}

/// Whether `condition` holds, saying why the test cannot run here when it
/// does not. Under CI a condition that does not hold fails the test instead.
#[track_caller]
pub fn holds(prerequisite: &str, condition: bool) -> bool {
    available(prerequisite, held(condition)).is_some()
}

fn held(condition: bool) -> Result<(), &'static str> {
    if condition {
        Ok(())
    } else {
        Err("it does not hold on this machine")
    }
}

/// The value, or the skip line to print. Under CI there is no skip line: a
/// missing prerequisite panics, and the panic message leaves the marker out,
/// so a test of this rule that expects the panic prints nothing CI's check
/// would take for a skip.
#[track_caller]
fn decide<T, E: Display>(ci: bool, prerequisite: &str, attempt: Result<T, E>) -> Result<T, String> {
    attempt.map_err(|reason| {
        assert!(
            !ci,
            "CI must run this test, and it needs {prerequisite}: {reason}. \
             The CI workflow's install step lists what each test needs."
        );
        format!("{SKIP_MARKER} needs {prerequisite}: {reason}")
    })
}

/// The value of a `Result`, or a return from the calling test when it is an
/// error: a skip on a developer's machine, a failure under CI.
#[macro_export]
macro_rules! require {
    ($prerequisite:expr, $attempt:expr $(,)?) => {
        match $crate::available($prerequisite, $attempt) {
            ::std::option::Option::Some(value) => value,
            ::std::option::Option::None => return,
        }
    };
}

/// A return from the calling test when a condition does not hold: a skip on
/// a developer's machine, a failure under CI.
#[macro_export]
macro_rules! require_that {
    ($prerequisite:expr, $condition:expr $(,)?) => {
        if !$crate::holds($prerequisite, $condition) {
            return;
        }
    };
}

#[cfg(test)]
mod tests {
    // These call `decide`, which prints nothing: a skip line from this
    // crate's own tests would fail CI's check of the test output.
    use super::{SKIP_MARKER, decide, held, is_ci};

    #[test]
    fn ci_is_any_value_but_empty_false_and_zero() {
        assert!(is_ci(Some("true")));
        assert!(is_ci(Some("1")));
        assert!(is_ci(Some("yes")));
        assert!(!is_ci(None));
        assert!(!is_ci(Some("")));
        assert!(!is_ci(Some("false")));
        assert!(!is_ci(Some("0")));
    }

    #[test]
    fn a_missing_prerequisite_is_a_skip_on_a_developers_machine() {
        let attempt: Result<(), &str> = Err("gjs is not installed");
        let line = decide(false, "gjs", attempt).unwrap_err();
        assert!(line.starts_with(SKIP_MARKER), "{line}");
        assert!(line.contains("needs gjs: gjs is not installed"), "{line}");
        let line = decide(false, "a user other than root", held(false)).unwrap_err();
        assert!(line.starts_with(SKIP_MARKER), "{line}");
    }

    // This is the proof ticket 177 asks for: CI runs with `CI=true`, so a
    // prerequisite taken out of the workflow's install step makes the test
    // that needs it reach this panic, and the run goes red.
    #[test]
    #[should_panic(expected = "CI must run this test, and it needs gjs")]
    fn a_missing_program_fails_under_ci() {
        let attempt: Result<(), &str> = Err("gjs is not installed");
        let _ = decide(true, "gjs", attempt);
    }

    #[test]
    #[should_panic(expected = "CI must run this test, and it needs a user other than root")]
    fn a_condition_that_does_not_hold_fails_under_ci() {
        let _ = decide(true, "a user other than root", held(false));
    }

    #[test]
    fn a_prerequisite_that_is_there_is_passed_through_under_ci_too() {
        assert_eq!(decide(true, "dbus-daemon", Ok::<_, &str>(7)), Ok(7));
        assert_eq!(decide(true, "a user other than root", held(true)), Ok(()));
    }
}

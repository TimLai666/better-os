//! The two commands the field report said panicked and then hung.
//!
//! `better-manager catalog status` on a Zorin 18 machine printed zbus's "there
//! is no reactor running" from a worker thread and then never returned. Both
//! halves of that had causes, and both are covered here:
//!
//! * `/usr/bin/better-manager` was the *window*, which ignored its arguments
//!   and opened a window — a program that does not exit is what a hang looks
//!   like from a terminal. The command line now ships beside it as
//!   `better-manager-cli`, and `gui_invocation.rs` in `manager-gui` covers the
//!   window's half.
//! * Cargo unified zbus's `tokio` feature into every binary built alongside the
//!   services, and that flavor panics off a tokio thread.
//!   `manager-platform::flavor` keeps the feature out; this file runs the real
//!   binary and reads its stderr, because a manifest scan cannot prove the
//!   shipped program is quiet.
//!
//! These run in the default execution mode — the mode the report used — rather
//! than `--execution mock`, so the host probe, the state store, and the catalog
//! all run the way they do for a person. A machine outside the release matrix
//! is a supported outcome of that: it stops the command with a stated reason,
//! which is a clean exit and not a hang. That is why the assertion is on
//! termination and on the absence of a panic, and on the exit status only when
//! the host resolved.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Longer than either command should ever need and far shorter than the two
/// minutes the report waited before giving up.
const TIMEOUT: Duration = Duration::from_secs(30);

struct Run {
    status: Option<i32>,
    stderr: String,
    stdout: String,
    timed_out: bool,
}

fn run(directory: &Path, arguments: &[&str]) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args([
            "--state-path",
            &directory.join("state.json").display().to_string(),
        ])
        .args([
            "--catalog-path",
            &directory.join("catalog.json").display().to_string(),
        ])
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the manager binary runs");

    let started = Instant::now();
    loop {
        match child.try_wait().expect("the child can be waited on") {
            Some(status) => {
                let output = child.wait_with_output().expect("the pipes can be read");
                return Run {
                    status: status.code(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    timed_out: false,
                };
            }
            None if started.elapsed() >= TIMEOUT => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("the pipes can be read");
                return Run {
                    status: None,
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    timed_out: true,
                };
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn assert_finished_without_panicking(run: &Run, what: &str) {
    assert!(
        !run.timed_out,
        "{what} did not finish within {TIMEOUT:?}; stderr was:\n{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("panicked"),
        "{what} panicked:\n{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("there is no reactor running"),
        "{what} hit the zbus reactor panic again:\n{}",
        run.stderr
    );
    // A host this release does not publish for is the one refusal these
    // commands are allowed to end in, and it is stated rather than silent.
    let clean = run.status == Some(0);
    assert!(
        clean || run.stderr.contains("platform.error.unsupported_host"),
        "{what} exited with {:?} for a reason that is not an unsupported host:\n{}",
        run.status,
        run.stderr
    );
}

fn temporary(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("better-os-invocation-{name}-"))
        .tempdir()
        .unwrap()
}

#[test]
fn catalog_status_finishes_quietly_in_the_mode_a_person_runs_it_in() {
    let directory = temporary("catalog-status");
    let run = run(directory.path(), &["catalog", "status"]);
    assert_finished_without_panicking(&run, "catalog status");
    if run.status == Some(0) {
        assert!(run.stdout.contains("catalog source:"), "{}", run.stdout);
    }
}

#[test]
fn status_for_one_component_finishes_quietly_in_the_mode_a_person_runs_it_in() {
    let directory = temporary("status");
    let run = run(directory.path(), &["status", "better-manager"]);
    assert_finished_without_panicking(&run, "status better-manager");
}

/// The whole `status` table, which walks every manifest rather than one.
#[test]
fn status_for_every_component_finishes_quietly() {
    let directory = temporary("status-all");
    let run = run(directory.path(), &["status"]);
    assert_finished_without_panicking(&run, "status");
}

/// `reconcile` reads dpkg and writes the state file, and it is the command the
/// self-healing path runs. It has to come back too.
#[test]
fn reconcile_finishes_quietly() {
    let directory = temporary("reconcile");
    let run = run(directory.path(), &["reconcile"]);
    assert_finished_without_panicking(&run, "reconcile");
}

/// `--adopt` names one component. An id the catalog does not carry is refused
/// rather than accepted and written.
#[test]
fn adopting_an_unknown_component_is_refused_without_writing_state() {
    let directory = temporary("adopt-unknown");
    let run = run(
        directory.path(),
        &["reconcile", "--adopt", "better-nothing"],
    );
    assert!(!run.timed_out, "the command hung");
    assert!(!run.stderr.contains("panicked"), "{}", run.stderr);
    assert_ne!(run.status, Some(0));
}

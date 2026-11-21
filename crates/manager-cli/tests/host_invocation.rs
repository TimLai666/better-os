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
    /// Every `systemctl` invocation the command attempted. The guard flag
    /// stops the binary from running them, and the guard's log variable
    /// makes it record them here, so a real mode run can never reach the
    /// user manager of whoever runs the tests.
    systemctl: Option<String>,
}

/// Where the guarded binary writes the `systemctl` calls it was stopped from
/// making.
fn systemctl_guard(directory: &Path) -> std::path::PathBuf {
    directory.join("systemctl-guard.log")
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
        .env(better_core::test_guard::GUARD_VARIABLE, "1")
        .env(
            better_core::test_guard::LOG_VARIABLE,
            systemctl_guard(directory),
        )
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
                    systemctl: std::fs::read_to_string(systemctl_guard(directory)).ok(),
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
                    systemctl: std::fs::read_to_string(systemctl_guard(directory)).ok(),
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
    // A fresh state records nothing as enabled, so the startup pass has
    // nothing to start; and were it to try, the guard would have stopped it.
    assert_eq!(
        run.systemctl, None,
        "{what} tried to run systemctl in a test"
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
    assert_eq!(run.systemctl, None);
}

/// The package the guard tests record as installed and enabled.
const GUARDED_PACKAGE: &str = "better-git";

/// The installed version a state file records for `package`.
fn recorded_version(state: &Path, package: &str) -> String {
    let text = std::fs::read_to_string(state).expect("the install wrote a state");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    fn find(value: &serde_json::Value, package: &str, inside: bool) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                if inside
                    && let Some(serde_json::Value::String(version)) = map.get("installed_version")
                {
                    return Some(version.clone());
                }
                map.iter()
                    .find_map(|(key, child)| find(child, package, inside || key == package))
            }
            serde_json::Value::Array(items) => {
                items.iter().find_map(|child| find(child, package, inside))
            }
            _ => None,
        }
    }
    find(&value, package, false).expect("the state records the package's version")
}

/// A state that records `GUARDED_PACKAGE` installed and enabled, and a
/// folder holding a `dpkg-query` that says so and lists one user unit for
/// it: the startup pass then has a unit to start, so the guard has a
/// command to hold back. The fake is the checked-in fixture reached through
/// a link, never a file this test wrote (AGENTS.md).
struct Recorded {
    directory: tempfile::TempDir,
    bin: std::path::PathBuf,
    version: String,
}

fn recorded(name: &str) -> Recorded {
    let directory = temporary(name);
    let install = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args(["--execution", "mock"])
        .args([
            "--state-path",
            &directory.path().join("state.json").display().to_string(),
        ])
        .args([
            "--catalog-path",
            &directory.path().join("catalog.json").display().to_string(),
        ])
        .args(["run", GUARDED_PACKAGE, "install"])
        .env("HOME", directory.path())
        .env(better_core::test_guard::GUARD_VARIABLE, "1")
        .output()
        .expect("the manager binary runs");
    assert!(install.status.success(), "{install:?}");
    let version = recorded_version(&directory.path().join("state.json"), GUARDED_PACKAGE);
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dpkg-query"),
        bin.join("dpkg-query"),
    )
    .unwrap();
    Recorded {
        directory,
        bin,
        version,
    }
}

/// Runs `status` in the real mode, so the startup pass runs, from
/// `started_in`, with the guard flag and the log variable as given.
fn guarded_status(
    recorded: &Recorded,
    started_in: &Path,
    log: Option<&Path>,
) -> std::process::Output {
    let path = format!(
        "{}:{}",
        recorded.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_manager-cli"));
    command
        .args([
            "--state-path",
            &recorded
                .directory
                .path()
                .join("state.json")
                .display()
                .to_string(),
        ])
        .args([
            "--catalog-path",
            &recorded
                .directory
                .path()
                .join("catalog.json")
                .display()
                .to_string(),
        ])
        .arg("status")
        .current_dir(started_in)
        .env("PATH", path)
        .env("HOME", recorded.directory.path())
        .env("BETTER_OS_FAKE_DPKG_PACKAGE", GUARDED_PACKAGE)
        .env("BETTER_OS_FAKE_DPKG_VERSION", &recorded.version)
        .env(better_core::test_guard::GUARD_VARIABLE, "1");
    match log {
        Some(log) => command.env(better_core::test_guard::LOG_VARIABLE, log),
        None => command.env_remove(better_core::test_guard::LOG_VARIABLE),
    };
    command.output().expect("the manager binary runs")
}

fn supported_host(output: &std::process::Output) -> bool {
    !String::from_utf8_lossy(&output.stderr).contains("platform.error.unsupported_host")
}

/// The guard is a flag (ticket 178). With a component the startup pass
/// would start, the command holds a `systemctl` call back. Started with only
/// the flag, from an empty folder, it writes nothing there: `=1` used to be
/// taken as a log path and left a file named `1` in whatever folder Better
/// Manager was started from. The same command with the log variable too
/// writes the call it held back where the variable names, which is what
/// shows the first run had one to hold back (a held-back call prints
/// nothing, because to the startup pass it is a session with no user
/// manager).
#[test]
fn the_guard_flag_alone_writes_nothing_and_the_log_records_what_it_held_back() {
    let recorded = recorded("flag-alone");
    let started_in = recorded.directory.path().join("started-here");
    std::fs::create_dir(&started_in).unwrap();

    let output = guarded_status(&recorded, &started_in, None);

    test_prerequisites::require_that!("a host this release supports", supported_host(&output));
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("panicked"),
        "{output:?}"
    );
    let left: Vec<_> = std::fs::read_dir(&started_in)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "the guard wrote {left:?}");

    let log = recorded.directory.path().join("held-back.log");
    let output = guarded_status(&recorded, &started_in, Some(&log));
    let held = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        held.lines()
            .any(|line| line.starts_with("systemctl --user")),
        "nothing was held back, so the first run proved nothing: {held:?}; stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_dir(&started_in).unwrap().count(), 0);
}

/// A relative log path is refused with a message and writes nothing.
#[test]
fn a_relative_guard_log_is_refused_and_writes_nothing() {
    let recorded = recorded("relative-log");
    let started_in = recorded.directory.path().join("started-here");
    std::fs::create_dir(&started_in).unwrap();

    let output = guarded_status(&recorded, &started_in, Some(Path::new("held-back.log")));

    test_prerequisites::require_that!("a host this release supports", supported_host(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(better_core::test_guard::LOG_VARIABLE) && stderr.contains("absolute"),
        "{stderr}"
    );
    assert_eq!(std::fs::read_dir(&started_in).unwrap().count(), 0);
}

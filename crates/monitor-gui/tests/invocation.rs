//! What the shipped `better-monitor` binary does when it is handed arguments it
//! should answer or refuse rather than open a window for.
//!
//! These run the real binary. Every case must end on its own within the
//! timeout without opening a window, which is why none of them passes an
//! argument the window would accept.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Exit status, stdout, and stderr, or `None` for the status when the binary
/// had to be killed because it did not exit.
fn run(arguments: &[&str]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_monitor-gui"))
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the window binary runs");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child can be waited on") {
            break status.code();
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = child.wait_with_output().expect("the pipes can be read");
    (
        status,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn help_and_version_are_answered_without_a_window() {
    for flag in ["--help", "--version"] {
        let (status, stdout, stderr) = run(&[flag]);
        assert_eq!(status, Some(0), "{flag} did not exit cleanly: {stderr}");
        assert!(stdout.contains("better-monitor"), "{flag}: {stdout}");
    }
    let (_, stdout, _) = run(&["--version"]);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
}

#[test]
fn an_unknown_argument_is_refused_with_exit_2() {
    let (status, stdout, stderr) = run(&["inspect"]);
    assert_eq!(status, Some(2), "stdout: {stdout}\nstderr: {stderr}");
    assert!(stderr.contains("「inspect」"), "{stderr}");
    assert!(stderr.contains("better-monitor-cli"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

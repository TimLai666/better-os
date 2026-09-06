//! What `/usr/bin/better-manager` does when it is handed a command line.
//!
//! It used to ignore every argument and open a window. `better-manager catalog
//! status` therefore ran the manager's window on a machine whose user was
//! waiting at a terminal, and a window does not exit — the field report called
//! it a hang, and it was one.
//!
//! These tests run the real binary. Every case here must terminate on its own
//! within the timeout without opening a window, which is why none of them
//! passes an argument the window would accept.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(20);

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn run(arguments: &[&str]) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_manager-gui"))
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the window binary runs");

    let started = Instant::now();
    loop {
        match child.try_wait().expect("the child can be waited on") {
            Some(status) => {
                let output = child.wait_with_output().expect("the pipes can be read");
                return Run {
                    status: status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    timed_out: false,
                };
            }
            None if started.elapsed() >= TIMEOUT => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("the pipes can be read");
                return Run {
                    status: None,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    timed_out: true,
                };
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The reported invocation. It must come back, say what it is, and point at the
/// program that would have answered it.
#[test]
fn a_command_line_argument_is_refused_instead_of_opening_a_window() {
    let run = run(&["catalog", "status"]);

    assert!(
        !run.timed_out,
        "the window binary did not exit for a command-line argument"
    );
    assert_eq!(run.status, Some(2), "stderr was:\n{}", run.stderr);
    assert!(
        run.stderr.contains("better-manager-cli"),
        "the refusal must name the program that answers this: {}",
        run.stderr
    );
    assert!(run.stderr.contains("catalog"), "{}", run.stderr);
    assert!(
        !run.stderr.contains("panicked"),
        "the refusal must not panic:\n{}",
        run.stderr
    );
}

/// `--version` at a program's own name is a question, not a request for a
/// window, and it is what ticket 46 taught the command line to answer.
#[test]
fn the_window_answers_version_and_help_without_opening_anything() {
    for flag in ["--version", "-V"] {
        let run = run(&[flag]);
        assert!(!run.timed_out, "{flag} did not exit");
        assert_eq!(run.status, Some(0), "{flag}: {}", run.stderr);
        assert!(
            run.stdout.contains(env!("CARGO_PKG_VERSION")),
            "{flag}: {}",
            run.stdout
        );
    }

    for flag in ["--help", "-h"] {
        let run = run(&[flag]);
        assert!(!run.timed_out, "{flag} did not exit");
        assert_eq!(run.status, Some(0), "{flag}: {}", run.stderr);
        assert!(
            run.stdout.contains("better-manager-cli"),
            "{flag} must point at the command line: {}",
            run.stdout
        );
    }
}

/// The packaging is what makes the refusal's advice true: a message naming a
/// program that no package installs would be worse than no message.
#[test]
fn the_package_ships_the_command_line_the_refusal_names() {
    let script = include_str!("../../../packaging/build-deb.sh");
    assert!(
        script.contains("\"manager-cli:usr/bin/better-manager-cli\""),
        "the better-manager package must install the command line"
    );
    assert!(
        script.contains("-p manager-cli"),
        "the packaging build must build the command line"
    );
}

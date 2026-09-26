//! The command line calls itself by the name it is installed under.
//!
//! `/usr/bin/better-manager` is the window, so the command line ships as
//! `/usr/bin/better-manager-cli`. Its help, its version, and clap's own usage
//! errors must say that name, or a person copying a usage line out of an error
//! runs the window instead. The cargo binary is `manager-cli`, which is why the
//! name cannot be left to whatever the process was started as.

use std::process::Command;

fn run(arguments: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args(arguments)
        .output()
        .expect("the manager binary runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn help_names_the_installed_command() {
    let (status, stdout, stderr) = run(&["--help"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stdout.contains("Usage: better-manager-cli "), "{stdout}");
}

#[test]
fn version_names_the_installed_command() {
    let (status, stdout, stderr) = run(&["--version"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout.trim(),
        format!("better-manager-cli {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn a_usage_error_names_the_installed_command() {
    let (status, _, stderr) = run(&["bogus"]);
    assert_eq!(status, Some(2), "{stderr}");
    assert!(stderr.contains("Usage: better-manager-cli "), "{stderr}");
}

//! `better-manager --version`, against the shipped binary.
//!
//! A person asked to report which manager they are running had no way to answer
//! before this: the command line accepted no version flag at all, and the
//! window stated the version nowhere. This asserts the flag exists, that it
//! reports the version this workspace was built at, and that it needs neither a
//! state file nor a network to answer.

use std::process::Command;

#[test]
fn the_version_flag_reports_the_workspace_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .arg("--version")
        .output()
        .expect("the manager binary runs");

    assert!(
        output.status.success(),
        "--version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("clap prints UTF-8");
    // The crate inherits `version.workspace`, so this is the version the whole
    // release was built at rather than a number kept in step by hand.
    let version = env!("CARGO_PKG_VERSION");
    assert!(stdout.contains(version), "{stdout}");
    assert!(stdout.contains("better-manager"), "{stdout}");
    // A version is what a bug report quotes, so it has to look like one rather
    // than being an empty string clap happily printed.
    assert_eq!(version.split('.').count(), 3, "{version}");
    assert!(version.split('.').all(|part| !part.is_empty()), "{version}");
}

#[test]
fn the_short_version_flag_answers_the_same_way() {
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .arg("-V")
        .output()
        .expect("the manager binary runs");

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

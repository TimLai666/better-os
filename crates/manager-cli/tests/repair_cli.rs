//! `better-manager-cli repair` (ticket 68, ADR 0014).
//!
//! Nothing here reaches a system bus. The real path asks the privileged
//! service, and what `manager-core` does with its answer is tested there
//! against a fake service. What these tests prove is the command's surface: it
//! exists, says what it does, and refuses to pretend in a simulation.

use std::process::Command;

fn run(arguments: &[&str]) -> std::process::Output {
    let directory = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args([
            "--state-path",
            &directory.path().join("state.json").display().to_string(),
        ])
        .args([
            "--catalog-path",
            &directory.path().join("catalog.json").display().to_string(),
        ])
        .args(arguments)
        .output()
        .expect("the manager binary runs")
}

#[test]
fn the_repair_command_is_listed_and_documents_its_check_flag() {
    let help = run(&["--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("repair"));

    let help = run(&["repair", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("dpkg --configure -a"), "{text}");
    assert!(text.contains("--check"), "{text}");
}

/// A simulated repair would report that a machine changed when nothing did.
#[test]
fn a_simulation_refuses_to_repair_rather_than_pretending() {
    for arguments in [
        &["--execution", "mock", "repair"][..],
        &["--execution", "mock", "repair", "--check"][..],
    ] {
        let output = run(arguments);
        assert!(!output.status.success(), "{arguments:?} must not succeed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("manager.error.repair_needs_real_execution"),
            "{arguments:?}: {stderr}"
        );
    }
}

/// `repair` takes nothing that could reach the command the daemon runs.
#[test]
fn the_repair_command_takes_no_package_or_option_for_dpkg() {
    let output = run(&["--execution", "mock", "repair", "better-monitor"]);
    assert_eq!(output.status.code(), Some(2), "clap refuses the argument");
}

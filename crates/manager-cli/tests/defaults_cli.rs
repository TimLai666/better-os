//! The five `defaults` subcommands, end to end, against a disposable snapshot
//! store and a simulated desktop.
//!
//! Everything here runs the shipped binary. The point is not to re-test the
//! engine but to prove the CLI reaches the same one, and that the honest
//! outcomes — skipped, changed externally, restored — actually reach a user.

use std::path::Path;
use std::process::Command;

const MANIFEST: &str = r#"
schema_version: 2
id: better-files
display_name: Better Files
component_type: replacement
version: 1.0.0
targets:
  distributions: [ubuntu]
  releases: ["24.04"]
  architectures: [amd64]
artifacts:
  - release: "24.04"
    architecture: amd64
    url: https://example.com/better-files_1.0.0_ubuntu-24.04_amd64.deb
    sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    release_asset: better-files_1.0.0_ubuntu-24.04_amd64.deb
lifecycle:
  install: plan-install
  enable: plan-enable
  disable: plan-disable
  remove: plan-remove
  rollback: plan-rollback
default_integrations:
  - id: default-file-manager
    kind: application-handler
    exclusivity: exclusive
    target:
      desired:
        type: desktop_entry
        value: io.betteros.Files.desktop
      keys: [inode/directory]
    platforms: [ubuntu]
    sessions: [gnome]
    apply_adapter: xdg-default-app
    verify_adapter: xdg-default-app
    restore_policy: captured-value
    privileges: user
    session_effect: immediate
    health_prerequisites: []
"#;

struct Cli {
    directory: tempfile::TempDir,
}

impl Cli {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("better-files.yaml"), MANIFEST).unwrap();
        // The desktop starts out pointing somewhere else, the way a real one
        // would before Better OS is asked to change anything.
        std::fs::write(
            directory.path().join("desktop.json"),
            r#"{"XdgDefaultApp/better-files/default-file-manager":
                 {"state":"set","value":{"type":"desktop_entry",
                  "value":"org.gnome.Nautilus.desktop"}}}"#,
        )
        .unwrap();
        Self { directory }
    }

    fn path(&self, name: &str) -> String {
        self.directory.path().join(name).display().to_string()
    }

    fn run(&self, arguments: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
            .env("XDG_CURRENT_DESKTOP", "GNOME")
            .args(["--execution", "mock"])
            .args(["--state-path", &self.path("state.json")])
            .arg("defaults")
            .args(["--manifest", &self.path("better-files.yaml")])
            .args(["--snapshot-dir", &self.path("snapshots")])
            .args(["--mock-desktop", &self.path("desktop.json")])
            .args(arguments)
            .output()
            .expect("the manager binary runs");
        assert!(
            output.status.success(),
            "{:?} failed: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn desktop(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("desktop.json")).unwrap()
    }

    fn snapshot_count(&self) -> usize {
        match std::fs::read_dir(self.directory.path().join("snapshots")) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
                .count(),
            Err(_) => 0,
        }
    }
}

#[test]
fn inspect_plan_apply_verify_and_restore_share_one_engine() {
    let cli = Cli::new();

    let inspect = cli.run(&["inspect"]);
    assert!(inspect.contains("better-files not-default"));
    assert!(inspect.contains("default-file-manager"));
    assert!(inspect.contains("org.gnome.Nautilus.desktop"));
    assert_eq!(cli.snapshot_count(), 0);

    let plan = cli.run(&["plan"]);
    assert!(plan.contains("Apply plan: 1 of 1 entries would change"));
    assert!(plan.contains("apply DesktopEntry(\"io.betteros.Files.desktop\")"));
    // Planning changes nothing.
    assert_eq!(cli.snapshot_count(), 0);
    assert!(cli.desktop().contains("Nautilus"));

    let apply = cli.run(&["apply"]);
    assert!(apply.contains("captured previous values into snapshot"));
    assert!(apply.contains("applied DesktopEntry(\"io.betteros.Files.desktop\")"));
    assert!(cli.desktop().contains("io.betteros.Files.desktop"));

    let verify = cli.run(&["verify"]);
    assert!(verify.contains("better-files default"));

    let restore_plan = cli.run(&["plan", "--restore"]);
    assert!(restore_plan.contains("Restore plan: 1 of 1 entries would change"));
    assert!(restore_plan.contains("restore Set"));

    let restore = cli.run(&["restore"]);
    assert!(restore.contains("restored Set"));
    assert!(cli.desktop().contains("org.gnome.Nautilus.desktop"));
}

#[test]
fn a_change_made_outside_better_manager_is_reported_and_needs_confirming() {
    let cli = Cli::new();
    cli.run(&["apply"]);

    // Something else takes the association over.
    std::fs::write(
        cli.directory.path().join("desktop.json"),
        r#"{"XdgDefaultApp/better-files/default-file-manager":
             {"state":"set","value":{"type":"desktop_entry",
              "value":"org.kde.dolphin.desktop"}}}"#,
    )
    .unwrap();

    let inspect = cli.run(&["inspect"]);
    assert!(inspect.contains("better-files changed-externally"));

    let plan = cli.run(&["plan"]);
    assert!(plan.contains("Apply plan: 0 of 1 entries would change"));
    assert!(plan.contains("ChangedExternallyWithoutConfirmation"));
    assert!(plan.contains("--confirm-external better-files:default-file-manager"));

    // Applying without the confirmation leaves the other program's choice in
    // place.
    cli.run(&["apply"]);
    assert!(cli.desktop().contains("org.kde.dolphin.desktop"));

    let confirmed = cli.run(&[
        "--confirm-external",
        "better-files:default-file-manager",
        "apply",
    ]);
    assert!(confirmed.contains("OverwritesExternalChange"));
    assert!(confirmed.contains("applied DesktopEntry(\"io.betteros.Files.desktop\")"));
    assert!(cli.desktop().contains("io.betteros.Files.desktop"));
}

#[test]
fn the_built_in_catalog_declares_no_defaults_and_says_so() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .args(["--execution", "mock"])
        .args([
            "--state-path",
            &directory.path().join("state.json").display().to_string(),
        ])
        .arg("defaults")
        .args([
            "--snapshot-dir",
            &directory.path().join("snapshots").display().to_string(),
        ])
        .arg("inspect")
        .output()
        .expect("the manager binary runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("no component in this catalog declares a default integration"));
    assert!(!Path::new(&directory.path().join("snapshots")).exists());
}

#[test]
fn selecting_one_component_never_reaches_another() {
    let cli = Cli::new();
    let plan = cli.run(&["--component", "better-files", "plan"]);
    assert!(plan.contains("better-files:default-file-manager"));

    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .env("XDG_CURRENT_DESKTOP", "GNOME")
        .args(["--execution", "mock"])
        .args(["--state-path", &cli.path("state.json")])
        .arg("defaults")
        .args(["--manifest", &cli.path("better-files.yaml")])
        .args(["--snapshot-dir", &cli.path("snapshots")])
        .args(["--component", "better-monitor"])
        .arg("plan")
        .output()
        .expect("the manager binary runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Apply plan: 0 of 0 entries would change"));
}

#[test]
fn a_session_the_declaration_does_not_support_is_unavailable_rather_than_attempted() {
    let cli = Cli::new();
    let output = Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .env("XDG_CURRENT_DESKTOP", "KDE")
        .args(["--execution", "mock"])
        .args(["--state-path", &cli.path("state.json")])
        .arg("defaults")
        .args(["--manifest", &cli.path("better-files.yaml")])
        .args(["--snapshot-dir", &cli.path("snapshots")])
        .args(["--mock-desktop", &cli.path("desktop.json")])
        .arg("inspect")
        .output()
        .expect("the manager binary runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("unavailable (defaults.not_supported_on_this_system)"));
    assert!(cli.desktop().contains("Nautilus"));
}

//! `better-manager-cli run <id> remove` and the person's settings and history
//! (owner, 2026-11-13): removed with the component unless `--keep-data` says
//! otherwise. A simulation names what it would move and moves nothing; the
//! move itself is tested in `manager-platform` against a scratch home.

use std::path::Path;
use std::process::{Command, Output};

fn run(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_manager-cli"))
        .args(["--execution", "mock"])
        .args([
            "--state-path",
            &directory.join("state.json").display().to_string(),
        ])
        .args([
            "--catalog-path",
            &directory.join("catalog.json").display().to_string(),
        ])
        .args(arguments)
        .env("HOME", directory)
        .env("XDG_CONFIG_HOME", directory.join(".config"))
        .env("XDG_STATE_HOME", directory.join(".local/state"))
        .env("XDG_DATA_HOME", directory.join(".local/share"))
        .env("BETTER_OS_TEST_SYSTEMCTL_GUARD", "1")
        .output()
        .expect("the CLI runs")
}

fn installed_git(directory: &Path) {
    let output = run(directory, &["run", "better-git", "install"]);
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn removing_names_the_personal_data_it_moves_and_never_a_shared_file() {
    let directory = tempfile::tempdir().unwrap();
    installed_git(directory.path());
    let data = directory.path().join(".config/better-os");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("git.json"), "{}").unwrap();

    let output = run(directory.path(), &["run", "better-git", "remove"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("would move to the trash: ~/.config/better-os/git.json"));
    assert!(stdout.contains("would move to the trash: ~/.local/state/better-os/git-recent.json"));
    // A simulation changes nothing on the machine.
    assert!(data.join("git.json").exists());
}

#[test]
fn keep_data_leaves_everything_for_a_reinstall() {
    let directory = tempfile::tempdir().unwrap();
    installed_git(directory.path());
    let output = run(
        directory.path(),
        &["run", "better-git", "remove", "--keep-data"],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("trash"));
}

#[test]
fn keep_data_means_nothing_without_a_removal() {
    let directory = tempfile::tempdir().unwrap();
    let output = run(
        directory.path(),
        &["run", "better-git", "install", "--keep-data"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("keep_data_without_remove"));
}

#[test]
fn removing_one_component_names_only_its_own_entries_and_logs_what_it_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let install = run(directory.path(), &["run", "better-files", "install"]);
    assert!(install.status.success(), "{install:?}");
    let output = run(directory.path(), &["run", "better-files", "remove"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines().filter(|line| line.contains("trash")) {
        assert!(
            line.contains("/files") || line.contains("/app-chooser"),
            "{line}"
        );
    }
    let trashed: String = stdout
        .lines()
        .filter(|line| line.contains("trash"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!trashed.is_empty());
    assert!(!trashed.contains("mimeapps.list"));
    assert!(!trashed.contains("bookmarks"));
    assert!(!trashed.contains("git"));
    assert!(
        stderr.contains("not deleting ~/.config/mimeapps.list"),
        "{stderr}"
    );
    assert!(
        stderr.contains("not deleting ~/.config/gtk-3.0/bookmarks"),
        "{stderr}"
    );
}

#[test]
fn a_historical_entry_is_named_even_though_no_version_writes_it_now() {
    let directory = tempfile::tempdir().unwrap();
    installed_git(directory.path());
    let output = run(directory.path(), &["run", "better-git", "remove"]);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("would move to the trash: ~/.local/state/better-os/git-repositories.json")
    );
}

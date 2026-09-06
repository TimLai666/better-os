//! The one thing every zbus dependency in this workspace has to agree on.
//!
//! zbus picks its I/O backend at compile time from a cargo feature, and cargo
//! unifies features across every package a single `cargo build` names.
//! `packaging/build-deb.sh` builds the services and the windows in one
//! invocation, so one crate asking for zbus's `tokio` feature compiles that
//! flavor into every Better OS window as well.
//!
//! That flavor spawns zbus's internal tasks with `tokio::task::spawn`, which
//! panics off a tokio thread. gpui opens a zbus connection to the XDG desktop
//! portal from its own background executor, and accesskit opens one to the
//! accessibility bus from a thread it spawns itself — neither is reachable
//! from this project, so neither can be wrapped in a runtime guard. Both
//! panicked with "there is no reactor running" on every window `v0.2.5`
//! shipped. Ticket 49 removed the feature rather than the symptom.
//!
//! The check is on the manifests rather than on a built binary because that is
//! where the mistake is made, and because a test that only fails once the
//! packaging build runs would find it after the release.

/// Every `Cargo.toml` in the workspace, as (path, text).
#[cfg(test)]
fn workspace_manifests() -> Vec<(std::path::PathBuf, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("manager-platform sits two directories below the workspace root")
        .to_path_buf();

    let mut manifests = vec![(
        root.join("Cargo.toml"),
        std::fs::read_to_string(root.join("Cargo.toml")).expect("the workspace manifest is there"),
    )];
    let crates = std::fs::read_dir(root.join("crates")).expect("the crates directory is there");
    for entry in crates {
        let path = entry
            .expect("a readable directory entry")
            .path()
            .join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&path) {
            manifests.push((path, text));
        }
    }
    manifests
}

#[cfg(test)]
mod tests {
    use super::workspace_manifests;

    /// The regression this file exists for. A dependency line naming both
    /// `zbus` and the `tokio` feature is the mistake, wherever it is written.
    #[test]
    fn no_crate_asks_zbus_for_its_tokio_flavor() {
        let mut offenders = Vec::new();
        for (path, text) in workspace_manifests() {
            for (number, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.starts_with('#') {
                    continue;
                }
                let names_zbus = line.starts_with("zbus =") || line.starts_with("zbus_polkit =");
                if names_zbus && line.contains("\"tokio\"") {
                    offenders.push(format!("{}:{}: {line}", path.display(), number + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "zbus's tokio flavor panics on gpui's portal thread and on accesskit's, \
             and cargo compiles it into every binary built alongside the crate that asks \
             for it:\n{}",
            offenders.join("\n")
        );
    }

    /// The scan has to be able to see a violation, or its silence means
    /// nothing. This is the line ticket 49 removed, checked against the same
    /// rule the real manifests are checked against.
    #[test]
    fn the_scan_recognizes_the_line_it_was_written_to_catch() {
        let removed = r#"zbus = { version = "5", default-features = false, features = ["tokio"] }"#;
        assert!(removed.starts_with("zbus =") && removed.contains("\"tokio\""));
        let kept = r#"zbus = { version = "5", optional = true }"#;
        assert!(!kept.contains("\"tokio\""));
    }

    /// Every manifest that depends on zbus at all is found by the scan, so a
    /// crate cannot fall out of it by being named something new.
    #[test]
    fn the_scan_reaches_every_crate_that_depends_on_zbus() {
        let with_zbus = workspace_manifests()
            .into_iter()
            .filter(|(_, text)| text.contains("zbus"))
            .count();
        assert!(
            with_zbus >= 10,
            "expected the scan to see every zbus consumer, saw {with_zbus}"
        );
    }
}

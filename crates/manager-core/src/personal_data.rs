//! Which of the person's files "Also delete my settings and history" deletes
//! when a component is removed.
//!
//! The answer is a closed list, kept here and nowhere else (owner,
//! 2026-11-15): every directory or file a Better OS component has ever kept
//! the person's data in, current and historical, each with the component that
//! owns it. Removing a component deletes its own entries and nothing else:
//! never another component's, never Better Manager's own state, catalog, or
//! download cache, and never anything off the list. A shared file such as
//! `~/.config/mimeapps.list` or GTK's bookmarks is not on it, whatever a
//! manifest says.
//!
//! A manifest's `paths` are only checked against the list. A home path in
//! them that is not one of its own component's entries is refused with a
//! reason the caller logs; it is never deleted. A historical entry is deleted
//! whether or not a current manifest still names it, so data an older
//! version wrote does not outlive the component.
//!
//! The list was built from every manifest's `paths`, every crate's state,
//! config, cache, and data path, and `git log -p --all -S` over those paths.

/// One entry: a path relative to the home directory, as the default XDG
/// locations put it, and the component that owns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataEntry {
    pub component: &'static str,
    /// Relative to the home directory, no leading `~/`, no trailing `/`.
    pub path: &'static str,
    /// Written by a version no longer shipped; kept so it is still cleaned.
    pub historical: bool,
}

const fn entry(component: &'static str, path: &'static str) -> DataEntry {
    DataEntry {
        component,
        path,
        historical: false,
    }
}

const fn historical(component: &'static str, path: &'static str) -> DataEntry {
    DataEntry {
        component,
        path,
        historical: true,
    }
}

/// Every data location a Better OS component has used.
pub const DATA_DIRECTORIES: &[DataEntry] = &[
    // Rules, history, the service's state and settings, quick sessions, and
    // the window's preferences (`awake-gui-preferences.json`) all live here.
    entry("better-awake", ".local/state/better-awake"),
    // History store (schema 1 at the top, schema 2 below it).
    entry("better-monitor", ".local/state/better-monitor"),
    // Appearance, language, and the tray's settings.
    entry("better-monitor", ".config/better-os/monitor"),
    entry("better-files", ".config/better-os/files"),
    entry("better-files", ".local/state/better-os/files"),
    // Durable jobs, LocalSend, plugins.
    entry("better-files", ".local/share/better-os/files"),
    // The Open With chooser that ships with Better Files: its appearance.
    entry("better-files", ".config/better-os/app-chooser"),
    entry("better-git", ".config/better-os/git.json"),
    entry("better-git", ".local/state/better-os/git-recent.json"),
    entry("better-git", ".local/state/better-os/git-trusted"),
    // Named by the manifest; the window keeps its list in git-recent.json.
    historical("better-git", ".local/state/better-os/git-repositories.json"),
    // config.json, backup.json, safe-mode, gestures.json and its backup,
    // and the window's appearance.
    entry("better-touchpad", ".config/better-os/touchpad"),
    entry("better-launcher", ".config/better-os/launcher"),
    entry("better-launcher", ".local/state/better-os/launcher"),
    entry("better-storage", ".config/better-os/storage"),
];

/// The entries `component` owns, which removing it deletes.
pub fn owned_by(component: &str) -> Vec<&'static DataEntry> {
    DATA_DIRECTORIES
        .iter()
        .filter(|entry| entry.component == component)
        .collect()
}

/// Whether removing `component` has anything of the person's to delete,
/// which decides whether the uninstall confirmation offers the check box.
pub fn has_personal_data(component: &str) -> bool {
    !owned_by(component).is_empty()
}

/// Why a home path in a manifest is not deleted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// It climbs out (`..`), or is not a plain relative path.
    Malformed,
    /// It belongs to another component on the list.
    AnotherComponent(&'static str),
    /// It is on no list: a file the desktop and other programs share, such
    /// as `mimeapps.list`, or data whose loss the component cannot undo.
    NotOnList,
}

/// A manifest path that names the person's home and is not deleted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefusedPath {
    pub path: String,
    pub reason: Refusal,
}

impl RefusedPath {
    /// The line the window and the command line log.
    pub fn log_line(&self, component: &str) -> String {
        let reason = match self.reason {
            Refusal::Malformed => "not a plain path under the home directory".to_string(),
            Refusal::AnotherComponent(owner) => format!("belongs to {owner}"),
            Refusal::NotOnList => "not on the list of Better OS data directories".to_string(),
        };
        format!(
            "{component}: not deleting {} at removal: {reason}",
            self.path
        )
    }
}

/// The home paths in `component`'s manifest `paths` that are not its own
/// entries. Paths outside the home directory (program files, dconf keys)
/// are the package's and are not checked here.
pub fn refused_manifest_paths(component: &str, declared: &[String]) -> Vec<RefusedPath> {
    declared
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix("~/")?.trim_end_matches('/');
            let refuse = |reason| {
                Some(RefusedPath {
                    path: path.clone(),
                    reason,
                })
            };
            if relative.is_empty()
                || relative
                    .split('/')
                    .any(|segment| segment.is_empty() || segment == "." || segment == "..")
            {
                return refuse(Refusal::Malformed);
            }
            match covering(relative) {
                Some(entry) if entry.component == component => None,
                Some(entry) => refuse(Refusal::AnotherComponent(entry.component)),
                None => refuse(Refusal::NotOnList),
            }
        })
        .collect()
}

/// The entry `relative` is, or is inside.
fn covering(relative: &str) -> Option<&'static DataEntry> {
    DATA_DIRECTORIES.iter().find(|entry| {
        relative == entry.path
            || relative
                .strip_prefix(entry.path)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests() -> Vec<better_core::ComponentManifest> {
        crate::catalog::built_in_manifests()
    }

    #[test]
    fn every_manifest_home_path_is_its_own_entry_or_refused_for_a_reason() {
        for manifest in manifests() {
            let id = manifest.id.as_str();
            let refused = refused_manifest_paths(id, &manifest.paths);
            for path in manifest.paths.iter().filter(|path| path.starts_with("~/")) {
                let relative = path.trim_start_matches("~/").trim_end_matches('/');
                let own = covering(relative).is_some_and(|entry| entry.component == id);
                let rejected = refused.iter().any(|refusal| &refusal.path == path);
                assert!(own ^ rejected, "{id}: {path}");
            }
        }
    }

    #[test]
    fn removing_better_files_never_touches_shared_files_or_another_component() {
        let files = owned_by("better-files");
        for entry in &files {
            assert_eq!(entry.component, "better-files");
            for forbidden in [
                ".config/mimeapps.list",
                ".config/gtk-3.0/bookmarks",
                ".config/better-os/git.json",
                ".local/state/better-os/git-recent.json",
                ".local/state/better-os/git-trusted",
                ".local/state/better-os/manager-state.json",
                ".local/share/better-os/app-chooser/rollback",
            ] {
                assert!(
                    forbidden != entry.path && !forbidden.starts_with(&format!("{}/", entry.path)),
                    "{forbidden} under {}",
                    entry.path
                );
            }
        }
        let manifest = manifests()
            .into_iter()
            .find(|manifest| manifest.id.as_str() == "better-files")
            .unwrap();
        let refused = refused_manifest_paths("better-files", &manifest.paths);
        let names: Vec<&str> = refused
            .iter()
            .map(|refusal| refusal.path.as_str())
            .collect();
        assert!(names.contains(&"~/.config/mimeapps.list"));
        assert!(names.contains(&"~/.config/gtk-3.0/bookmarks"));
        assert!(names.contains(&"~/.local/share/better-os/app-chooser/rollback"));
        assert!(
            refused
                .iter()
                .all(|refusal| refusal.reason == Refusal::NotOnList)
        );
    }

    #[test]
    fn nothing_on_the_list_is_better_manager_s_own_state() {
        assert!(owned_by("better-manager").is_empty());
        for entry in DATA_DIRECTORIES {
            assert!(!entry.path.ends_with("better-os"), "{}", entry.path);
            assert!(!entry.path.contains("manager-state"), "{}", entry.path);
            assert!(!entry.path.contains("artifacts"), "{}", entry.path);
            assert!(!entry.path.contains("catalog"), "{}", entry.path);
        }
    }

    #[test]
    fn a_historical_entry_is_still_cleaned() {
        let git = owned_by("better-git");
        assert!(
            git.iter()
                .any(|entry| entry.historical && entry.path.ends_with("git-repositories.json"))
        );
    }

    #[test]
    fn another_component_s_path_and_a_climbing_path_are_refused() {
        let refused = refused_manifest_paths(
            "better-files",
            &[
                "~/.config/better-os/git.json".to_string(),
                "~/.config/better-os/files/../../Documents".to_string(),
                "/usr/bin/better-files".to_string(),
                "~/.config/better-os/files/view.json".to_string(),
            ],
        );
        assert_eq!(
            refused,
            [
                RefusedPath {
                    path: "~/.config/better-os/git.json".to_string(),
                    reason: Refusal::AnotherComponent("better-git"),
                },
                RefusedPath {
                    path: "~/.config/better-os/files/../../Documents".to_string(),
                    reason: Refusal::Malformed,
                },
            ]
        );
        assert!(
            refused[0]
                .log_line("better-files")
                .contains("belongs to better-git")
        );
    }

    #[test]
    fn no_two_entries_overlap_and_each_owner_is_a_shipped_component() {
        let ids: Vec<String> = manifests().iter().map(|m| m.id.to_string()).collect();
        for (i, a) in DATA_DIRECTORIES.iter().enumerate() {
            assert!(ids.contains(&a.component.to_string()), "{}", a.component);
            for b in &DATA_DIRECTORIES[i + 1..] {
                assert!(
                    a.path != b.path
                        && !a.path.starts_with(&format!("{}/", b.path))
                        && !b.path.starts_with(&format!("{}/", a.path)),
                    "{} / {}",
                    a.path,
                    b.path
                );
            }
        }
    }
}

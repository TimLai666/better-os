//! Which of a component's declared paths are the person's own data, the
//! things "Also delete my settings and history" deletes at uninstall.
//!
//! A manifest's `paths` list everything a component touches: its program
//! files, dconf keys, and files it shares with the rest of the desktop, such
//! as `~/.config/mimeapps.list`. Only some of that is the person's data for
//! that component. The rule is decided here, once, for the window and the
//! command line: a path under `~/.local/state`, `~/.config`, or `~/.cache`
//! whose first directory below that root starts with `better-`. Anything
//! else is never deleted, whatever the manifest says, because a shared file
//! such as the desktop's list of default applications belongs to the person
//! and every other program, not to the component being removed.

/// The roots personal data may live under, relative to the home directory.
pub const ROOTS: [&str; 3] = [".local/state", ".config", ".cache"];

/// One path that counts as the person's data, relative to their home
/// directory (`.config/better-os/git.json`).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PersonalPath(String);

impl PersonalPath {
    pub fn relative(&self) -> &str {
        &self.0
    }
}

/// The paths of `declared` that are the person's data, in the order the
/// manifest lists them, without duplicates.
pub fn personal_paths(declared: &[String]) -> Vec<PersonalPath> {
    let mut found: Vec<PersonalPath> = Vec::new();
    for path in declared {
        if let Some(path) = classify(path)
            && !found.contains(&path)
        {
            found.push(path);
        }
    }
    found
}

/// Whether a component declares any personal data, which is what decides
/// whether the uninstall confirmation offers the check box at all.
pub fn has_personal_data(declared: &[String]) -> bool {
    !personal_paths(declared).is_empty()
}

fn classify(path: &str) -> Option<PersonalPath> {
    let relative = path.strip_prefix("~/")?.trim_end_matches('/');
    let segments: Vec<&str> = relative.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    ROOTS.iter().find_map(|root| {
        let root_segments: Vec<&str> = root.split('/').collect();
        let below = segments.strip_prefix(root_segments.as_slice())?;
        // A component's own directory or file, never the root itself.
        let first = below.first()?;
        first
            .starts_with("better-")
            .then(|| PersonalPath(relative.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::{has_personal_data, personal_paths};

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_component_s_own_directories_under_the_three_roots_count() {
        let found = personal_paths(&paths(&[
            "~/.config/better-os/git.json",
            "~/.local/state/better-awake/awake-rules.json",
            "~/.cache/better-os/thumbnails/",
        ]));
        let relative: Vec<&str> = found.iter().map(|path| path.relative()).collect();
        assert_eq!(
            relative,
            [
                ".config/better-os/git.json",
                ".local/state/better-awake/awake-rules.json",
                ".cache/better-os/thumbnails",
            ]
        );
    }

    #[test]
    fn shared_desktop_files_and_system_paths_never_count() {
        assert!(
            personal_paths(&paths(&[
                "~/.config/mimeapps.list",
                "~/.config/gtk-3.0/bookmarks",
                "~/.local/share/better-os/files/jobs",
                "/usr/bin/better-files",
                "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/better-launcher/name",
                "~/.config",
                "~/.config/",
            ]))
            .is_empty()
        );
    }

    #[test]
    fn a_path_that_climbs_out_is_refused() {
        assert!(
            personal_paths(&paths(&[
                "~/.config/better-os/../../Documents",
                "~/.config/better-os//x",
                "~/.config/./better-os",
            ]))
            .is_empty()
        );
    }

    #[test]
    fn the_shipped_better_files_manifest_keeps_the_shared_files() {
        let manifest = better_core::ComponentManifest::parse_yaml(include_str!(
            "../../../components/manifests/better-files.yaml"
        ))
        .unwrap();
        let found = personal_paths(&manifest.paths);
        let relative: Vec<&str> = found.iter().map(|path| path.relative()).collect();
        assert_eq!(relative, [".config/better-os/files/view.json"]);
        assert!(!has_personal_data(&paths(&["/usr/bin/better-manager"])));
    }

    #[test]
    fn duplicates_are_listed_once() {
        assert_eq!(
            personal_paths(&paths(&[
                "~/.config/better-os/git.json",
                "~/.config/better-os/git.json/"
            ]))
            .len(),
            1
        );
    }
}

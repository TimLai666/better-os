//! XDG user directories: Home, Downloads, Documents, and the rest.
//!
//! These are what the sidebar's built-in section shows. The values come from
//! `$XDG_CONFIG_HOME/user-dirs.dirs`, which `xdg-user-dirs` writes and which
//! is the only place the user's actual choices are recorded — a hard-coded
//! `~/Downloads` is wrong on any localized install, where the directory is
//! named in the user's own language.
//!
//! A directory that does not exist is still returned, marked absent. Dropping
//! it would make the sidebar's contents depend on whether a folder happened to
//! have been created, and Issue #6 asks for missing locations to be indicated
//! rather than silently removed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use files_core::location::{LocalPath, Location};

/// The user directories this build knows about.
///
/// A closed set: these are the keys `xdg-user-dirs` defines. An unrecognized
/// key in the configuration file is ignored rather than turned into a sidebar
/// row with a machine name on it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UserDirectory {
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    Templates,
    PublicShare,
}

impl UserDirectory {
    /// The key used in `user-dirs.dirs`.
    pub fn key(self) -> &'static str {
        match self {
            UserDirectory::Home => "XDG_HOME_DIR",
            UserDirectory::Desktop => "XDG_DESKTOP_DIR",
            UserDirectory::Documents => "XDG_DOCUMENTS_DIR",
            UserDirectory::Downloads => "XDG_DOWNLOAD_DIR",
            UserDirectory::Music => "XDG_MUSIC_DIR",
            UserDirectory::Pictures => "XDG_PICTURES_DIR",
            UserDirectory::Videos => "XDG_VIDEOS_DIR",
            UserDirectory::Templates => "XDG_TEMPLATES_DIR",
            UserDirectory::PublicShare => "XDG_PUBLICSHARE_DIR",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [
            UserDirectory::Desktop,
            UserDirectory::Documents,
            UserDirectory::Downloads,
            UserDirectory::Music,
            UserDirectory::Pictures,
            UserDirectory::Videos,
            UserDirectory::Templates,
            UserDirectory::PublicShare,
        ]
        .into_iter()
        .find(|directory| directory.key() == key)
    }

    /// The order the sidebar lists them in.
    pub const SIDEBAR_ORDER: [UserDirectory; 9] = [
        UserDirectory::Home,
        UserDirectory::Desktop,
        UserDirectory::Documents,
        UserDirectory::Downloads,
        UserDirectory::Music,
        UserDirectory::Pictures,
        UserDirectory::Videos,
        UserDirectory::Templates,
        UserDirectory::PublicShare,
    ];
}

/// One resolved directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDirectory {
    pub directory: UserDirectory,
    pub location: Location,
    /// Whether the directory is actually there. A sidebar shows an absent one
    /// as unavailable rather than dropping the row.
    pub present: bool,
}

/// Every user directory for one session.
#[derive(Clone, Debug, Default)]
pub struct UserDirectories {
    resolved: BTreeMap<UserDirectory, ResolvedDirectory>,
}

impl UserDirectories {
    /// Reads the directories from the process environment.
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.as_ref().map(|home| home.join(".config")));
        Self::from_values(home.as_deref(), config_home.as_deref())
    }

    /// Builds from explicit values, which is the only reason this is testable
    /// without changing the process environment.
    pub fn from_values(home: Option<&Path>, config_home: Option<&Path>) -> Self {
        let mut resolved = BTreeMap::new();
        let Some(home) = home.filter(|path| path.is_absolute()) else {
            // With no home directory there is nothing to resolve against.
            // An empty set is the honest answer; defaulting to `/` would put
            // the filesystem root in the sidebar labelled "Home".
            return Self { resolved };
        };
        if let Ok(location) = LocalPath::new(home) {
            insert(&mut resolved, UserDirectory::Home, location);
        }

        let configured = config_home
            .map(|config| config.join("user-dirs.dirs"))
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|contents| parse_user_dirs(&contents, home))
            .unwrap_or_default();

        for directory in UserDirectory::SIDEBAR_ORDER {
            if directory == UserDirectory::Home {
                continue;
            }
            let path = match configured.get(&directory) {
                Some(path) => path.clone(),
                // The specification's fallback is the English name under the
                // home directory. Used only when the file did not say.
                None => home.join(default_name(directory)),
            };
            if let Ok(location) = LocalPath::new(path) {
                insert(&mut resolved, directory, location);
            }
        }
        Self { resolved }
    }

    pub fn get(&self, directory: UserDirectory) -> Option<&ResolvedDirectory> {
        self.resolved.get(&directory)
    }

    pub fn location(&self, directory: UserDirectory) -> Option<&Location> {
        self.resolved
            .get(&directory)
            .map(|resolved| &resolved.location)
    }

    pub fn home(&self) -> Option<&Location> {
        self.location(UserDirectory::Home)
    }

    /// The sidebar's built-in section, in order.
    pub fn sidebar(&self) -> Vec<&ResolvedDirectory> {
        UserDirectory::SIDEBAR_ORDER
            .iter()
            .filter_map(|directory| self.resolved.get(directory))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }
}

fn insert(
    resolved: &mut BTreeMap<UserDirectory, ResolvedDirectory>,
    directory: UserDirectory,
    path: LocalPath,
) {
    let present = path.as_path().is_dir();
    resolved.insert(
        directory,
        ResolvedDirectory {
            directory,
            location: Location::Local(path),
            present,
        },
    );
}

fn default_name(directory: UserDirectory) -> &'static str {
    match directory {
        UserDirectory::Home => "",
        UserDirectory::Desktop => "Desktop",
        UserDirectory::Documents => "Documents",
        UserDirectory::Downloads => "Downloads",
        UserDirectory::Music => "Music",
        UserDirectory::Pictures => "Pictures",
        UserDirectory::Videos => "Videos",
        UserDirectory::Templates => "Templates",
        UserDirectory::PublicShare => "Public",
    }
}

/// Parses `user-dirs.dirs`, whose lines read `XDG_X_DIR="$HOME/Name"`.
fn parse_user_dirs(contents: &str, home: &Path) -> BTreeMap<UserDirectory, PathBuf> {
    let mut values = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(directory) = UserDirectory::from_key(key.trim()) else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        let path = match value.strip_prefix("$HOME/") {
            Some(relative) => home.join(relative),
            None if value == "$HOME" => home.to_path_buf(),
            None => PathBuf::from(value),
        };
        // A value that is not absolute after expansion names nothing usable.
        // Skipped, so the specification default applies instead.
        if path.is_absolute() {
            values.insert(directory, path);
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_directories_win_over_the_english_defaults() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/user");
        let config = home.join(".config");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(home.join("下載")).unwrap();
        fs::write(
            config.join("user-dirs.dirs"),
            "# generated\nXDG_DOWNLOAD_DIR=\"$HOME/下載\"\nXDG_DESKTOP_DIR=\"$HOME/桌面\"\n",
        )
        .unwrap();

        let directories = UserDirectories::from_values(Some(&home), Some(&config));
        let downloads = directories.get(UserDirectory::Downloads).unwrap();
        assert_eq!(
            downloads.location.as_local_path().unwrap().as_path(),
            home.join("下載")
        );
        assert!(downloads.present);
    }

    #[test]
    fn an_absent_directory_is_still_listed_and_marked_absent() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/user");
        fs::create_dir_all(&home).unwrap();
        let directories = UserDirectories::from_values(Some(&home), None);
        let documents = directories.get(UserDirectory::Documents).unwrap();
        assert!(!documents.present);
        assert_eq!(
            documents.location.as_local_path().unwrap().as_path(),
            home.join("Documents")
        );
    }

    #[test]
    fn the_sidebar_lists_home_first_and_keeps_a_fixed_order() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/user");
        fs::create_dir_all(&home).unwrap();
        let directories = UserDirectories::from_values(Some(&home), None);
        let order: Vec<UserDirectory> = directories
            .sidebar()
            .iter()
            .map(|resolved| resolved.directory)
            .collect();
        assert_eq!(order, UserDirectory::SIDEBAR_ORDER);
    }

    #[test]
    fn a_session_without_a_home_directory_resolves_nothing() {
        let directories = UserDirectories::from_values(None, None);
        assert!(directories.is_empty());
        assert_eq!(directories.home(), None);
    }

    #[test]
    fn a_relative_or_unknown_entry_falls_back_to_the_default() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/user");
        let config = home.join(".config");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("user-dirs.dirs"),
            "XDG_MUSIC_DIR=\"relative/Music\"\nXDG_MADE_UP_DIR=\"$HOME/Nope\"\n",
        )
        .unwrap();
        let directories = UserDirectories::from_values(Some(&home), Some(&config));
        assert_eq!(
            directories
                .location(UserDirectory::Music)
                .and_then(Location::as_local_path)
                .map(LocalPath::as_path),
            Some(home.join("Music").as_path())
        );
    }

    #[test]
    fn an_absolute_path_outside_home_is_honored() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home/user");
        let config = home.join(".config");
        fs::create_dir_all(&config).unwrap();
        let external = root.path().join("pool/pictures");
        fs::create_dir_all(&external).unwrap();
        fs::write(
            config.join("user-dirs.dirs"),
            format!("XDG_PICTURES_DIR=\"{}\"\n", external.display()),
        )
        .unwrap();
        let directories = UserDirectories::from_values(Some(&home), Some(&config));
        let pictures = directories.get(UserDirectory::Pictures).unwrap();
        assert_eq!(
            pictures.location.as_local_path().unwrap().as_path(),
            external
        );
        assert!(pictures.present);
    }
}

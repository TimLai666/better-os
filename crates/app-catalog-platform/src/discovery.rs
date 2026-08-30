//! XDG application-directory discovery.
//!
//! The directory list and its order are the whole precedence rule: the first
//! directory holding a desktop ID owns that ID, and everything found later
//! under the same ID is shadowed. `$XDG_DATA_HOME` comes first, then each
//! `$XDG_DATA_DIRS` entry in the order the variable lists them.

use std::fs;
use std::path::{Path, PathBuf};

use app_catalog_core::{
    Catalog, CatalogBuilder, DesktopId, DirectoryRank, EntryError, EntryScope, ExecutableProbe,
};

/// The fallback `$XDG_DATA_DIRS` value the Base Directory Specification
/// defines.
pub const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";

/// How deep a subdirectory tree under an application directory is walked.
/// Real entries nest one level (`kde4/`, vendor prefixes); the cap stops a
/// symlink loop from turning discovery into an infinite walk.
pub const MAX_SUBDIRECTORY_DEPTH: usize = 6;

/// One directory that may contain desktop entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationDirectory {
    pub path: PathBuf,
    /// Precedence, lowest wins.
    pub rank: usize,
    pub scope: EntryScope,
}

impl ApplicationDirectory {
    fn directory_rank(&self) -> DirectoryRank {
        DirectoryRank {
            rank: self.rank,
            scope: self.scope,
        }
    }
}

/// The ordered application directories for one session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplicationDirectories {
    directories: Vec<ApplicationDirectory>,
}

impl ApplicationDirectories {
    /// Builds the list from the environment, exactly as the Base Directory
    /// Specification says: `$XDG_DATA_HOME` or `~/.local/share`, then
    /// `$XDG_DATA_DIRS` or its default.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .as_deref(),
            std::env::var_os("HOME").map(PathBuf::from).as_deref(),
            std::env::var("XDG_DATA_DIRS").ok().as_deref(),
        )
    }

    /// Builds the list from explicit values. Discovery is testable only
    /// because this exists: a test never has to change the process
    /// environment.
    pub fn from_values(
        data_home: Option<&Path>,
        home: Option<&Path>,
        data_dirs: Option<&str>,
    ) -> Self {
        let mut directories = Vec::new();
        let user_base = match data_home {
            Some(path) if path.is_absolute() => Some(path.to_path_buf()),
            // A relative `$XDG_DATA_HOME` is invalid and is ignored, per the
            // specification, rather than being resolved against the process's
            // working directory.
            Some(_) | None => home.map(|home| home.join(".local").join("share")),
        };
        if let Some(base) = user_base {
            directories.push(ApplicationDirectory {
                path: base.join("applications"),
                rank: 0,
                scope: EntryScope::User,
            });
        }
        let data_dirs = match data_dirs {
            Some(value) if !value.trim().is_empty() => value,
            _ => DEFAULT_DATA_DIRS,
        };
        for raw in data_dirs.split(':') {
            let path = Path::new(raw);
            if raw.is_empty() || !path.is_absolute() {
                continue;
            }
            let path = path.join("applications");
            if directories.iter().any(|existing| existing.path == path) {
                continue;
            }
            directories.push(ApplicationDirectory {
                path,
                rank: directories.len(),
                scope: EntryScope::System,
            });
        }
        Self { directories }
    }

    pub fn new(directories: Vec<ApplicationDirectory>) -> Self {
        Self { directories }
    }

    pub fn directories(&self) -> &[ApplicationDirectory] {
        &self.directories
    }

    pub fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }
}

/// Reads every directory in order and assembles a catalog. A directory that
/// does not exist is not an error: most systems have no user application
/// directory until something writes one.
pub fn discover(directories: &ApplicationDirectories, probe: &dyn ExecutableProbe) -> Catalog {
    let mut builder = CatalogBuilder::new(probe);
    for directory in directories.directories() {
        let rank = directory.directory_rank();
        scan_directory(&directory.path, &directory.path, 0, &rank, &mut builder);
    }
    builder.build()
}

fn scan_directory(
    root: &Path,
    current: &Path,
    depth: usize,
    rank: &DirectoryRank,
    builder: &mut CatalogBuilder<'_>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // `file_type` does not follow symlinks, so a symlinked directory is
        // resolved explicitly and still counts against the depth cap.
        let is_directory = if file_type.is_symlink() {
            fs::metadata(&path)
                .map(|meta| meta.is_dir())
                .unwrap_or(false)
        } else {
            file_type.is_dir()
        };
        if is_directory {
            if depth < MAX_SUBDIRECTORY_DEPTH {
                scan_directory(root, &path, depth + 1, rank, builder);
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let desktop_id = match DesktopId::from_relative_path(relative) {
            Ok(id) => id,
            Err(error) => {
                builder.reject(path, None, error);
                continue;
            }
        };
        match fs::read(&path) {
            Ok(bytes) => builder.add_entry(desktop_id, path, rank, &bytes),
            Err(error) => builder.reject(
                path,
                Some(desktop_id),
                EntryError::Unreadable(error.kind().to_string()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_directory_outranks_system_directories_in_order() {
        let directories = ApplicationDirectories::from_values(
            Some(Path::new("/home/user/.local/share")),
            Some(Path::new("/home/user")),
            Some("/opt/extra/share:/usr/share"),
        );
        let paths: Vec<(&str, usize)> = directories
            .directories()
            .iter()
            .map(|directory| (directory.path.to_str().unwrap(), directory.rank))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("/home/user/.local/share/applications", 0),
                ("/opt/extra/share/applications", 1),
                ("/usr/share/applications", 2),
            ]
        );
        assert_eq!(directories.directories()[0].scope, EntryScope::User);
        assert_eq!(directories.directories()[1].scope, EntryScope::System);
    }

    #[test]
    fn missing_data_dirs_falls_back_to_the_specified_default() {
        let directories =
            ApplicationDirectories::from_values(None, Some(Path::new("/home/user")), None);
        let paths: Vec<&str> = directories
            .directories()
            .iter()
            .map(|directory| directory.path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                "/home/user/.local/share/applications",
                "/usr/local/share/applications",
                "/usr/share/applications",
            ]
        );
    }

    #[test]
    fn relative_and_duplicate_directories_are_ignored() {
        let directories = ApplicationDirectories::from_values(
            Some(Path::new("relative/share")),
            Some(Path::new("/home/user")),
            Some("relative:/usr/share::/usr/share"),
        );
        let paths: Vec<&str> = directories
            .directories()
            .iter()
            .map(|directory| directory.path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            vec![
                "/home/user/.local/share/applications",
                "/usr/share/applications",
            ]
        );
    }

    #[test]
    fn no_home_and_no_data_dirs_still_yields_the_system_defaults() {
        let directories = ApplicationDirectories::from_values(None, None, None);
        assert_eq!(directories.directories().len(), 2);
        assert_eq!(directories.directories()[0].rank, 0);
    }
}

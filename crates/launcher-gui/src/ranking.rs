//! What a search shows, in order: a calculation, then files and folders when
//! the query is a path, then the applications `launcher-core` ranked.
//!
//! The applications' order is `launcher-core`'s and is not touched here:
//! names beginning with the query, then a later word of the name, then
//! keywords and the generic name, then fuzzy name matches. This module adds
//! the two kinds of answer Windows 11's Start search gives that are not
//! applications — a calculator result for `12*(3+4)`, and "open this file or
//! folder" for `~/Documents` or `/etc/hosts` — and decides where they go.
//!
//! The filesystem is reached through [`PathProbe`], so every rule here is
//! tested against a fake tree.

use std::path::{Path, PathBuf};

use launcher_core::{LauncherApplication, NoUsage, RankingOptions, SearchIndex};

use crate::calculator::{Calculation, evaluate};

/// How many applications a search lists. Windows shows a short list; past
/// this a person types another letter.
pub const APP_RESULTS: usize = 24;

/// How many files and folders a path query lists.
pub const PATH_RESULTS: usize = 6;

/// How many entries a directory is read for, so a path query into a huge
/// directory stays one keystroke's work.
pub const DIRECTORY_SCAN_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum PathKind {
    Folder,
    File,
}

/// A file or folder a path query found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathHit {
    pub path: PathBuf,
    pub kind: PathKind,
}

impl PathHit {
    /// The name shown on the row: the last component, or the path itself
    /// for `/`.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// A path as a person reads it: the home folder written `~`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// One search result.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchItem {
    Calculation(Calculation),
    Path(PathHit),
    App(LauncherApplication),
}

impl SearchItem {
    pub fn application(&self) -> Option<&LauncherApplication> {
        match self {
            SearchItem::App(application) => Some(application),
            _ => None,
        }
    }
}

/// What a path query may ask of the filesystem.
pub trait PathProbe {
    /// Whether the path is a folder, a file, or nothing.
    fn kind(&self, path: &Path) -> Option<PathKind>;
    /// A folder's entries, at most `limit` of them, in any order.
    fn children(&self, folder: &Path, limit: usize) -> Vec<(String, PathKind)>;
    /// The home folder `~` stands for.
    fn home(&self) -> Option<PathBuf>;
}

/// No filesystem at all: what the render thread searches with, so a typed
/// path never blocks a frame. Files and folders come from a background read
/// ([`path_hits_within`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPaths;

impl PathProbe for NoPaths {
    fn kind(&self, _: &Path) -> Option<PathKind> {
        None
    }

    fn children(&self, _: &Path, _: usize) -> Vec<(String, PathKind)> {
        Vec::new()
    }

    fn home(&self) -> Option<PathBuf> {
        None
    }
}

/// How long a files-and-folders read may take before it is given up: a dead
/// network mount can block `stat` for minutes.
pub const PATH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);

/// [`path_hits`] on a thread of its own, waited for at most `timeout`. A read
/// that does not finish in time is abandoned (its thread ends whenever the
/// filesystem answers) and the search shows no files rather than freezing.
pub fn path_hits_within(
    query: String,
    probe: std::sync::Arc<dyn PathProbe + Send + Sync>,
    timeout: std::time::Duration,
) -> Option<Vec<PathHit>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("launcher-paths".into())
        .spawn(move || {
            let _ = sender.send(path_hits(&query, probe.as_ref()));
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()
}

/// Puts files and folders into a result list: after a calculation, before
/// the applications, replacing any already there.
pub fn insert_paths(items: &mut Vec<SearchItem>, hits: Vec<PathHit>) {
    items.retain(|item| !matches!(item, SearchItem::Path(_)));
    let at = items
        .iter()
        .take_while(|item| matches!(item, SearchItem::Calculation(_)))
        .count();
    for (offset, hit) in hits.into_iter().enumerate() {
        items.insert(at + offset, SearchItem::Path(hit));
    }
}

/// The real filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileSystem;

impl PathProbe for FileSystem {
    fn kind(&self, path: &Path) -> Option<PathKind> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(if metadata.is_dir() {
            PathKind::Folder
        } else {
            PathKind::File
        })
    }

    fn children(&self, folder: &Path, limit: usize) -> Vec<(String, PathKind)> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .take(limit)
            .map(|entry| {
                let folder = entry
                    .file_type()
                    .map(|kind| kind.is_dir() || (kind.is_symlink() && entry.path().is_dir()))
                    .unwrap_or(false);
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    if folder {
                        PathKind::Folder
                    } else {
                        PathKind::File
                    },
                )
            })
            .collect()
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
    }
}

/// The path a query names, when it names one: it begins with `/` or `~`.
/// A trailing `/` is dropped from the path; [`path_hits`] reads it from the
/// query.
pub fn query_path(query: &str, probe: &dyn PathProbe) -> Option<PathBuf> {
    let query = query.trim();
    if query.starts_with('/') {
        return Some(PathBuf::from(query).components().collect());
    }
    let rest = query.strip_prefix('~')?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        // `~user` is another person's home; not offered.
        return None;
    }
    let home = probe.home()?;
    Some(
        home.join(rest.trim_start_matches('/'))
            .components()
            .collect(),
    )
}

/// The files and folders a path query offers: the path itself when it
/// exists, then what the folder it is typed into holds and begins with what
/// was typed after the last `/`, folders first, by name. Hidden entries are
/// offered only once a `.` is typed.
pub fn path_hits(query: &str, probe: &dyn PathProbe) -> Vec<PathHit> {
    let Some(path) = query_path(query, probe) else {
        return Vec::new();
    };
    let typed = query.trim();
    let mut hits = Vec::new();
    let exact = probe.kind(&path);
    if let Some(kind) = exact {
        hits.push(PathHit {
            path: path.clone(),
            kind,
        });
    }
    // Typed into a folder (`~/Documents/`): what it holds. Otherwise, the
    // siblings that begin with the last part (`~/Doc`).
    let (folder, prefix) = if typed.ends_with('/') && exact == Some(PathKind::Folder) {
        (path.clone(), String::new())
    } else {
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => {
                (parent.to_path_buf(), name.to_string_lossy().into_owned())
            }
            _ => return hits,
        }
    };
    let prefix = prefix.to_lowercase();
    let mut children: Vec<(String, PathKind)> = probe
        .children(&folder, DIRECTORY_SCAN_LIMIT)
        .into_iter()
        .filter(|(name, _)| name.to_lowercase().starts_with(&prefix))
        .filter(|(name, _)| prefix.starts_with('.') || !name.starts_with('.'))
        .filter(|(name, _)| folder.join(name) != path)
        .collect();
    children.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    for (name, kind) in children {
        if hits.len() >= PATH_RESULTS {
            break;
        }
        hits.push(PathHit {
            path: folder.join(name),
            kind,
        });
    }
    hits
}

/// Everything a query shows, in order.
pub fn search(query: &str, index: &SearchIndex, probe: &dyn PathProbe) -> Vec<SearchItem> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    if let Some(calculation) = evaluate(query) {
        items.push(SearchItem::Calculation(calculation));
    }
    items.extend(path_hits(query, probe).into_iter().map(SearchItem::Path));
    let options = RankingOptions {
        usage_weighting: false,
        limit: Some(APP_RESULTS),
    };
    items.extend(
        index
            .search(query, &options, &NoUsage)
            .results()
            .iter()
            .map(|result| SearchItem::App(result.application.clone())),
    );
    items
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A tree of folders and files, as `path -> kind`.
    #[derive(Default)]
    pub(crate) struct FakeTree {
        pub entries: BTreeMap<PathBuf, PathKind>,
        pub home: Option<PathBuf>,
    }

    impl FakeTree {
        pub(crate) fn with(paths: &[(&str, PathKind)]) -> Self {
            Self {
                entries: paths
                    .iter()
                    .map(|(path, kind)| (PathBuf::from(path), *kind))
                    .collect(),
                home: Some(PathBuf::from("/home/someone")),
            }
        }
    }

    impl PathProbe for FakeTree {
        fn kind(&self, path: &Path) -> Option<PathKind> {
            self.entries.get(path).copied()
        }

        fn children(&self, folder: &Path, limit: usize) -> Vec<(String, PathKind)> {
            self.entries
                .iter()
                .filter(|(path, _)| path.parent() == Some(folder))
                .take(limit)
                .map(|(path, kind)| {
                    (
                        path.file_name().unwrap().to_string_lossy().into_owned(),
                        *kind,
                    )
                })
                .collect()
        }

        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    fn tree() -> FakeTree {
        FakeTree::with(&[
            ("/home/someone", PathKind::Folder),
            ("/home/someone/Documents", PathKind::Folder),
            ("/home/someone/Downloads", PathKind::Folder),
            ("/home/someone/doc-notes.txt", PathKind::File),
            ("/home/someone/.config", PathKind::Folder),
            ("/home/someone/Documents/report.odt", PathKind::File),
            ("/etc", PathKind::Folder),
            ("/etc/hosts", PathKind::File),
        ])
    }

    fn names(hits: &[PathHit]) -> Vec<String> {
        hits.iter()
            .map(|hit| hit.path.display().to_string())
            .collect()
    }

    #[test]
    fn only_a_query_starting_with_a_slash_or_a_tilde_is_a_path() {
        let tree = tree();
        assert_eq!(query_path("cal", &tree), None);
        assert_eq!(query_path("~someone", &tree), None);
        assert_eq!(query_path("/etc", &tree), Some("/etc".into()));
        assert_eq!(query_path("~", &tree), Some("/home/someone".into()));
        assert_eq!(
            query_path("~/Documents", &tree),
            Some("/home/someone/Documents".into())
        );
    }

    #[test]
    fn an_existing_path_comes_first_then_what_begins_with_the_last_part() {
        let tree = tree();
        assert_eq!(names(&path_hits("/etc/hosts", &tree)), ["/etc/hosts"]);
        assert_eq!(path_hits("/etc/hosts", &tree)[0].kind, PathKind::File);
        assert_eq!(
            names(&path_hits("~/do", &tree)),
            [
                "/home/someone/Documents",
                "/home/someone/Downloads",
                "/home/someone/doc-notes.txt"
            ],
            "folders first, case does not matter"
        );
        assert_eq!(
            names(&path_hits("~/Documents", &tree)),
            ["/home/someone/Documents"],
            "the exact folder, and no sibling repeats it"
        );
        assert_eq!(
            names(&path_hits("~/Documents/", &tree)),
            [
                "/home/someone/Documents",
                "/home/someone/Documents/report.odt"
            ],
            "a trailing slash lists what the folder holds"
        );
    }

    #[test]
    fn hidden_entries_wait_for_a_dot_and_missing_paths_find_nothing() {
        let tree = tree();
        assert!(!names(&path_hits("~/", &tree)).contains(&"/home/someone/.config".to_string()));
        assert_eq!(names(&path_hits("~/.c", &tree)), ["/home/someone/.config"]);
        assert!(path_hits("/nowhere/at/all", &tree).is_empty());
        let homeless = FakeTree { home: None, ..tree };
        assert!(path_hits("~/Documents", &homeless).is_empty());
    }

    #[test]
    fn the_home_folder_is_written_as_a_tilde() {
        let home = Path::new("/home/someone");
        assert_eq!(display_path(Path::new("/home/someone"), Some(home)), "~");
        assert_eq!(
            display_path(Path::new("/home/someone/Documents/a.odt"), Some(home)),
            "~/Documents/a.odt"
        );
        assert_eq!(
            display_path(Path::new("/etc/hosts"), Some(home)),
            "/etc/hosts"
        );
        assert_eq!(
            display_path(Path::new("/home/someoneelse"), Some(home)),
            "/home/someoneelse"
        );
        assert_eq!(display_path(Path::new("/etc"), None), "/etc");
    }

    /// A filesystem whose every call blocks, like a dead NFS mount.
    struct Hung;

    impl PathProbe for Hung {
        fn kind(&self, _: &Path) -> Option<PathKind> {
            std::thread::sleep(std::time::Duration::from_secs(5));
            None
        }
        fn children(&self, _: &Path, _: usize) -> Vec<(String, PathKind)> {
            Vec::new()
        }
        fn home(&self) -> Option<PathBuf> {
            None
        }
    }

    #[test]
    fn a_hung_filesystem_is_given_up_on_instead_of_freezing_the_search() {
        let started = std::time::Instant::now();
        let hits = path_hits_within(
            "/mnt/dead".into(),
            std::sync::Arc::new(Hung),
            std::time::Duration::from_millis(100),
        );
        assert_eq!(hits, None);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let found = path_hits_within(
            "/etc/hosts".into(),
            std::sync::Arc::new(tree()),
            std::time::Duration::from_secs(5),
        );
        assert_eq!(found.map(|hits| hits.len()), Some(1));
        assert!(path_hits("/etc/hosts", &NoPaths).is_empty());
    }

    #[test]
    fn files_go_after_a_calculation_and_before_applications() {
        let hit = |path: &str| PathHit {
            path: PathBuf::from(path),
            kind: PathKind::File,
        };
        let calculation = crate::calculator::evaluate("1+1").unwrap();
        let mut items = vec![SearchItem::Calculation(calculation.clone())];
        insert_paths(&mut items, vec![hit("/a"), hit("/b")]);
        insert_paths(&mut items, vec![hit("/c")]);
        assert_eq!(
            items,
            [
                SearchItem::Calculation(calculation),
                SearchItem::Path(hit("/c"))
            ]
        );
    }

    #[test]
    fn a_path_query_lists_at_most_a_handful() {
        let paths: Vec<(String, PathKind)> = (0..20)
            .map(|n| (format!("/many/file{n:02}"), PathKind::File))
            .collect();
        let mut tree = FakeTree::with(&[("/many", PathKind::Folder)]);
        for (path, kind) in &paths {
            tree.entries.insert(PathBuf::from(path), *kind);
        }
        assert_eq!(path_hits("/many/", &tree).len(), PATH_RESULTS);
    }

    pub(crate) fn index_of(entries: &[(&str, &str)]) -> SearchIndex {
        use app_catalog_core::{ApplicationRecord, DesktopFile, DesktopId, EntryScope, NoProbe};
        let records: Vec<ApplicationRecord> = entries
            .iter()
            .map(|(id, body)| {
                let file =
                    DesktopFile::parse(&format!("[Desktop Entry]\nType=Application\n{body}\n"))
                        .unwrap();
                ApplicationRecord::from_desktop_file(
                    DesktopId::new(*id).unwrap(),
                    PathBuf::from(format!("/usr/share/applications/{id}")),
                    EntryScope::System,
                    &file,
                    &NoProbe,
                )
                .unwrap()
            })
            .collect();
        SearchIndex::build(&records, &launcher_core::IndexOptions::new())
    }

    #[test]
    fn a_calculation_comes_first_then_paths_then_applications() {
        let index = index_of(&[
            ("calc.desktop", "Name=Calculator\nExec=calc"),
            ("files.desktop", "Name=Files\nExec=files"),
        ]);
        let tree = tree();
        let items = search("12*(3+4)", &index, &tree);
        assert!(matches!(&items[0], SearchItem::Calculation(c) if c.display == "84"));
        assert_eq!(items.len(), 1);

        let items = search("~/Doc", &index, &tree);
        assert!(matches!(&items[0], SearchItem::Path(hit) if hit.kind == PathKind::Folder));

        let items = search("cal", &index, &tree);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].application().map(|a| a.display_name.as_str()),
            Some("Calculator")
        );
        assert!(search("  ", &index, &tree).is_empty());
    }
}

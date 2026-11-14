//! The repository list, as GitHub Desktop keeps it: every repository the
//! person added, each with an optional alias, searchable, and marked missing
//! when its folder is gone. It lives in
//! `$XDG_STATE_HOME/better-os/git-repositories.json`, written beside its
//! name and renamed, and re-read before every write so two windows do not
//! lose each other's additions. The recently opened order is `recent`'s.
//!
//! The list holds repositories, not folders: a linked worktree is the same
//! repository as its main worktree, because they share one git common
//! directory. Opening a worktree saves its repository, and an older list
//! that saved a linked worktree as a repository of its own is folded into
//! that repository once Git has said which one it is
//! ([`RepositoryList::merge_worktrees`]). [`RepositoryList::groups`] draws
//! each repository once with its worktrees under it.

use std::io;
use std::path::{Path, PathBuf};

use git_core::Worktree;
use serde::{Deserialize, Serialize};

use crate::worktree_links::same_folder;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Entry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl Entry {
    /// The name the list shows: the alias, or the folder's name.
    pub fn name(&self) -> String {
        self.alias
            .clone()
            .unwrap_or_else(|| folder_name(&self.path))
    }
}

pub fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Document {
    repositories: Vec<Entry>,
}

/// One row of the repository list, ready to draw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    /// The folder, for the caption.
    pub location: String,
    /// The folder is gone (moved or deleted); the row offers to locate or
    /// remove it.
    pub missing: bool,
    pub current: bool,
    pub recent: bool,
}

/// What Git last said about a folder in the list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Known {
    /// The git directory every worktree of the repository shares
    /// (`git rev-parse --git-common-dir`), when it could be read.
    pub common_dir: Option<PathBuf>,
    /// The repository's worktrees, the main one first.
    pub worktrees: Vec<Worktree>,
}

impl Known {
    /// The main worktree when it is a working tree that is there: not a bare
    /// repository, and not the common directory itself (which is what Git
    /// lists as the main entry when the repository's git directory is kept
    /// apart from its working tree, or when that working tree is gone).
    pub fn main_worktree(&self) -> Option<&Worktree> {
        self.worktrees.first().filter(|w| {
            w.main && !w.bare && !w.prunable && self.common_dir.as_deref() != Some(w.path.as_path())
        })
    }

    /// The folder that stands for the repository in the list: its main
    /// worktree, or its common directory when it has no main worktree to
    /// show (a bare repository, or one whose main worktree is gone). `None`
    /// when Git said nothing.
    pub fn repository(&self) -> Option<PathBuf> {
        if let Some(main) = self.main_worktree() {
            return Some(main.path.clone());
        }
        self.common_dir
            .clone()
            .or_else(|| self.worktrees.first().map(|w| w.path.clone()))
    }

    /// What tells two folders apart as repositories: the common directory,
    /// or the repository's folder when the common directory is not known.
    fn key(&self) -> Option<PathBuf> {
        self.common_dir.clone().or_else(|| self.repository())
    }
}

/// One repository in the list, with its worktrees, ready to draw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group {
    /// The saved entry the row stands for: what an alias, locating, or
    /// removing acts on.
    pub path: PathBuf,
    /// Every saved entry that is this repository (one, once merged).
    pub entries: Vec<PathBuf>,
    pub name: String,
    /// The folder the repository's folder is in, for the caption.
    pub location: String,
    /// The folder the repository row opens: the main worktree, or the first
    /// worktree that is there when it has none. `None` for a row that only
    /// heads its worktrees (a bare repository with none of them left).
    pub opens: Option<PathBuf>,
    /// The entry's folder is gone and Git knows nothing about it; the row
    /// offers to locate or remove it.
    pub missing: bool,
    /// The folder open in the window is this repository or one of its
    /// worktrees.
    pub current: bool,
    pub recent: bool,
    /// Its worktrees, the main one first, when it has any besides the main
    /// one; empty otherwise.
    pub worktrees: Vec<WorktreeRow>,
}

/// One worktree under its repository.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeRow {
    pub path: PathBuf,
    /// Its folder's name.
    pub name: String,
    /// The branch checked out there, or `None` when it is detached.
    pub branch: Option<String>,
    /// The commit it is on, shortened, for a detached one.
    pub short_sha: Option<String>,
    pub main: bool,
    /// It is the folder open in the window.
    pub current: bool,
    /// Its folder is gone; Git would prune it.
    pub missing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RepositoryList {
    file: Option<PathBuf>,
    pub entries: Vec<Entry>,
}

impl RepositoryList {
    pub fn default_file() -> Option<PathBuf> {
        crate::recent::default_path().map(|p| p.with_file_name("git-repositories.json"))
    }

    /// Reads the list; a missing or unreadable file is empty.
    pub fn load(file: Option<PathBuf>) -> RepositoryList {
        let entries = file
            .as_deref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Document>(&b).ok())
            .map(|d| d.repositories)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.path.is_absolute())
            .collect();
        RepositoryList { file, entries }
    }

    fn merge_disk(&mut self) {
        let disk = RepositoryList::load(self.file.clone());
        for entry in disk.entries {
            if !self.entries.iter().any(|e| e.path == entry.path) {
                self.entries.push(entry);
            }
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let document = Document {
            repositories: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
        let partial = file.with_extension("json.partial");
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, file)
    }

    /// Adds a repository (no-op when it is there).
    pub fn add(&mut self, path: &Path) -> io::Result<()> {
        self.merge_disk();
        if !self.entries.iter().any(|e| e.path == path) {
            self.entries.push(Entry {
                path: path.to_path_buf(),
                alias: None,
            });
        }
        self.save()
    }

    /// Removes a repository from the list; its folder is left alone.
    pub fn remove(&mut self, path: &Path) -> io::Result<()> {
        self.merge_disk();
        self.entries.retain(|e| e.path != path);
        self.save()
    }

    /// Sets or clears (blank) an alias.
    pub fn set_alias(&mut self, path: &Path, alias: &str) -> io::Result<()> {
        self.merge_disk();
        let alias = alias.trim();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.alias = (!alias.is_empty()).then(|| alias.to_string());
        }
        self.save()
    }

    /// Points a missing repository's entry at the folder it moved to.
    pub fn relocate(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.merge_disk();
        let alias = self
            .entries
            .iter()
            .find(|e| e.path == from)
            .and_then(|e| e.alias.clone());
        self.entries.retain(|e| e.path != from && e.path != to);
        self.entries.push(Entry {
            path: to.to_path_buf(),
            alias,
        });
        self.save()
    }

    /// Saves the repository of a folder that was opened: `folder` is the
    /// worktree, `repository` what [`Known::repository`] said. A saved entry
    /// for the worktree itself becomes the repository's.
    pub fn add_opened(&mut self, folder: &Path, repository: &Path) -> io::Result<()> {
        self.merge_disk();
        self.fold(folder, repository);
        if !self.entries.iter().any(|e| e.path == repository) {
            self.entries.push(Entry {
                path: repository.to_path_buf(),
                alias: None,
            });
        }
        self.save()
    }

    /// Folds saved entries that are worktrees of another entry's repository
    /// into it, from `(entry, repository)` pairs Git answered. Writes only
    /// when something changed; says whether it did.
    pub fn merge_worktrees(&mut self, pairs: &[(PathBuf, PathBuf)]) -> io::Result<bool> {
        if !pairs.iter().any(|(entry, repository)| {
            entry != repository && self.entries.iter().any(|e| e.path == *entry)
        }) {
            return Ok(false);
        }
        self.merge_disk();
        let mut changed = false;
        for (entry, repository) in pairs {
            changed |= self.fold(entry, repository);
        }
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    /// Makes the entry for `folder` the repository's, in its place. The
    /// repository's own alias wins; the folder's carries over only when the
    /// repository has none.
    fn fold(&mut self, folder: &Path, repository: &Path) -> bool {
        if folder == repository {
            return false;
        }
        let Some(index) = self.entries.iter().position(|e| e.path == folder) else {
            return false;
        };
        let old = self.entries.remove(index);
        match self.entries.iter_mut().find(|e| e.path == repository) {
            Some(entry) => {
                if entry.alias.is_none() {
                    entry.alias = old.alias;
                }
            }
            None => self.entries.insert(
                index,
                Entry {
                    path: repository.to_path_buf(),
                    alias: old.alias,
                },
            ),
        }
        true
    }

    /// Removes several entries at once (a repository and the worktrees that
    /// were saved as if they were repositories); no folder is touched.
    pub fn remove_all(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        self.merge_disk();
        self.entries.retain(|e| !paths.contains(&e.path));
        self.save()
    }

    pub fn alias(&self, path: &Path) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .and_then(|e| e.alias.as_deref())
    }

    /// The rows for a search: case-insensitive on the name and the folder,
    /// recent ones first in their recent order, then the rest by name.
    /// `exists` says whether a folder is there (a test passes its own).
    pub fn rows(
        &self,
        query: &str,
        current: Option<&Path>,
        recent: &[PathBuf],
        exists: impl Fn(&Path) -> bool,
    ) -> Vec<Row> {
        let query = query.trim().to_lowercase();
        let mut rows: Vec<Row> = self
            .entries
            .iter()
            .filter(|e| {
                query.is_empty()
                    || e.name().to_lowercase().contains(&query)
                    || e.path.to_string_lossy().to_lowercase().contains(&query)
            })
            .map(|e| Row {
                path: e.path.clone(),
                name: e.name(),
                location: e
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                missing: !exists(&e.path),
                current: current == Some(e.path.as_path()),
                recent: recent.contains(&e.path),
            })
            .collect();
        rows.sort_by(|a, b| {
            let rank = |r: &Row| {
                recent
                    .iter()
                    .position(|p| *p == r.path)
                    .unwrap_or(usize::MAX)
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        rows
    }
}

impl RepositoryList {
    /// The list as the dropdown draws it: each repository once, keyed by its
    /// common directory, with its worktrees under it. `known` is what Git
    /// last said about an entry, `exists` whether a folder is there (a test
    /// passes its own).
    ///
    /// The search matches a repository's name and folder (all its worktrees
    /// stay), or a worktree's folder name or branch (only the matching
    /// worktrees stay). Recent repositories come first, ranked by their
    /// most recently opened worktree, then the rest by name.
    pub fn groups(
        &self,
        query: &str,
        current: Option<&Path>,
        recent: &[PathBuf],
        known: impl Fn(&Path) -> Option<Known>,
        exists: impl Fn(&Path) -> bool,
    ) -> Vec<Group> {
        let query = query.trim().to_lowercase();
        let matches = |text: &str| query.is_empty() || text.to_lowercase().contains(&query);
        let is_current = |path: &Path| current.is_some_and(|c| same_folder(c, path));
        // (key, the repository's folder, the group, every folder in it)
        let mut keyed: Vec<(PathBuf, PathBuf, Group, Vec<PathBuf>)> = Vec::new();
        for entry in &self.entries {
            let info = known(&entry.path);
            let key = info
                .as_ref()
                .and_then(Known::key)
                .unwrap_or_else(|| entry.path.clone());
            if let Some((_, repository, group, _)) = keyed.iter_mut().find(|(k, _, _, _)| *k == key)
            {
                group.entries.push(entry.path.clone());
                // The repository's own entry names it, as `fold` keeps it.
                if entry.path == *repository && entry.alias.is_some() {
                    group.name = entry.name();
                }
                continue;
            }
            let repository = info
                .as_ref()
                .and_then(Known::repository)
                .unwrap_or_else(|| entry.path.clone());
            let name = entry
                .alias
                .clone()
                .unwrap_or_else(|| folder_name(&repository));
            let worktrees: Vec<WorktreeRow> = info
                .as_ref()
                .map(|info| worktree_rows(info, &is_current))
                .unwrap_or_default();
            let mut folders: Vec<PathBuf> = worktrees.iter().map(|w| w.path.clone()).collect();
            folders.push(entry.path.clone());
            folders.push(repository.clone());
            let opens = match &info {
                Some(info) => info.main_worktree().map(|w| w.path.clone()).or_else(|| {
                    worktrees
                        .iter()
                        .find(|w| !w.missing && exists(&w.path))
                        .map(|w| w.path.clone())
                }),
                None => exists(&entry.path).then(|| entry.path.clone()),
            };
            let group = Group {
                path: entry.path.clone(),
                entries: vec![entry.path.clone()],
                name,
                location: repository
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                opens,
                missing: info.is_none() && !exists(&entry.path),
                current: folders.iter().any(|f| is_current(f)),
                recent: false,
                // Only the main worktree: the repository row is enough.
                worktrees: if worktrees.iter().any(|w| !w.main) {
                    worktrees
                } else {
                    Vec::new()
                },
            };
            keyed.push((key, repository, group, folders));
        }
        let mut groups = Vec::new();
        for (_, repository, mut group, folders) in keyed {
            let repository_matches = matches(&group.name) || matches(&repository.to_string_lossy());
            if !repository_matches {
                group
                    .worktrees
                    .retain(|w| matches(&w.name) || w.branch.as_deref().is_some_and(matches));
                if group.worktrees.is_empty() {
                    continue;
                }
            }
            let rank = folders
                .iter()
                .chain(group.entries.iter())
                .filter_map(|f| recent.iter().position(|r| r == f))
                .min()
                .unwrap_or(usize::MAX);
            group.recent = rank != usize::MAX;
            groups.push((rank, group));
        }
        groups.sort_by(|(a_rank, a), (b_rank, b)| {
            a_rank
                .cmp(b_rank)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        groups.into_iter().map(|(_, group)| group).collect()
    }
}

/// A repository's worktrees for the list: the main one first and marked,
/// bare entries and the common directory itself left out.
fn worktree_rows(info: &Known, is_current: &impl Fn(&Path) -> bool) -> Vec<WorktreeRow> {
    let main = info.main_worktree().map(|w| w.path.clone());
    info.worktrees
        .iter()
        .filter(|w| !w.bare && (!w.main || main.as_ref() == Some(&w.path)))
        .map(|w| WorktreeRow {
            path: w.path.clone(),
            name: folder_name(&w.path),
            branch: w.branch.clone(),
            short_sha: w
                .branch
                .is_none()
                .then(|| w.head.as_deref().map(|h| h.chars().take(7).collect()))
                .flatten(),
            main: w.main,
            current: is_current(&w.path),
            missing: w.prunable,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Fixture repositories made by the real `git` in a temporary directory,
    //! with its own `HOME` and no global or system configuration passed to
    //! each command, so nothing reads the person's own settings.

    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use super::{Known, RepositoryList};

    fn git(home: &Path, dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Better Git Test")
            .env("GIT_COMMITTER_NAME", "Better Git Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// What the session's background read learns about a folder.
    fn known(home: &Path, folder: &Path) -> Known {
        let list = Command::new("git")
            .arg("-C")
            .arg(folder)
            .args(["worktree", "list", "--porcelain", "-z"])
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git");
        let common = git(
            home,
            folder,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        Known {
            common_dir: Some(PathBuf::from(common.trim())),
            worktrees: git_core::parse_worktrees(&list.stdout).expect("worktree list"),
        }
    }

    struct Fixture {
        tmp: TempDir,
        home: PathBuf,
        main: PathBuf,
        /// Three linked worktrees: two on branches, the last detached.
        linked: [PathBuf; 3],
    }

    impl Fixture {
        fn new() -> Fixture {
            let tmp = TempDir::new().unwrap();
            let home = tmp.path().join("home");
            std::fs::create_dir_all(&home).unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let main = root.join("better-os");
            std::fs::create_dir_all(&main).unwrap();
            git(&home, &main, &["init", "-q", "-b", "main"]);
            git(
                &home,
                &main,
                &["commit", "-q", "--allow-empty", "-m", "first"],
            );
            let linked = [
                root.join("wt/ticket-101"),
                root.join("wt/ticket-102"),
                root.join("wt/release-0.2.13"),
            ];
            for (folder, branch) in linked[..2].iter().zip(["ticket-101", "ticket-102"]) {
                git(
                    &home,
                    &main,
                    &[
                        "worktree",
                        "add",
                        "-q",
                        "-b",
                        branch,
                        folder.to_str().unwrap(),
                    ],
                );
            }
            git(
                &home,
                &main,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    linked[2].to_str().unwrap(),
                ],
            );
            Fixture {
                tmp,
                home,
                main,
                linked,
            }
        }

        fn folders(&self) -> Vec<PathBuf> {
            let mut all = vec![self.main.clone()];
            all.extend(self.linked.iter().cloned());
            all
        }

        fn known(&self, folder: &Path) -> Option<Known> {
            folder.exists().then(|| known(&self.home, folder))
        }

        fn list(&self) -> RepositoryList {
            RepositoryList::load(Some(self.tmp.path().join("state/git-repositories.json")))
        }
    }

    fn permutations(items: &[PathBuf]) -> Vec<Vec<PathBuf>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(i);
            for mut tail in permutations(&rest) {
                tail.insert(0, first.clone());
                out.push(tail);
            }
        }
        out
    }

    #[test]
    fn a_repository_and_its_worktrees_opened_in_any_order_are_one_group() {
        let f = Fixture::new();
        let orders = permutations(&f.folders());
        assert_eq!(orders.len(), 24);
        for order in orders {
            let file = f.tmp.path().join("state/git-repositories.json");
            let _ = std::fs::remove_file(&file);
            let mut list = f.list();
            for folder in &order {
                let repository = known(&f.home, folder).repository().unwrap();
                list.add_opened(folder, &repository).unwrap();
            }
            let current = order.last().unwrap();
            let groups = list.groups(
                "",
                Some(current),
                &order.iter().rev().cloned().collect::<Vec<_>>(),
                |p| f.known(p),
                Path::exists,
            );
            assert_eq!(groups.len(), 1, "{order:?}");
            let group = &groups[0];
            assert_eq!(group.name, "better-os");
            assert_eq!(group.path, f.main);
            assert_eq!(group.opens.as_deref(), Some(f.main.as_path()));
            assert!(group.current && group.recent);
            // The main one first, then the linked ones in Git's order.
            let mut names: Vec<&str> = group.worktrees.iter().map(|w| w.name.as_str()).collect();
            assert_eq!(names.remove(0), "better-os");
            names.sort_unstable();
            assert_eq!(names, ["release-0.2.13", "ticket-101", "ticket-102"]);
            assert!(group.worktrees[0].main);
            assert!(group.worktrees[1..].iter().all(|w| !w.main));
            let marked: Vec<&Path> = group
                .worktrees
                .iter()
                .filter(|w| w.current)
                .map(|w| w.path.as_path())
                .collect();
            assert_eq!(marked, [current.as_path()], "{order:?}");
            // Written once: the file holds the repository and nothing else.
            assert_eq!(
                f.list()
                    .entries
                    .iter()
                    .map(|e| e.path.clone())
                    .collect::<Vec<_>>(),
                std::slice::from_ref(&f.main)
            );
        }
    }

    #[test]
    fn a_saved_list_with_linked_worktrees_as_repositories_is_merged() {
        let f = Fixture::new();
        // The list an older Better Git wrote: a linked worktree first, named,
        // then the main worktree, then another linked one.
        let mut list = f.list();
        for folder in [&f.linked[0], &f.main, &f.linked[1]] {
            list.add(folder).unwrap();
        }
        list.set_alias(&f.linked[0], "Ticket").unwrap();
        let mut list = f.list();
        let selected = f.linked[0].clone();

        // Drawn as one repository before anything is written.
        let groups = list.groups("", Some(&selected), &[], |p| f.known(p), Path::exists);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entries.len(), 3);

        let pairs: Vec<(PathBuf, PathBuf)> = list
            .entries
            .iter()
            .map(|e| {
                (
                    e.path.clone(),
                    f.known(&e.path).unwrap().repository().unwrap(),
                )
            })
            .collect();
        assert!(list.merge_worktrees(&pairs).unwrap());
        assert!(
            !list.merge_worktrees(&pairs).unwrap(),
            "nothing left to merge"
        );
        let reread = f.list();
        assert_eq!(reread.entries.len(), 1);
        assert_eq!(reread.entries[0].path, f.main);
        assert_eq!(
            reread.alias(&f.main),
            Some("Ticket"),
            "the repository had no alias of its own"
        );
        // The worktree that was open stays the one marked.
        let groups = reread.groups(
            "",
            Some(&selected),
            std::slice::from_ref(&selected),
            |p| f.known(p),
            Path::exists,
        );
        assert_eq!(groups.len(), 1);
        let marked: Vec<&Path> = groups[0]
            .worktrees
            .iter()
            .filter(|w| w.current)
            .map(|w| w.path.as_path())
            .collect();
        assert_eq!(marked, [selected.as_path()]);
    }

    #[test]
    fn a_detached_worktree_is_named_by_its_folder() {
        let f = Fixture::new();
        let mut list = f.list();
        list.add_opened(&f.linked[2], &f.main).unwrap();
        let groups = list.groups("", None, &[], |p| f.known(p), Path::exists);
        let detached = groups[0]
            .worktrees
            .iter()
            .find(|w| w.path == f.linked[2])
            .unwrap();
        assert_eq!(detached.name, "release-0.2.13");
        assert_eq!(detached.branch, None);
        let head = git(&f.home, &f.main, &["rev-parse", "HEAD"]);
        assert_eq!(detached.short_sha.as_deref(), Some(&head.trim()[..7]));
        let on_branch = groups[0]
            .worktrees
            .iter()
            .find(|w| w.path == f.linked[0])
            .unwrap();
        assert_eq!(on_branch.branch.as_deref(), Some("ticket-101"));
        assert_eq!(on_branch.short_sha, None);
    }

    #[test]
    fn removing_a_repository_removes_every_entry_of_it_and_no_folder() {
        let f = Fixture::new();
        let mut list = f.list();
        for folder in f.folders() {
            list.add(&folder).unwrap();
        }
        let other = f.tmp.path().canonicalize().unwrap().join("other");
        std::fs::create_dir_all(&other).unwrap();
        git(&f.home, &other, &["init", "-q", "-b", "main"]);
        list.add(&other).unwrap();
        let groups = list.groups("", None, &[], |p| f.known(p), Path::exists);
        assert_eq!(groups.len(), 2);
        let repository = groups.iter().find(|g| g.name == "better-os").unwrap();
        assert_eq!(repository.entries.len(), 4);
        list.remove_all(&repository.entries).unwrap();
        let reread = f.list();
        assert_eq!(
            reread
                .entries
                .iter()
                .map(|e| e.path.clone())
                .collect::<Vec<_>>(),
            [other]
        );
        assert!(f.folders().iter().all(|folder| folder.exists()));
    }

    #[test]
    fn the_search_matches_repositories_worktree_folders_and_branches() {
        let f = Fixture::new();
        let mut list = f.list();
        list.add_opened(&f.main, &f.main).unwrap();
        let search = |query: &str| {
            list.groups(query, None, &[], |p| f.known(p), Path::exists)
                .into_iter()
                .map(|g| {
                    (
                        g.name,
                        g.worktrees.into_iter().map(|w| w.name).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(search("BETTER")[0].1.len(), 4, "the repository keeps all");
        assert_eq!(
            search("release"),
            [("better-os".to_string(), vec!["release-0.2.13".to_string()])]
        );
        assert_eq!(
            search("ticket-102"),
            [("better-os".to_string(), vec!["ticket-102".to_string()])]
        );
        // A branch name that is not a folder name.
        git(
            &f.home,
            &f.linked[0],
            &["switch", "-q", "-c", "feature/menu"],
        );
        assert_eq!(
            search("feature/"),
            [("better-os".to_string(), vec!["ticket-101".to_string()])]
        );
        assert!(search("nothing-like-this").is_empty());
    }

    #[test]
    fn a_bare_repository_and_one_whose_main_worktree_is_gone_still_show() {
        let f = Fixture::new();
        let root = f.tmp.path().canonicalize().unwrap();
        // A bare clone with one worktree.
        let bare = root.join("bare.git");
        git(
            &f.home,
            &root,
            &[
                "clone",
                "-q",
                "--bare",
                f.main.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let bare_wt = root.join("bare-work");
        git(
            &f.home,
            &bare,
            &["worktree", "add", "-q", bare_wt.to_str().unwrap(), "main"],
        );
        // A repository whose git directory is kept apart, its main worktree
        // deleted, one linked worktree left.
        let store = root.join("store.git");
        let gone = root.join("gone");
        git(
            &f.home,
            &root,
            &[
                "init",
                "-q",
                "-b",
                "main",
                &format!("--separate-git-dir={}", store.display()),
                gone.to_str().unwrap(),
            ],
        );
        git(
            &f.home,
            &gone,
            &["commit", "-q", "--allow-empty", "-m", "first"],
        );
        let left = root.join("left");
        git(
            &f.home,
            &gone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "left",
                left.to_str().unwrap(),
            ],
        );
        std::fs::remove_dir_all(&gone).unwrap();

        let mut list = f.list();
        for folder in [&bare_wt, &left] {
            let repository = known(&f.home, folder).repository().unwrap();
            list.add_opened(folder, &repository).unwrap();
        }
        let groups = list.groups("", None, &[], |p| f.known(p), Path::exists);
        assert_eq!(groups.len(), 2, "{groups:#?}");
        let by_name = |name: &str| groups.iter().find(|g| g.name == name).unwrap();
        let bare_group = by_name("bare.git");
        assert_eq!(bare_group.path, bare);
        assert!(!bare_group.missing);
        assert_eq!(bare_group.opens.as_deref(), Some(bare_wt.as_path()));
        assert_eq!(
            bare_group
                .worktrees
                .iter()
                .map(|w| &w.path)
                .collect::<Vec<_>>(),
            [&bare_wt]
        );
        let store_group = by_name("store.git");
        assert_eq!(store_group.path, store);
        assert!(!store_group.missing);
        assert_eq!(store_group.opens.as_deref(), Some(left.as_path()));
        assert_eq!(
            store_group
                .worktrees
                .iter()
                .map(|w| &w.path)
                .collect::<Vec<_>>(),
            [&left]
        );
    }
}

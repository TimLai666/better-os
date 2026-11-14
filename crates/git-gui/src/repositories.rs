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
    /// Folders taken out of the list lately (removed, or folded into their
    /// repository), so a second window that still holds them does not write
    /// them back when it merges this file into its own list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dropped: Vec<PathBuf>,
}

/// How many dropped folders the file remembers.
const DROPPED_LIMIT: usize = 256;

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

/// What Git said about one folder, gathered off the window's thread
/// ([`Known::assemble`]).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    /// `git rev-parse --show-toplevel`; `None` in a bare repository.
    pub toplevel: Option<PathBuf>,
    /// This folder's own git directory and the one its worktrees share.
    pub git_dir: Option<PathBuf>,
    pub common_dir: Option<PathBuf>,
    /// `git worktree list`, the main entry first.
    pub worktrees: Vec<Worktree>,
    /// `core.worktree` in the common directory, joined to it.
    pub core_worktree: Option<PathBuf>,
}

/// What Git last said about a folder in the list.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Known {
    /// The git directory every worktree of the repository shares
    /// (`git rev-parse --git-common-dir`), when it could be read.
    pub common_dir: Option<PathBuf>,
    /// The repository's worktrees, the main one first.
    pub worktrees: Vec<Worktree>,
    /// The main worktree's folder, when Git says where it is and it is
    /// there.
    pub main: Option<PathBuf>,
    /// The repository has no main worktree to show: it is bare, or Git
    /// records where the main worktree was and that folder is gone.
    pub no_main: bool,
}

impl Known {
    /// What a read of `folder` means, or `None` when `folder` is not the
    /// top of a repository: Git walks up to an enclosing repository from a
    /// folder that no longer is one (a submodule taken out, a `.git` deleted
    /// under a dotfiles repository), and that answer is about another
    /// repository. A bare repository is accepted at its own folder.
    ///
    /// Git lists the common directory itself as the main entry for a
    /// submodule and for a repository made with `--separate-git-dir`, so the
    /// main worktree is found from what Git knows instead: the folder itself
    /// when its git directory is the common one, `core.worktree` when Git
    /// records it, and the main entry when it is a working tree. This reads
    /// the file system and belongs off the window's thread.
    pub fn assemble(folder: &Path, facts: Facts) -> Option<Known> {
        let here = folder.canonicalize().ok()?;
        let canonical = |p: &Path| p.canonicalize().ok();
        let first = facts.worktrees.first().filter(|w| w.main);
        let bare = first.is_some_and(|w| w.bare);
        let accepted = match &facts.toplevel {
            Some(top) => canonical(top).as_ref() == Some(&here),
            None => bare && facts.common_dir.as_deref().and_then(canonical).as_ref() == Some(&here),
        };
        if !accepted {
            return None;
        }
        let candidate = if bare {
            None
        } else if facts.git_dir.is_some() && facts.git_dir == facts.common_dir {
            facts.toplevel.clone()
        } else if let Some(core) = facts.core_worktree.clone() {
            Some(core)
        } else {
            first
                .map(|w| w.path.clone())
                .filter(|p| facts.common_dir.as_ref() != Some(p))
        };
        let main = candidate.as_deref().and_then(canonical);
        Some(Known {
            no_main: bare || (candidate.is_some() && main.is_none()),
            common_dir: facts.common_dir,
            worktrees: facts.worktrees,
            main,
        })
    }

    /// The folder that stands for the repository in the list: its main
    /// worktree, or its common directory when it has none to show (bare, or
    /// gone). `None` when Git cannot say where the main worktree is, as
    /// from a linked worktree of a `--separate-git-dir` repository: such an
    /// entry is kept as it was rather than guessed at.
    pub fn repository(&self) -> Option<PathBuf> {
        self.main
            .clone()
            .or_else(|| self.no_main.then(|| self.common_dir.clone()).flatten())
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
    dropped: Vec<PathBuf>,
    /// How many times the file was written, for tests.
    pub writes: usize,
}

impl RepositoryList {
    pub fn default_file() -> Option<PathBuf> {
        crate::recent::default_path().map(|p| p.with_file_name("git-repositories.json"))
    }

    /// Reads the list; a missing or unreadable file is empty.
    pub fn load(file: Option<PathBuf>) -> RepositoryList {
        let document = file
            .as_deref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Document>(&b).ok())
            .unwrap_or_default();
        let entries = document
            .repositories
            .into_iter()
            .filter(|e| e.path.is_absolute() && !document.dropped.contains(&e.path))
            .collect();
        RepositoryList {
            file,
            entries,
            dropped: document.dropped,
            writes: 0,
        }
    }

    /// Takes in what another window wrote: its additions, and what it
    /// dropped (which stays dropped here too).
    fn merge_disk(&mut self) {
        let disk = RepositoryList::load(self.file.clone());
        for path in disk.dropped {
            if !self.dropped.contains(&path) {
                self.dropped.push(path);
            }
        }
        for entry in disk.entries {
            if !self.entries.iter().any(|e| e.path == entry.path) {
                self.entries.push(entry);
            }
        }
        let dropped = &self.dropped;
        self.entries.retain(|e| !dropped.contains(&e.path));
    }

    fn drop_path(&mut self, path: &Path) {
        if !self.dropped.iter().any(|p| p == path) {
            self.dropped.push(path.to_path_buf());
        }
        let excess = self.dropped.len().saturating_sub(DROPPED_LIMIT);
        self.dropped.drain(..excess);
    }

    fn undrop(&mut self, path: &Path) {
        self.dropped.retain(|p| p != path);
    }

    fn save(&mut self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let document = Document {
            repositories: self.entries.clone(),
            dropped: self.dropped.clone(),
        };
        self.writes += 1;
        let bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
        let partial = file.with_extension("json.partial");
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, file)
    }

    /// Adds a repository (no-op when it is there).
    pub fn add(&mut self, path: &Path) -> io::Result<()> {
        self.merge_disk();
        self.undrop(path);
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
        self.drop_path(path);
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
        self.drop_path(from);
        self.undrop(to);
        self.entries.push(Entry {
            path: to.to_path_buf(),
            alias,
        });
        self.save()
    }

    /// Saves the repository of a folder that was opened: `folder` is the
    /// worktree, `repository` what [`Known::repository`] said. A saved entry
    /// for the worktree itself becomes the repository's. Writes only when
    /// something changed.
    pub fn add_opened(&mut self, folder: &Path, repository: &Path) -> io::Result<()> {
        self.merge_disk();
        let mut changed = self.fold(folder, repository);
        if !self.entries.iter().any(|e| e.path == repository) {
            self.undrop(repository);
            self.entries.push(Entry {
                path: repository.to_path_buf(),
                alias: None,
            });
            changed = true;
        }
        if changed {
            self.save()?;
        }
        Ok(())
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
        self.drop_path(folder);
        self.undrop(repository);
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
        for path in paths {
            self.drop_path(path);
        }
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
    /// last said about an entry, `exists` whether a folder was there at the
    /// last read (the window passes what its background read recorded, so
    /// drawing never touches the file system). `current` is compared as it
    /// is: Git's paths are already canonical.
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
        let is_current = |path: &Path| current == Some(path);
        // Entries by repository, in the list's order.
        let mut keyed: Vec<(PathBuf, Members)> = Vec::new();
        for entry in &self.entries {
            let info = known(&entry.path);
            let key = info
                .as_ref()
                .and_then(Known::key)
                .unwrap_or_else(|| entry.path.clone());
            match keyed.iter_mut().find(|(k, _)| *k == key) {
                Some((_, members)) => members.push((entry, info)),
                None => keyed.push((key, vec![(entry, info)])),
            }
        }
        let mut groups = Vec::new();
        for (_, members) in keyed {
            // What knows the main worktree best speaks for the repository.
            let info = members
                .iter()
                .filter_map(|(_, info)| info.as_ref())
                .max_by_key(|info| (info.main.is_some(), info.no_main))
                .cloned();
            let first = members[0].0;
            let repository = info
                .as_ref()
                .and_then(|i| i.repository().or_else(|| i.common_dir.clone()))
                .unwrap_or_else(|| first.path.clone());
            // The repository's own entry is the one acted on, and names it.
            let own = members
                .iter()
                .map(|(e, _)| *e)
                .find(|e| e.path == repository)
                .unwrap_or(first);
            let name = own
                .alias
                .clone()
                .or_else(|| members.iter().find_map(|(e, _)| e.alias.clone()))
                .unwrap_or_else(|| folder_name(&repository));
            let worktrees: Vec<WorktreeRow> = info
                .as_ref()
                .map(|info| worktree_rows(info, &is_current))
                .unwrap_or_default();
            let entries: Vec<PathBuf> = members.iter().map(|(e, _)| e.path.clone()).collect();
            let mut folders: Vec<PathBuf> = worktrees.iter().map(|w| w.path.clone()).collect();
            folders.extend(entries.iter().cloned());
            folders.push(repository.clone());
            let opens = match &info {
                Some(info) => info.main.clone().or_else(|| {
                    worktrees
                        .iter()
                        .find(|w| !w.missing)
                        .map(|w| w.path.clone())
                }),
                None => exists(&own.path).then(|| own.path.clone()),
            };
            let mut group = Group {
                path: own.path.clone(),
                name,
                location: repository
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                opens,
                missing: info.is_none() && !exists(&own.path),
                current: folders.iter().any(|f| is_current(f)),
                recent: false,
                // Only the main worktree: the repository row is enough.
                worktrees: if worktrees.iter().any(|w| !w.main) {
                    worktrees
                } else {
                    Vec::new()
                },
                entries,
            };
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

/// The saved entries of one repository, with what Git said about each.
type Members<'a> = Vec<(&'a Entry, Option<Known>)>;

/// A repository's worktrees for the list: the main one first and marked,
/// under the folder it really is (Git lists a submodule's common directory
/// in its place); bare entries and an unknown main worktree left out.
fn worktree_rows(info: &Known, is_current: &impl Fn(&Path) -> bool) -> Vec<WorktreeRow> {
    info.worktrees
        .iter()
        .filter(|w| !w.bare)
        .filter_map(|w| {
            let path = if w.main {
                info.main.clone()?
            } else {
                w.path.clone()
            };
            Some(WorktreeRow {
                name: folder_name(&path),
                branch: w.branch.clone(),
                short_sha: w
                    .branch
                    .is_none()
                    .then(|| w.head.as_deref().map(|h| h.chars().take(7).collect()))
                    .flatten(),
                main: w.main,
                current: is_current(&path),
                missing: w.prunable,
                path,
            })
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

    use super::{Facts, Known, RepositoryList};

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

    /// A git command's output, or `None` when it fails.
    fn try_git(home: &Path, dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    }

    /// What the session's background read learns about a folder, gathered
    /// the way `session::read_known` gathers it.
    fn known(home: &Path, folder: &Path) -> Option<Known> {
        let text = |out: Vec<u8>| String::from_utf8(out).unwrap().trim().to_string();
        let worktrees = git_core::parse_worktrees(&try_git(
            home,
            folder,
            &["worktree", "list", "--porcelain", "-z"],
        )?)
        .unwrap();
        let dirs = try_git(
            home,
            folder,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
        )
        .map(text);
        let (git_dir, common_dir) = dirs
            .as_deref()
            .and_then(|d| d.split_once('\n'))
            .map(|(a, b)| (PathBuf::from(a), PathBuf::from(b)))
            .unzip();
        let core_worktree = common_dir.as_ref().and_then(|c| {
            try_git(home, c, &["config", "--get", "core.worktree"])
                .map(text)
                .map(|v| c.join(v))
        });
        Known::assemble(
            folder,
            Facts {
                toplevel: try_git(home, folder, &["rev-parse", "--show-toplevel"])
                    .map(text)
                    .map(PathBuf::from),
                git_dir,
                common_dir,
                worktrees,
                core_worktree,
            },
        )
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
            known(&self.home, folder)
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
                let repository = known(&f.home, folder).unwrap().repository().unwrap();
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
            // Written once, whatever the order: the first open saves the
            // repository and every later one finds it there.
            assert_eq!(list.writes, 1, "{order:?}");
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
            // Git cannot say where `left`'s main worktree was (no
            // `core.worktree`), so `left` is kept as it was opened.
            let repository = known(&f.home, folder)
                .unwrap()
                .repository()
                .unwrap_or_else(|| folder.clone());
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
        assert_eq!(store_group.path, left);
        assert_eq!(store_group.entries, std::slice::from_ref(&left));
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

    #[test]
    fn a_submodule_and_a_separate_git_dir_repository_keep_their_main_worktree() {
        let f = Fixture::new();
        let root = f.tmp.path().canonicalize().unwrap();
        // `--separate-git-dir`, main worktree present, one linked worktree.
        let store = root.join("store.git");
        let main = root.join("apart");
        git(
            &f.home,
            &root,
            &[
                "init",
                "-q",
                "-b",
                "main",
                &format!("--separate-git-dir={}", store.display()),
                main.to_str().unwrap(),
            ],
        );
        git(
            &f.home,
            &main,
            &["commit", "-q", "--allow-empty", "-m", "a"],
        );
        let apart_wt = root.join("apart-wt");
        git(
            &f.home,
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "w",
                apart_wt.to_str().unwrap(),
            ],
        );
        // A submodule with a linked worktree of its own.
        let sup = root.join("sup");
        std::fs::create_dir_all(&sup).unwrap();
        git(&f.home, &sup, &["init", "-q", "-b", "main"]);
        git(
            &f.home,
            &sup,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                f.main.to_str().unwrap(),
                "sub",
            ],
        );
        let sub = sup.join("sub");
        let sub_wt = root.join("sub-wt");
        git(
            &f.home,
            &sub,
            &["worktree", "add", "-q", "-b", "s", sub_wt.to_str().unwrap()],
        );

        // Git lists the common directory as the main entry for both.
        let from_sub = known(&f.home, &sub).unwrap();
        assert_eq!(
            from_sub.worktrees[0].path,
            sup.join(".git/modules/sub"),
            "the case this test is about"
        );
        assert_eq!(from_sub.repository(), Some(sub.clone()));
        assert_eq!(
            known(&f.home, &sub_wt).unwrap().repository(),
            Some(sub.clone()),
            "found through core.worktree"
        );
        assert_eq!(
            known(&f.home, &main).unwrap().repository(),
            Some(main.clone())
        );

        let mut list = f.list();
        list.add(&sub).unwrap();
        list.add(&main).unwrap();
        list.add(&apart_wt).unwrap();
        list.add(&sub_wt).unwrap();
        let before = std::fs::read(f.tmp.path().join("state/git-repositories.json")).unwrap();
        let writes = list.writes;
        let pairs: Vec<(PathBuf, PathBuf)> = list
            .entries
            .iter()
            .filter_map(|e| Some((e.path.clone(), known(&f.home, &e.path)?.repository()?)))
            .collect();
        list.merge_worktrees(&pairs).unwrap();
        // Only the submodule's linked worktree folds; `sub` and `main` stay.
        assert_eq!(list.writes, writes + 1);
        assert_eq!(
            list.entries
                .iter()
                .map(|e| e.path.clone())
                .collect::<Vec<_>>(),
            [sub.clone(), main.clone(), apart_wt.clone()]
        );
        assert_ne!(
            before,
            std::fs::read(f.tmp.path().join("state/git-repositories.json")).unwrap()
        );
        let pairs_again: Vec<(PathBuf, PathBuf)> = [&sub, &main]
            .iter()
            .map(|p| {
                (
                    (*p).clone(),
                    known(&f.home, p).unwrap().repository().unwrap(),
                )
            })
            .collect();
        assert!(!list.merge_worktrees(&pairs_again).unwrap(), "untouched");

        let groups = list.groups("", Some(&sub_wt), &[], |p| f.known(p), Path::exists);
        assert_eq!(groups.len(), 2, "{groups:#?}");
        let sub_group = groups.iter().find(|g| g.name == "sub").unwrap();
        assert_eq!(sub_group.path, sub);
        assert_eq!(sub_group.opens.as_deref(), Some(sub.as_path()));
        assert!(sub_group.current);
        let paths: Vec<&PathBuf> = sub_group.worktrees.iter().map(|w| &w.path).collect();
        assert_eq!(paths, [&sub, &sub_wt]);
        assert!(sub_group.worktrees[0].main && sub_group.worktrees[1].current);
        let apart = groups.iter().find(|g| g.name == "apart").unwrap();
        assert_eq!(apart.path, main);
        assert_eq!(apart.opens.as_deref(), Some(main.as_path()));
        assert_eq!(apart.entries, [main.clone(), apart_wt.clone()]);
        let paths: Vec<&PathBuf> = apart.worktrees.iter().map(|w| &w.path).collect();
        assert_eq!(paths, [&main, &apart_wt]);
    }

    #[test]
    fn a_folder_that_is_no_longer_a_repository_is_not_read_as_the_one_around_it() {
        let f = Fixture::new();
        // A plain folder inside a repository: Git walks up to the outer one.
        let inner = f.main.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        assert!(try_git(&f.home, &inner, &["worktree", "list"]).is_some());
        assert_eq!(known(&f.home, &inner), None);
        assert_eq!(known(&f.home, &f.tmp.path().join("gone")), None);
        let mut list = f.list();
        list.add(&inner).unwrap();
        let groups = list.groups("", None, &[], |p| f.known(p), |_| false);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].missing && groups[0].opens.is_none());
        assert_eq!(groups[0].path, inner);
    }

    #[test]
    fn a_symlinked_entry_folds_into_its_repository_with_its_alias() {
        let f = Fixture::new();
        let link = f.tmp.path().canonicalize().unwrap().join("link");
        std::os::unix::fs::symlink(&f.main, &link).unwrap();
        let mut list = f.list();
        list.add(&link).unwrap();
        list.set_alias(&link, "Mine").unwrap();
        let repository = known(&f.home, &link).unwrap().repository().unwrap();
        assert_eq!(repository, f.main);
        assert!(list.merge_worktrees(&[(link.clone(), repository)]).unwrap());
        let reread = f.list();
        assert_eq!(reread.entries.len(), 1);
        assert_eq!(reread.entries[0].path, f.main);
        assert_eq!(reread.alias(&f.main), Some("Mine"));
    }

    #[test]
    fn another_window_holding_an_old_list_does_not_bring_dropped_entries_back() {
        let f = Fixture::new();
        let mut ours = f.list();
        for folder in f.folders() {
            ours.add(&folder).unwrap();
        }
        // A second window read the list before anything was folded.
        let mut theirs = f.list();
        let pairs: Vec<(PathBuf, PathBuf)> = f
            .linked
            .iter()
            .map(|l| (l.clone(), f.main.clone()))
            .collect();
        ours.merge_worktrees(&pairs).unwrap();
        // It writes its stale list (an alias, say) after re-reading.
        theirs.set_alias(&f.main, "Main").unwrap();
        assert_eq!(
            f.list()
                .entries
                .iter()
                .map(|e| e.path.clone())
                .collect::<Vec<_>>(),
            std::slice::from_ref(&f.main)
        );
        // Removal survives the same way, and adding again brings it back.
        let mut theirs = f.list();
        ours.remove_all(std::slice::from_ref(&f.main)).unwrap();
        theirs.set_alias(&f.main, "Again").unwrap();
        assert!(f.list().entries.is_empty());
        theirs.add(&f.main).unwrap();
        assert_eq!(f.list().entries.len(), 1);
    }
}

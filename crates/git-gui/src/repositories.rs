//! The repository list, as GitHub Desktop keeps it: every repository the
//! person added, each with an optional alias, searchable, and marked missing
//! when its folder is gone. It lives in
//! `$XDG_STATE_HOME/better-os/git-repositories.json`, written beside its
//! name and renamed, and re-read before every write so two windows do not
//! lose each other's additions. The recently opened order is `recent`'s.

use std::io;
use std::path::{Path, PathBuf};

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

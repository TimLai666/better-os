//! The changes list as GitHub Desktop has it: every changed file against
//! HEAD, a check box per file that says whether it goes into the commit,
//! and, inside a file's diff, a tick per changed line. A file with some of
//! its lines ticked is indeterminate. Nothing touches git's index until the
//! commit, which builds it from this ([`Inclusion::selections`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use better_ui::fluent::toggle::CheckState;
use git_core::{
    ChangeKind, DiffLine, DiffLineKind, FileChange, FileDiff, LineKey, RepositoryStatus,
    change_fingerprint, line_key,
};
use git_platform::FileSelection;
use git_platform::patch::LineSelection;

/// What of one file goes into the commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Include {
    All,
    Nothing,
    /// Only these changed lines (by their numbers, `git_core::line_key`),
    /// ticked on a diff with this `git_core::change_fingerprint`. The
    /// numbers and the fingerprint do not depend on how much context the
    /// diff shows, so the choice survives expanding the whole file.
    Lines {
        fingerprint: u64,
        lines: BTreeSet<LineKey>,
    },
}

/// The letter in a row's state square.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Badge {
    Modified,
    Added,
    Deleted,
    Renamed,
    Conflicted,
}

impl Badge {
    pub fn letter(self) -> &'static str {
        match self {
            Badge::Modified => "M",
            Badge::Added => "A",
            Badge::Deleted => "D",
            Badge::Renamed => "R",
            Badge::Conflicted => "!",
        }
    }
}

/// The state shown for a change: what HEAD to working tree did to it.
pub fn badge_of(change: &FileChange) -> Badge {
    if change.conflicted {
        return Badge::Conflicted;
    }
    if change.untracked {
        return Badge::Added;
    }
    let kinds = [change.staged, change.unstaged];
    if kinds.contains(&Some(ChangeKind::Renamed)) || kinds.contains(&Some(ChangeKind::Copied)) {
        Badge::Renamed
    } else if change.staged == Some(ChangeKind::Added) {
        if change.unstaged == Some(ChangeKind::Deleted) {
            // Added to the index and then deleted: nothing against HEAD,
            // shown as the deletion it last was.
            Badge::Deleted
        } else {
            Badge::Added
        }
    } else if kinds.contains(&Some(ChangeKind::Deleted)) {
        Badge::Deleted
    } else {
        Badge::Modified
    }
}

/// One row of the changes list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub name: String,
    pub folder: String,
    pub badge: Badge,
    pub check: CheckState,
    pub selected: bool,
    pub conflicted: bool,
}

/// The per-file inclusion, kept across refreshes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inclusion {
    files: BTreeMap<PathBuf, Include>,
}

impl Inclusion {
    /// Keeps the choices for paths still changed, includes new ones whole,
    /// and forgets paths that are no longer changed. A conflicted path is
    /// never included.
    pub fn sync(&mut self, status: &RepositoryStatus) {
        let mut next = BTreeMap::new();
        for change in &status.changes {
            let include = if change.conflicted {
                Include::Nothing
            } else {
                self.files.remove(&change.path).unwrap_or(Include::All)
            };
            next.insert(change.path.clone(), include);
        }
        self.files = next;
    }

    pub fn get(&self, path: &Path) -> Include {
        self.files.get(path).cloned().unwrap_or(Include::Nothing)
    }

    pub fn check(&self, path: &Path) -> CheckState {
        match self.files.get(path) {
            Some(Include::All) => CheckState::Checked,
            Some(Include::Lines { lines, .. }) if !lines.is_empty() => CheckState::Indeterminate,
            _ => CheckState::Unchecked,
        }
    }

    /// The file's check box was clicked: indeterminate and unchecked become
    /// checked, checked becomes unchecked.
    pub fn toggle_file(&mut self, path: &Path) {
        let next = match self.check(path) {
            CheckState::Checked => Include::Nothing,
            _ => Include::All,
        };
        if let Some(include) = self.files.get_mut(path) {
            *include = next;
        }
    }

    /// The header check box: everything when not everything is in, else
    /// nothing. Conflicted files are left out.
    pub fn toggle_all(&mut self, rows: &[Row]) {
        let all = rows
            .iter()
            .filter(|r| !r.conflicted)
            .all(|r| r.check == CheckState::Checked);
        for row in rows.iter().filter(|r| !r.conflicted) {
            self.files.insert(
                row.path.clone(),
                if all { Include::Nothing } else { Include::All },
            );
        }
    }

    /// The header check box's state over the rows shown.
    pub fn summary(rows: &[Row]) -> CheckState {
        let mut seen_checked = false;
        let mut seen_other = false;
        for row in rows.iter().filter(|r| !r.conflicted) {
            match row.check {
                CheckState::Checked => seen_checked = true,
                CheckState::Indeterminate => return CheckState::Indeterminate,
                CheckState::Unchecked => seen_other = true,
            }
        }
        match (seen_checked, seen_other) {
            (true, true) => CheckState::Indeterminate,
            (true, false) => CheckState::Checked,
            _ => CheckState::Unchecked,
        }
    }

    /// Whether a changed line is ticked.
    pub fn line_included(&self, path: &Path, line: &DiffLine) -> bool {
        match self.files.get(path) {
            Some(Include::All) => true,
            Some(Include::Lines { lines, .. }) => lines.contains(&line_key(line)),
            _ => false,
        }
    }

    /// Every file whole: during a merge a partial commit would drop the
    /// other side's changes, so GitHub Desktop commits every file whole and
    /// locks the check boxes. Conflicted files stay out until resolved.
    pub fn include_everything(&mut self, status: &RepositoryStatus) {
        for change in &status.changes {
            let include = if change.conflicted {
                Include::Nothing
            } else {
                Include::All
            };
            self.files.insert(change.path.clone(), include);
        }
    }

    /// Ticks or unticks lines of the file whose diff is `diff`. Lines that
    /// are not changes are ignored. Every changed line ticked is `All`;
    /// none is `Nothing`.
    pub fn set_lines(
        &mut self,
        path: &Path,
        diff: &FileDiff,
        targets: &[(usize, usize)],
        on: bool,
    ) {
        let FileDiff::Text { hunks } = diff else {
            return;
        };
        let changed: BTreeSet<LineKey> = hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|l| l.kind != DiffLineKind::Context)
            .map(line_key)
            .collect();
        let fingerprint = change_fingerprint(diff);
        let mut lines = match self.files.get(path) {
            Some(Include::All) => changed.clone(),
            Some(Include::Lines {
                lines,
                fingerprint: f,
            }) if *f == fingerprint => lines.clone(),
            _ => BTreeSet::new(),
        };
        let targets = targets
            .iter()
            .filter_map(|&(h, l)| hunks.get(h)?.lines.get(l))
            .filter(|l| l.kind != DiffLineKind::Context)
            .map(line_key);
        for target in targets {
            if on {
                lines.insert(target);
            } else {
                lines.remove(&target);
            }
        }
        let include = if lines.is_empty() {
            Include::Nothing
        } else if lines == changed {
            Include::All
        } else {
            Include::Lines { fingerprint, lines }
        };
        self.files.insert(path.to_path_buf(), include);
    }

    /// A ticked-line choice made on a diff that has changed since (the
    /// file was edited) is meaningless; it becomes the whole file, as
    /// GitHub Desktop does. Showing more or less context is not a change.
    pub fn reconcile(&mut self, path: &Path, diff: &FileDiff) {
        if !matches!(diff, FileDiff::Text { .. }) {
            return;
        }
        let fingerprint = change_fingerprint(diff);
        if let Some(include) = self.files.get_mut(path)
            && let Include::Lines { fingerprint: f, .. } = include
            && *f != fingerprint
        {
            *include = Include::All;
        }
    }

    /// The commit's files, in the status's order.
    pub fn selections(&self, status: &RepositoryStatus) -> Vec<FileSelection> {
        status
            .changes
            .iter()
            .filter(|c| !c.conflicted)
            .filter_map(|change| {
                let lines = match self.files.get(&change.path)? {
                    Include::Nothing => return None,
                    Include::All => None,
                    Include::Lines { fingerprint, lines } => Some(LineSelection {
                        fingerprint: *fingerprint,
                        selected: lines.iter().copied().collect(),
                    }),
                };
                Some(FileSelection {
                    path: change.path.clone(),
                    original_path: change.original_path.clone(),
                    lines,
                })
            })
            .collect()
    }

    pub fn any_included(&self) -> bool {
        self.files.values().any(|i| match i {
            Include::All => true,
            Include::Lines { lines, .. } => !lines.is_empty(),
            Include::Nothing => false,
        })
    }
}

/// The rows, filtered by a case-insensitive search on the path.
pub fn rows(
    status: &RepositoryStatus,
    inclusion: &Inclusion,
    selected: Option<&Path>,
    filter: &str,
) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    status
        .changes
        .iter()
        .filter(|c| filter.is_empty() || c.path.to_string_lossy().to_lowercase().contains(&filter))
        .map(|change| {
            let text = change.path.to_string_lossy();
            let (folder, name) = match text.rsplit_once('/') {
                Some((folder, name)) => (folder.to_string(), name.to_string()),
                None => (String::new(), text.to_string()),
            };
            Row {
                path: change.path.clone(),
                original_path: change.original_path.clone(),
                name,
                folder,
                badge: badge_of(change),
                check: inclusion.check(&change.path),
                selected: selected == Some(change.path.as_path()),
                conflicted: change.conflicted,
            }
        })
        .collect()
}

/// The changed lines of one hunk, to tick or untick it as a whole.
pub fn hunk_lines(diff: &FileDiff, hunk: usize) -> Vec<(usize, usize)> {
    let FileDiff::Text { hunks } = diff else {
        return Vec::new();
    };
    hunks
        .get(hunk)
        .map(|h| {
            h.lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.kind != DiffLineKind::Context)
                .map(|(l, _)| (hunk, l))
                .collect()
        })
        .unwrap_or_default()
}

/// Cuts a folder caption at its leading end, keeping the end that names the
/// file's place.
pub fn truncate_leading(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let keep: String = text
        .chars()
        .skip(count - max_chars.saturating_sub(1))
        .collect();
    format!("…{keep}")
}

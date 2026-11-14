//! Whether a dialog's primary button may be pressed, and if not, which field
//! says why.
//!
//! [`crate::dialogs::validate`] decides what is wrong with the text; this
//! adds what only the disk can tell (the folder a new repository would be
//! made in is already a repository, or already holds files) and the field
//! each reason belongs under. The window disables the primary button and
//! draws the reason under that field, so pressing Create, or Enter in a
//! field, never silently does nothing.

use std::path::Path;

use git_core::{Branch, Tag};

use crate::dialogs::{CloneTab, Dialog, Invalid, validate};

/// What is in a folder a dialog would create something in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Folder {
    /// Nothing there yet, or an empty folder.
    Free,
    /// A Git repository already.
    Repository,
    /// A folder with files in it.
    HasFiles,
    /// Something that is not a folder (a file) is in the way.
    NotAFolder,
}

/// Looks at the disk. Anything that cannot be read counts as free; the
/// create step reports what goes wrong itself.
pub fn folder(path: &Path) -> Folder {
    if path.join(".git").exists() {
        return Folder::Repository;
    }
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => Folder::NotAFolder,
        Ok(_) => match std::fs::read_dir(path).map(|mut entries| entries.next().is_some()) {
            Ok(true) => Folder::HasFiles,
            _ => Folder::Free,
        },
        Err(_) => Folder::Free,
    }
}

/// Why the primary button is off, and under which field to say so. `None`
/// for the field means the reason is about the dialog as a whole (nothing
/// chosen in a list) and goes under the last field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blocked {
    pub reason: Invalid,
    pub field: Option<usize>,
}

/// Checks a dialog the way the window does before its primary button.
pub fn check(
    dialog: &Dialog,
    texts: &[String],
    branches: &[Branch],
    tags: &[Tag],
    look: &dyn Fn(&Path) -> Folder,
) -> Option<Blocked> {
    let text = |i: usize| texts.get(i).map(|t| t.trim()).unwrap_or("");
    if let Some(reason) = validate(dialog, texts, branches, tags) {
        return Some(Blocked {
            reason,
            field: field_of(dialog, reason, texts),
        });
    }
    match dialog {
        Dialog::NewRepository { .. } => {
            let name = crate::publish::sanitize_repository_name(text(0));
            let target = Path::new(text(1)).join(&name);
            let reason = match look(&target) {
                Folder::Free => return None,
                Folder::Repository => Invalid::AlreadyRepository,
                Folder::HasFiles => Invalid::NotEmpty,
                Folder::NotAFolder => Invalid::NotAFolder,
            };
            Some(Blocked {
                reason,
                field: Some(0),
            })
        }
        _ => None,
    }
}

/// Which field a reason from [`validate`] is about.
fn field_of(dialog: &Dialog, reason: Invalid, texts: &[String]) -> Option<usize> {
    let text = |i: usize| texts.get(i).map(|t| t.trim()).unwrap_or("");
    let path_ok = |i: usize| !text(i).is_empty() && Path::new(text(i)).is_absolute();
    match dialog {
        _ if reason == Invalid::NoSelection => None,
        Dialog::Clone { tab, .. } => match tab {
            CloneTab::Url if text(0).is_empty() => Some(0),
            _ => Some(1),
        },
        Dialog::NewRepository { .. } => {
            if crate::publish::sanitize_repository_name(text(0)).is_empty() {
                Some(0)
            } else {
                Some(1)
            }
        }
        Dialog::AddWorktree { .. } => Some(if path_ok(0) { 1 } else { 0 }),
        Dialog::Identity => crate::identity::check(text(0), text(1)).map(|p| p.field()),
        Dialog::Squash { .. } => Some(2),
        _ => Some(0),
    }
}

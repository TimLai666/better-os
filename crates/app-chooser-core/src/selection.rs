//! What the chooser returns.
//!
//! The result is an application identity. Issue #4's rule that a desktop
//! application selection is never silently converted into an executable path is
//! enforced by construction here: the only constructor that can populate
//! `executable_path` is the one the separate Choose Executable mode calls, and
//! that mode never produces a persistent association.

use std::path::{Path, PathBuf};

use app_catalog_core::DesktopId;

/// Whether the choice applies to this one open or becomes the user's default
/// for the file type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationMode {
    /// Open once. Nothing persistent is written.
    Once,
    /// Always use for this file type. One MIME association is written, with a
    /// rollback record.
    Default,
}

impl AssociationMode {
    /// Whether choosing this mode changes anything on disk.
    pub fn is_persistent(self) -> bool {
        matches!(self, Self::Default)
    }
}

/// The chooser's typed result.
///
/// Fields are private so the invariants hold for every caller, not only for the
/// ones that remember them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSelection {
    desktop_id: DesktopId,
    action_id: Option<String>,
    association_mode: AssociationMode,
    executable_path: Option<PathBuf>,
}

impl AppSelection {
    /// Open this file with this application, once.
    pub fn open_once(desktop_id: DesktopId, action_id: Option<String>) -> Self {
        Self {
            desktop_id,
            action_id,
            association_mode: AssociationMode::Once,
            executable_path: None,
        }
    }

    /// Open this file with this application and make it the default for the
    /// file type.
    pub fn always_use(desktop_id: DesktopId, action_id: Option<String>) -> Self {
        Self {
            desktop_id,
            action_id,
            association_mode: AssociationMode::Default,
            executable_path: None,
        }
    }

    /// The result of the separate Choose Executable mode. A path reaches this
    /// constructor only after [`crate::executable`] has confirmed it resolves
    /// safely, and the selection is never persistent.
    pub fn executable(desktop_id: DesktopId, path: PathBuf) -> Self {
        Self {
            desktop_id,
            action_id: None,
            association_mode: AssociationMode::Once,
            executable_path: Some(path),
        }
    }

    pub fn desktop_id(&self) -> &DesktopId {
        &self.desktop_id
    }

    pub fn action_id(&self) -> Option<&str> {
        self.action_id.as_deref()
    }

    pub fn association_mode(&self) -> AssociationMode {
        self.association_mode
    }

    /// The executable path, which exists only in executable-selection mode.
    pub fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    /// Whether this selection came from the Choose Executable mode.
    pub fn is_executable_selection(&self) -> bool {
        self.executable_path.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> DesktopId {
        DesktopId::new("editor.desktop").unwrap()
    }

    #[test]
    fn open_once_carries_no_path_and_nothing_persistent() {
        let selection = AppSelection::open_once(id(), None);
        assert_eq!(selection.association_mode(), AssociationMode::Once);
        assert!(!selection.association_mode().is_persistent());
        assert!(selection.executable_path().is_none());
    }

    #[test]
    fn always_use_is_persistent_and_still_carries_no_path() {
        let selection = AppSelection::always_use(id(), Some("new-window".into()));
        assert_eq!(selection.association_mode(), AssociationMode::Default);
        assert!(selection.association_mode().is_persistent());
        assert_eq!(selection.action_id(), Some("new-window"));
        assert!(selection.executable_path().is_none());
    }

    #[test]
    fn an_executable_selection_is_never_a_persistent_association() {
        let selection = AppSelection::executable(id(), PathBuf::from("/usr/bin/editor"));
        assert_eq!(
            selection.executable_path(),
            Some(Path::new("/usr/bin/editor"))
        );
        assert!(selection.is_executable_selection());
        // The whole point of the separate mode: it cannot become a default.
        assert_eq!(selection.association_mode(), AssociationMode::Once);
    }

    #[test]
    fn no_constructor_produces_a_default_association_with_a_path() {
        let selections = [
            AppSelection::open_once(id(), None),
            AppSelection::always_use(id(), None),
            AppSelection::executable(id(), PathBuf::from("/usr/bin/editor")),
        ];
        for selection in selections {
            assert!(
                !(selection.association_mode().is_persistent()
                    && selection.executable_path().is_some()),
                "a persistent association must never carry an executable path"
            );
        }
    }
}

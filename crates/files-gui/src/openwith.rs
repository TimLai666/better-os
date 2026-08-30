//! Opening a file: which application, and who decides.
//!
//! Double-clicking a file and choosing Open With are the same question asked
//! two ways, so they resolve through one function, [`route_open_file`]. The
//! difference is only what happens when there is no answer: a double-click with
//! no effective default opens the chooser, which is the behaviour Issue #6 asks
//! for and also the only honest one — a file manager that silently does nothing
//! is worse than one that asks.
//!
//! **Better Files makes no association decision of its own.** The effective
//! default comes from `mimeapps.list` through `app-chooser-core`, exactly as the
//! chooser reads it, and writing one goes through `AssociationStore`, which
//! captures its rollback record before the first change and edits a single line
//! so foreign content survives byte for byte. Removing Better Files therefore
//! cannot erase an unrelated association: there is no path here that rewrites
//! the file wholesale.
//!
//! **Open Once has no side effect.** The chooser launches the selection and
//! writes nothing. That property belongs to `app-chooser-core` and is asserted
//! there; what this module contributes is not adding a second write path
//! beside it.

use app_catalog_core::{DesktopId, LaunchTarget, MimeType};
use app_chooser_core::{AssociationStore, MimeGraph};
use files_core::LocalPath;

use crate::apps::CatalogHandle;

/// What opening this file should do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenRoute {
    /// There is an effective default and the catalog has it. Launch it.
    LaunchWith {
        desktop_id: DesktopId,
        mime: MimeType,
    },
    /// The type is known and nothing handles it, or the handler named in
    /// `mimeapps.list` is not installed any more. Ask.
    AskChooser { mime: MimeType },
    /// The type could not be resolved at all. The chooser needs one, so this is
    /// reported rather than turned into `application/octet-stream`, which would
    /// be a guess presented as a fact.
    NoMimeType,
}

/// Where the effective default came from.
///
/// Kept apart from the answer because the two failure modes look identical to a
/// user and are not: nothing is associated, versus something is associated and
/// is no longer installed. The second is worth saying out loud.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultSource {
    /// `mimeapps.list` names it and the catalog has it.
    UserAssociation,
    /// `mimeapps.list` names it and the catalog does not have it.
    AssociationMissingApplication,
    /// Nothing is associated.
    None,
}

/// Reads what a MIME type resolves to right now.
///
/// A trait so the routing tests do not need a `mimeapps.list` on the host and
/// so a session can hold one loaded copy rather than re-reading the file per
/// double-click.
pub trait DefaultHandlers {
    fn default_for(&self, mime: &MimeType) -> Option<DesktopId>;
}

/// The real answer, from the session's `mimeapps.list`.
///
/// Loaded once and re-read on demand. `app-chooser-core` owns the parsing; this
/// is a cache with a reload, not a second parser.
pub struct SessionDefaults {
    associations: Vec<(String, DesktopId)>,
}

impl SessionDefaults {
    /// Reads the user's `mimeapps.list`. An unreadable or absent file is an
    /// empty set of associations, which is what a fresh account genuinely has.
    pub fn from_env() -> Self {
        let associations = AssociationStore::for_user()
            .ok()
            .and_then(|store| store.load().ok())
            .map(|file| {
                let parsed = file.associations();
                // The parsed view borrows the file, so the pairs are copied out
                // rather than the borrow being kept alive.
                //
                // Every key that appears anywhere in the file is offered to
                // `default_for`, which applies the group precedence itself. A
                // type mentioned only under Added Associations therefore
                // answers `None` here rather than being mistaken for a default.
                let mut pairs: Vec<(String, DesktopId)> = Vec::new();
                for line in file.lines() {
                    let Some((left, _)) = line.text.split_once('=') else {
                        continue;
                    };
                    let Some(mime) = MimeType::parse(left.trim()) else {
                        continue;
                    };
                    let key = mime.as_str().to_string();
                    if pairs.iter().any(|(existing, _)| *existing == key) {
                        continue;
                    }
                    if let Some(id) = parsed.default_for(&mime) {
                        pairs.push((key, id.clone()));
                    }
                }
                pairs
            })
            .unwrap_or_default();
        Self { associations }
    }

    /// A fixed set, for tests and for a session that has been handed its
    /// associations from somewhere else.
    pub fn fixed(pairs: impl IntoIterator<Item = (String, DesktopId)>) -> Self {
        Self {
            associations: pairs.into_iter().collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.associations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.associations.is_empty()
    }
}

impl DefaultHandlers for SessionDefaults {
    fn default_for(&self, mime: &MimeType) -> Option<DesktopId> {
        self.associations
            .iter()
            .find(|(key, _)| key == mime.as_str())
            .map(|(_, id)| id.clone())
    }
}

/// Resolves a file's MIME type, canonicalizing aliases through the shared MIME
/// database when one was not already detected by the listing.
pub fn resolve_mime(
    graph: &MimeGraph,
    detected: Option<&MimeType>,
    file_name: &str,
) -> Option<MimeType> {
    detected
        .cloned()
        .or_else(|| graph.guess_from_file_name(file_name))
        .map(|mime| graph.canonical(&mime))
}

/// Decides what a double-click does.
pub fn route_open_file(
    mime: Option<&MimeType>,
    handlers: &dyn DefaultHandlers,
    catalog: &CatalogHandle,
) -> (OpenRoute, DefaultSource) {
    let Some(mime) = mime else {
        return (OpenRoute::NoMimeType, DefaultSource::None);
    };
    match handlers.default_for(mime) {
        Some(desktop_id) => {
            if catalog.snapshot().get(&desktop_id).is_some() {
                (
                    OpenRoute::LaunchWith {
                        desktop_id,
                        mime: mime.clone(),
                    },
                    DefaultSource::UserAssociation,
                )
            } else {
                // The association names an application that is not installed.
                // Asking is right; launching nothing is not, and neither is
                // pretending nothing was ever associated.
                (
                    OpenRoute::AskChooser { mime: mime.clone() },
                    DefaultSource::AssociationMissingApplication,
                )
            }
        }
        None => (
            OpenRoute::AskChooser { mime: mime.clone() },
            DefaultSource::None,
        ),
    }
}

/// Everything the embedded chooser needs to be opened for a file.
///
/// The chooser is a GPUI entity that takes exactly these three things, so this
/// is what the session hands the window. Building it here keeps the window from
/// deciding what a display name or a launch target is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChooserRequest {
    pub display_name: String,
    pub path: LocalPath,
    pub mime: MimeType,
}

impl ChooserRequest {
    /// Builds a request, or `None` when the path cannot be a launch target.
    pub fn new(path: &LocalPath, mime: MimeType) -> Option<Self> {
        // Proving the target is buildable here means the chooser is never
        // opened for a file it could not have launched anyway.
        LaunchTarget::path(path.as_path().to_path_buf()).ok()?;
        Some(Self {
            display_name: path.file_name(),
            path: path.clone(),
            mime,
        })
    }

    pub fn target(&self) -> LaunchTarget {
        LaunchTarget::path(self.path.as_path().to_path_buf())
            .expect("the target was proven buildable when the request was made")
    }
}

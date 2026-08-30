//! The Applications virtual location.
//!
//! Issue #4 forbids implementing this by symlinking, copying `.desktop` files
//! into a fake directory, or inventing paths a program might mistake for
//! executables. What is left is the honest implementation: the location is a
//! view over records the shared catalog already produced, and its rows are
//! [`EntryBody::Application`], which has no path accessor at all.
//!
//! No scanning happens here. A consumer injects a catalog snapshot — from
//! `app-catalog-platform`, or a hand-built one in a test — and this module
//! turns it into entries. `files-platform` never parses a desktop entry,
//! which is the seam ENG.md calls "one catalog, no second scanner".
//!
//! Opening a row produces an [`OpenIntent`], not a process. Spawning stays in
//! `app-catalog-platform`, so nothing in Better Files builds a command line.

use app_catalog_core::{Catalog, DesktopEnvironments, DesktopId, Locale, MimeType, Visibility};

use crate::entry::{
    ApplicationFacts, Entry, EntryBody, EntryKind, EntrySize, HiddenState, PermissionsSummary,
};
use crate::listing::{Cancelled, ListingSink};
use crate::location::{LocalPath, Location};

/// What the consumer knows about the session, so the same catalog produces the
/// right rows for the desktop the user is actually running.
#[derive(Clone, Debug, Default)]
pub struct ApplicationView {
    pub environments: DesktopEnvironments,
    pub locale: Option<Locale>,
    /// Include entries the catalog marked `NoDisplay` or excluded for this
    /// desktop. Off by default; a diagnostics view turns it on.
    pub include_hidden: bool,
}

/// Turns one catalog record into a listing row.
///
/// The name is the localized one the catalog resolved. Nothing here re-reads
/// a desktop file or invents a field the entry did not have.
pub fn entry_for(
    record: &app_catalog_core::ApplicationRecord,
    locale: Option<&Locale>,
    visible: bool,
) -> Entry {
    Entry {
        name: record.display_name(locale).to_string(),
        kind: EntryKind::Application,
        // An application has no file size. Reporting zero would sort every
        // application to one end of a size sort as if they were empty.
        size: EntrySize::NotApplicable,
        modified: None,
        permissions: PermissionsSummary::UNKNOWN,
        hidden: if visible {
            HiddenState::Visible
        } else {
            // An application excluded by its own `NoDisplay` or by this
            // desktop's rules is hidden in the same sense a dotfile is: it
            // exists and the "show hidden" preference reveals it.
            HiddenState::Hidden(crate::entry::HiddenReason::Dotfile)
        },
        mime: None,
        body: EntryBody::Application(ApplicationFacts {
            desktop_id: record.desktop_id.clone(),
            icon: record.icon.clone(),
            categories: record.categories.clone(),
            comment: record
                .comment
                .as_ref()
                .map(|text| text.resolve(locale).to_string()),
        }),
    }
}

/// Streams the Applications location from an injected catalog.
///
/// It uses the same [`ListingSink`] as a directory read, so a view draws the
/// Applications location with the code it already has, and cancellation works
/// identically: navigating away mid-list stops here too.
pub fn list_applications(
    catalog: &Catalog,
    view: &ApplicationView,
    sink: &mut ListingSink,
) -> Result<(), Cancelled> {
    for record in catalog.records() {
        let visibility = record.visibility_in(&view.environments);
        let visible = visibility == Visibility::Visible;
        if !visible && !view.include_hidden {
            continue;
        }
        sink.push(entry_for(record, view.locale.as_ref(), visible))?;
    }
    Ok(())
}

/// What "open this" means for a typed entry.
///
/// A closed enum rather than a `Result<PathBuf>`: opening an application, a
/// folder, and a file are three different actions, and the type says which one
/// the caller is being handed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenIntent {
    /// Go into this location. Folders, and mounted device folders.
    Navigate(Box<Location>),
    /// Launch this application through the shared catalog. The actual spawn is
    /// `app-catalog-platform`'s job.
    Launch {
        desktop_id: DesktopId,
        /// A desktop action such as "New Window", when one was chosen.
        action: Option<String>,
    },
    /// Open this file with whatever the session's association resolves to.
    /// Resolution is the chooser's job, not this crate's.
    OpenFile {
        path: LocalPath,
        mime: Option<MimeType>,
    },
    /// The entry cannot be opened, and why. A broken symlink and a trashed
    /// item are both refusals a view renders rather than errors reported after
    /// an attempt fails.
    Refused(OpenRefusal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenRefusal {
    /// A symlink whose target is not there.
    BrokenSymlink,
    /// A symlink chain that never terminates.
    SymlinkLoop,
    /// A trashed item. Restore it first; ticket 33 owns restore.
    ItemIsInTrash,
    /// A socket, FIFO, or device node.
    NotOpenable,
}

/// Decides what opening one entry does.
pub fn open_intent(entry: &Entry) -> OpenIntent {
    match &entry.body {
        EntryBody::Application(facts) => OpenIntent::Launch {
            desktop_id: facts.desktop_id.clone(),
            action: None,
        },
        EntryBody::Trashed(_) => OpenIntent::Refused(OpenRefusal::ItemIsInTrash),
        EntryBody::File(facts) => match &facts.symlink {
            crate::entry::SymlinkStatus::Broken { .. } => {
                OpenIntent::Refused(OpenRefusal::BrokenSymlink)
            }
            crate::entry::SymlinkStatus::Loop { .. } => {
                OpenIntent::Refused(OpenRefusal::SymlinkLoop)
            }
            _ => match entry.kind {
                EntryKind::Directory => {
                    OpenIntent::Navigate(Box::new(Location::Local(facts.path.clone())))
                }
                EntryKind::File => OpenIntent::OpenFile {
                    path: facts.path.clone(),
                    mime: entry.mime.clone(),
                },
                EntryKind::Special | EntryKind::Unknown | EntryKind::Application => {
                    OpenIntent::Refused(OpenRefusal::NotOpenable)
                }
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::SymlinkStatus;
    use crate::listing::{ListingEvent, ListingRequest, ListingSession};
    use app_catalog_core::{CatalogBuilder, EntryScope, ExecutableProbe};
    use std::path::PathBuf;

    struct AlwaysResolves;

    impl ExecutableProbe for AlwaysResolves {
        fn resolve(&self, program: &str) -> Option<PathBuf> {
            Some(PathBuf::from("/usr/bin").join(program))
        }
    }

    fn catalog_with(entries: &[(&str, &str)]) -> Catalog {
        let probe = AlwaysResolves;
        let mut builder = CatalogBuilder::new(&probe);
        let directory = app_catalog_core::DirectoryRank {
            rank: 0,
            scope: EntryScope::System,
        };
        for (id, contents) in entries {
            builder.add_entry(
                DesktopId::new(*id).expect("desktop id"),
                PathBuf::from(format!("/usr/share/applications/{id}")),
                &directory,
                contents.as_bytes(),
            );
        }
        builder.build()
    }

    const EDITOR: &str = "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\nComment=Edits text\nCategories=Utility;\n";
    const HIDDEN: &str =
        "[Desktop Entry]\nType=Application\nName=Daemon\nExec=daemon\nNoDisplay=true\n";

    #[test]
    fn an_application_row_has_no_path_and_launches_by_desktop_id() {
        let catalog = catalog_with(&[("org.example.Editor.desktop", EDITOR)]);
        let record = catalog.records().next().unwrap();
        let entry = entry_for(record, None, true);
        assert_eq!(entry.name, "Editor");
        assert_eq!(entry.kind, EntryKind::Application);
        // The rule this whole location exists to keep.
        assert_eq!(entry.as_local_path(), None);
        assert_eq!(
            open_intent(&entry),
            OpenIntent::Launch {
                desktop_id: DesktopId::new("org.example.Editor.desktop").unwrap(),
                action: None,
            }
        );
    }

    #[test]
    fn listing_applications_streams_through_the_same_sink_as_a_directory() {
        let catalog = catalog_with(&[
            ("org.example.Editor.desktop", EDITOR),
            ("org.example.Daemon.desktop", HIDDEN),
        ]);
        let request = ListingRequest::new(Location::Applications).with_batch_size(1);
        let (mut session, mut sink) = ListingSession::start(&request);
        list_applications(&catalog, &ApplicationView::default(), &mut sink).unwrap();
        sink.finish().unwrap();
        let mut names = Vec::new();
        for event in session.drain() {
            if let ListingEvent::Batch(batch) = event {
                names.extend(batch.entries.into_iter().map(|entry| entry.name));
            }
        }
        assert_eq!(names, ["Editor"]);
    }

    #[test]
    fn a_no_display_application_appears_only_when_hidden_entries_are_included() {
        let catalog = catalog_with(&[("org.example.Daemon.desktop", HIDDEN)]);
        let request = ListingRequest::new(Location::Applications);
        let (mut session, mut sink) = ListingSession::start(&request);
        let view = ApplicationView {
            include_hidden: true,
            ..ApplicationView::default()
        };
        list_applications(&catalog, &view, &mut sink).unwrap();
        sink.finish().unwrap();
        let mut hidden_flags = Vec::new();
        for event in session.drain() {
            if let ListingEvent::Batch(batch) = event {
                hidden_flags.extend(batch.entries.iter().map(|entry| entry.hidden.is_hidden()));
            }
        }
        assert_eq!(hidden_flags, [true]);
    }

    #[test]
    fn a_cancelled_applications_listing_stops_partway() {
        let catalog = catalog_with(&[
            ("org.example.Editor.desktop", EDITOR),
            ("org.example.Second.desktop", EDITOR),
        ]);
        let request = ListingRequest::new(Location::Applications).with_batch_size(1);
        let (session, mut sink) = ListingSession::start(&request);
        session.cancel();
        assert_eq!(
            list_applications(&catalog, &ApplicationView::default(), &mut sink),
            Err(Cancelled)
        );
    }

    #[test]
    fn opening_a_folder_navigates_and_a_broken_link_is_refused() {
        let folder = Entry::file(
            "Documents",
            LocalPath::new("/home/user/Documents").unwrap(),
            EntryKind::Directory,
        );
        assert_eq!(
            open_intent(&folder),
            OpenIntent::Navigate(Box::new(Location::local("/home/user/Documents").unwrap()))
        );

        let mut broken = Entry::file(
            "link",
            LocalPath::new("/home/user/link").unwrap(),
            EntryKind::Unknown,
        );
        if let EntryBody::File(facts) = &mut broken.body {
            facts.symlink = SymlinkStatus::Broken {
                target: PathBuf::from("/gone"),
            };
        }
        assert_eq!(
            open_intent(&broken),
            OpenIntent::Refused(OpenRefusal::BrokenSymlink)
        );
    }
}

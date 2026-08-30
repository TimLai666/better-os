//! Assembly of many desktop entries into one catalog.
//!
//! Two entries can claim the same desktop ID. The specification resolves that
//! by directory precedence, and the winner is the whole answer: a user entry
//! that sets `Hidden=true` deletes the system application rather than sitting
//! beside it. Losers and rejects are kept as diagnostics instead of being
//! thrown away, because "why is this application missing" is the question a
//! catalog gets asked most.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::entry::{DesktopFile, Locale};
use crate::error::EntryError;
use crate::record::{
    ApplicationRecord, DesktopEnvironments, DesktopId, EntryScope, ExecutableProbe, MimeType,
    NoProbe,
};

/// One application directory, ranked by how much it outranks the others.
/// Rank 0 wins; `$XDG_DATA_HOME/applications` is normally rank 0 and each
/// `$XDG_DATA_DIRS` entry follows in the order the variable lists them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryRank {
    pub rank: usize,
    pub scope: EntryScope,
}

/// An entry that lost to a higher-precedence entry with the same desktop ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShadowedEntry {
    pub desktop_id: DesktopId,
    pub path: PathBuf,
    pub rank: usize,
}

/// An entry that never became a record, with the machine key saying why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectedEntry {
    pub path: PathBuf,
    pub desktop_id: Option<DesktopId>,
    pub error: EntryError,
}

struct Candidate {
    record: ApplicationRecord,
    rank: usize,
}

/// Accumulates entries and resolves precedence as they arrive.
pub struct CatalogBuilder<'probe> {
    probe: &'probe dyn ExecutableProbe,
    candidates: BTreeMap<DesktopId, Candidate>,
    shadowed: Vec<ShadowedEntry>,
    rejected: Vec<RejectedEntry>,
}

impl Default for CatalogBuilder<'static> {
    fn default() -> Self {
        Self::new(&NoProbe)
    }
}

impl<'probe> CatalogBuilder<'probe> {
    pub fn new(probe: &'probe dyn ExecutableProbe) -> Self {
        Self {
            probe,
            candidates: BTreeMap::new(),
            shadowed: Vec::new(),
            rejected: Vec::new(),
        }
    }

    /// Adds one entry's bytes. A rejection is recorded, not returned: one bad
    /// file in `/usr/share/applications` must not empty the catalog.
    pub fn add_entry(
        &mut self,
        desktop_id: DesktopId,
        path: PathBuf,
        directory: &DirectoryRank,
        bytes: &[u8],
    ) {
        // A lower-ranked file cannot win, so it is not even parsed.
        if let Some(existing) = self.candidates.get(&desktop_id) {
            if existing.rank <= directory.rank {
                self.shadowed.push(ShadowedEntry {
                    desktop_id,
                    path,
                    rank: directory.rank,
                });
                return;
            }
        }
        let parsed = DesktopFile::parse_bytes(bytes).and_then(|file| {
            ApplicationRecord::from_desktop_file(
                desktop_id.clone(),
                path.clone(),
                directory.scope,
                &file,
                self.probe,
            )
        });
        match parsed {
            Ok(record) => {
                if let Some(previous) = self.candidates.insert(
                    desktop_id.clone(),
                    Candidate {
                        record,
                        rank: directory.rank,
                    },
                ) {
                    self.shadowed.push(ShadowedEntry {
                        desktop_id,
                        path: previous.record.source.path,
                        rank: previous.rank,
                    });
                }
            }
            Err(error) => self.rejected.push(RejectedEntry {
                path,
                desktop_id: Some(desktop_id),
                error,
            }),
        }
    }

    /// Records a file that could not even be read.
    pub fn reject(&mut self, path: PathBuf, desktop_id: Option<DesktopId>, error: EntryError) {
        self.rejected.push(RejectedEntry {
            path,
            desktop_id,
            error,
        });
    }

    pub fn build(self) -> Catalog {
        let mut records = BTreeMap::new();
        let mut hidden = Vec::new();
        for (desktop_id, candidate) in self.candidates {
            if candidate.record.visibility.hidden {
                // `Hidden=true` on the winning entry deletes the ID outright.
                hidden.push(desktop_id);
                continue;
            }
            records.insert(desktop_id, candidate.record);
        }
        Catalog {
            records,
            hidden,
            shadowed: self.shadowed,
            rejected: self.rejected,
        }
    }
}

/// The assembled catalog. Records are keyed by desktop ID and ordered by it,
/// so two runs over the same directories produce the same list.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    records: BTreeMap<DesktopId, ApplicationRecord>,
    hidden: Vec<DesktopId>,
    shadowed: Vec<ShadowedEntry>,
    rejected: Vec<RejectedEntry>,
}

impl Catalog {
    pub fn get(&self, desktop_id: &DesktopId) -> Option<&ApplicationRecord> {
        self.records.get(desktop_id)
    }

    /// Every record that survived precedence, including ones that are not
    /// displayed. A chooser wants this; a menu wants `visible`.
    pub fn records(&self) -> impl Iterator<Item = &ApplicationRecord> {
        self.records.values()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Records the current desktop environment should display.
    pub fn visible<'a>(
        &'a self,
        environments: &'a DesktopEnvironments,
    ) -> impl Iterator<Item = &'a ApplicationRecord> {
        self.records
            .values()
            .filter(move |record| record.visibility_in(environments).is_visible())
    }

    /// Records declaring support for a MIME type, visible ones only.
    pub fn supporting_mime_type<'a>(
        &'a self,
        mime: &'a MimeType,
        environments: &'a DesktopEnvironments,
    ) -> impl Iterator<Item = &'a ApplicationRecord> {
        self.visible(environments)
            .filter(move |record| record.supports_mime_type(mime))
    }

    /// Desktop IDs deleted by a `Hidden=true` entry.
    pub fn hidden(&self) -> &[DesktopId] {
        &self.hidden
    }

    /// Entries that lost to a higher-precedence entry with the same ID.
    pub fn shadowed(&self) -> &[ShadowedEntry] {
        &self.shadowed
    }

    /// Entries that were refused, each with a stable machine key.
    pub fn rejected(&self) -> &[RejectedEntry] {
        &self.rejected
    }

    /// Records sorted for presentation in one locale. Ties break on desktop ID
    /// so the order never depends on iteration luck.
    pub fn sorted_by_name<'a>(
        &'a self,
        locale: Option<&Locale>,
        environments: &DesktopEnvironments,
    ) -> Vec<&'a ApplicationRecord> {
        let mut records: Vec<&ApplicationRecord> = self
            .records
            .values()
            .filter(|record| record.visibility_in(environments).is_visible())
            .collect();
        records.sort_by(|left, right| {
            left.display_name(locale)
                .to_lowercase()
                .cmp(&right.display_name(locale).to_lowercase())
                .then_with(|| left.desktop_id.cmp(&right.desktop_id))
        });
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> DirectoryRank {
        DirectoryRank {
            rank: 0,
            scope: EntryScope::User,
        }
    }

    fn system(rank: usize) -> DirectoryRank {
        DirectoryRank {
            rank,
            scope: EntryScope::System,
        }
    }

    fn entry(name: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=app\n{extra}")
    }

    #[test]
    fn a_user_entry_shadows_a_system_entry_with_the_same_id() {
        let mut builder = CatalogBuilder::default();
        let id = DesktopId::new("editor.desktop").unwrap();
        builder.add_entry(
            id.clone(),
            PathBuf::from("/home/user/.local/share/applications/editor.desktop"),
            &user(),
            entry("User Editor", "").as_bytes(),
        );
        builder.add_entry(
            id.clone(),
            PathBuf::from("/usr/share/applications/editor.desktop"),
            &system(1),
            entry("System Editor", "").as_bytes(),
        );
        let catalog = builder.build();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.get(&id).unwrap().display_name(None), "User Editor");
        assert_eq!(catalog.shadowed().len(), 1);
        assert_eq!(
            catalog.shadowed()[0].path,
            PathBuf::from("/usr/share/applications/editor.desktop")
        );
    }

    #[test]
    fn precedence_does_not_depend_on_insertion_order() {
        let id = DesktopId::new("editor.desktop").unwrap();
        let mut builder = CatalogBuilder::default();
        builder.add_entry(
            id.clone(),
            PathBuf::from("/usr/share/applications/editor.desktop"),
            &system(2),
            entry("System Editor", "").as_bytes(),
        );
        builder.add_entry(
            id.clone(),
            PathBuf::from("/usr/local/share/applications/editor.desktop"),
            &system(1),
            entry("Local Editor", "").as_bytes(),
        );
        let catalog = builder.build();
        assert_eq!(catalog.get(&id).unwrap().display_name(None), "Local Editor");
        assert_eq!(catalog.shadowed().len(), 1);
    }

    #[test]
    fn a_hidden_user_entry_deletes_the_system_application() {
        let id = DesktopId::new("editor.desktop").unwrap();
        let mut builder = CatalogBuilder::default();
        builder.add_entry(
            id.clone(),
            PathBuf::from("/home/user/.local/share/applications/editor.desktop"),
            &user(),
            entry("Editor", "Hidden=true\n").as_bytes(),
        );
        builder.add_entry(
            id.clone(),
            PathBuf::from("/usr/share/applications/editor.desktop"),
            &system(1),
            entry("Editor", "").as_bytes(),
        );
        let catalog = builder.build();
        assert!(catalog.is_empty());
        assert_eq!(catalog.hidden(), &[id]);
    }

    #[test]
    fn a_rejected_entry_does_not_empty_the_catalog() {
        let mut builder = CatalogBuilder::default();
        builder.add_entry(
            DesktopId::new("good.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/good.desktop"),
            &system(1),
            entry("Good", "").as_bytes(),
        );
        builder.add_entry(
            DesktopId::new("bad.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/bad.desktop"),
            &system(1),
            b"not a desktop entry at all",
        );
        let catalog = builder.build();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.rejected().len(), 1);
        assert_eq!(
            catalog.rejected()[0].error.to_string(),
            "catalog.error.content_before_group:1"
        );
    }

    #[test]
    fn visible_applies_every_exclusion_rule() {
        let mut builder = CatalogBuilder::default();
        let cases = [
            ("plain.desktop", ""),
            ("nodisplay.desktop", "NoDisplay=true\n"),
            ("kde.desktop", "OnlyShowIn=KDE;\n"),
            ("notgnome.desktop", "NotShowIn=GNOME;\n"),
            ("missing.desktop", "TryExec=absent-program\n"),
        ];
        for (id, extra) in cases {
            builder.add_entry(
                DesktopId::new(id).unwrap(),
                PathBuf::from(format!("/usr/share/applications/{id}")),
                &system(1),
                entry("App", extra).as_bytes(),
            );
        }
        let catalog = builder.build();
        assert_eq!(catalog.len(), 5);
        let environments = DesktopEnvironments::parse("ubuntu:GNOME");
        let visible: Vec<&str> = catalog
            .visible(&environments)
            .map(|record| record.desktop_id.as_str())
            .collect();
        assert_eq!(visible, vec!["plain.desktop"]);
    }

    #[test]
    fn mime_lookup_returns_only_visible_supporting_records() {
        let mut builder = CatalogBuilder::default();
        builder.add_entry(
            DesktopId::new("shown.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/shown.desktop"),
            &system(1),
            entry("Shown", "MimeType=text/plain;\n").as_bytes(),
        );
        builder.add_entry(
            DesktopId::new("hidden.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/hidden.desktop"),
            &system(1),
            entry("Hidden", "MimeType=text/plain;\nNoDisplay=true\n").as_bytes(),
        );
        let catalog = builder.build();
        let mime = MimeType::parse("text/plain").unwrap();
        let environments = DesktopEnvironments::parse("GNOME");
        let found: Vec<&str> = catalog
            .supporting_mime_type(&mime, &environments)
            .map(|record| record.desktop_id.as_str())
            .collect();
        assert_eq!(found, vec!["shown.desktop"]);
    }

    #[test]
    fn sorting_is_locale_aware_and_deterministic() {
        let mut builder = CatalogBuilder::default();
        builder.add_entry(
            DesktopId::new("b.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/b.desktop"),
            &system(1),
            "[Desktop Entry]\nType=Application\nName=Zebra\nName[de]=Alpha\nExec=b\n".as_bytes(),
        );
        builder.add_entry(
            DesktopId::new("a.desktop").unwrap(),
            PathBuf::from("/usr/share/applications/a.desktop"),
            &system(1),
            "[Desktop Entry]\nType=Application\nName=Mango\nExec=a\n".as_bytes(),
        );
        let catalog = builder.build();
        let environments = DesktopEnvironments::default();
        let default_order: Vec<&str> = catalog
            .sorted_by_name(None, &environments)
            .iter()
            .map(|record| record.desktop_id.as_str())
            .collect();
        assert_eq!(default_order, vec!["a.desktop", "b.desktop"]);
        let german_order: Vec<&str> = catalog
            .sorted_by_name(Locale::parse("de_DE").as_ref(), &environments)
            .iter()
            .map(|record| record.desktop_id.as_str())
            .collect();
        assert_eq!(german_order, vec!["b.desktop", "a.desktop"]);
    }
}

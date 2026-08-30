//! Which applications to offer, in what order, and why.
//!
//! Three sections, exactly as Issue #4 requires: applications that explicitly
//! declare the selected type, applications that are compatible some other way
//! or have been used for it before, and everything else behind an explicit
//! expansion. The reason each application landed where it did travels with it
//! as a [`Compatibility`], because the surface has to be able to say, in the
//! user's own words, that an application does not declare the file's type.
//!
//! Ranking is deterministic and does no I/O, so it can run off the render
//! thread and be benchmarked against a synthetic catalog.

use app_catalog_core::{
    ApplicationRecord, DesktopEnvironments, DesktopId, EntryScope, Locale, MimeType, SourceKind,
};

use crate::mime::MimeResolution;
use crate::mimeapps::MimeAssociations;

/// Applications the user has opened this type with before, most recent first.
/// Better OS does not decide where this comes from; a caller may build it from
/// its own history or from the associations file.
#[derive(Clone, Debug, Default)]
pub struct UsageHistory {
    entries: Vec<DesktopId>,
}

impl UsageHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a history from the applications the associations file already
    /// mentions for this type, which is the only evidence available before
    /// Better OS has recorded any of its own.
    pub fn from_associations(associations: &MimeAssociations, mime: &MimeType) -> Self {
        Self {
            entries: associations.mentioned(mime),
        }
    }

    /// Records a use, moving it to the front.
    pub fn record(&mut self, desktop_id: DesktopId) {
        self.entries.retain(|entry| entry != &desktop_id);
        self.entries.insert(0, desktop_id);
    }

    /// How recently the application was used, 0 being most recent.
    pub fn position(&self, desktop_id: &DesktopId) -> Option<usize> {
        self.entries.iter().position(|entry| entry == desktop_id)
    }

    pub fn entries(&self) -> &[DesktopId] {
        &self.entries
    }
}

/// Why an application is being offered, and how strongly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Compatibility {
    /// The entry names this exact type, or an alias of it.
    Declares,
    /// The entry names a wildcard covering this type, such as `text/*`.
    DeclaresWildcard { pattern: MimeType },
    /// The entry names a more general type the selected one inherits from.
    DeclaresRelatedType { declared: MimeType, distance: usize },
    /// The user's own associations file names it for this type, though the
    /// entry itself does not.
    UserAssociated,
    /// It has been used for this type before, and declares nothing about it.
    PreviouslyUsed,
    /// It says nothing about this type at all. Choosing it is allowed and must
    /// be explained.
    NotDeclared,
}

impl Compatibility {
    /// Whether the application itself claims to handle the selected type. The
    /// surface explains the choice whenever this is false.
    pub fn declares_selected_type(&self) -> bool {
        matches!(self, Self::Declares)
    }

    /// Whether the surface must explain the choice to the user.
    pub fn needs_explanation(&self) -> bool {
        !self.declares_selected_type()
    }

    /// Order within a section. Lower sorts first. The second element separates
    /// a near ancestor from a distant one, so `text/plain` outranks
    /// `application/octet-stream` for a Rust source file.
    fn strength(&self) -> (u8, usize) {
        match self {
            Self::Declares => (0, 0),
            Self::UserAssociated => (1, 0),
            Self::PreviouslyUsed => (2, 0),
            Self::DeclaresRelatedType { distance, .. } => (3, *distance),
            Self::DeclaresWildcard { .. } => (4, 0),
            Self::NotDeclared => (5, 0),
        }
    }
}

/// One offered application, flattened for presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankedApplication {
    pub desktop_id: DesktopId,
    pub display_name: String,
    pub compatibility: Compatibility,
    pub source_kind: SourceKind,
    pub scope: EntryScope,
    /// The user's current default for this type.
    pub is_default: bool,
    pub previously_used: bool,
    /// The entry declares no single canonical executable, which the executable
    /// mode has to refuse.
    pub has_canonical_executable: bool,
}

/// The three sections the chooser shows.
#[derive(Clone, Debug, Default)]
pub struct ChooserSections {
    /// Applications that explicitly declare the selected type, plus the user's
    /// current default.
    pub recommended: Vec<RankedApplication>,
    /// Compatible through a more general or wildcard type, associated by the
    /// user, or previously used.
    pub other: Vec<RankedApplication>,
    /// Everything else, shown only behind an explicit expansion.
    pub all: Vec<RankedApplication>,
}

impl ChooserSections {
    pub fn is_empty(&self) -> bool {
        self.recommended.is_empty() && self.other.is_empty() && self.all.is_empty()
    }

    /// Every offered application, in section order.
    pub fn iter(&self) -> impl Iterator<Item = &RankedApplication> {
        self.recommended.iter().chain(&self.other).chain(&self.all)
    }

    pub fn find(&self, desktop_id: &DesktopId) -> Option<&RankedApplication> {
        self.iter().find(|entry| &entry.desktop_id == desktop_id)
    }

    /// Why this application was offered, which is what the explain-state on the
    /// chooser renders.
    pub fn compatibility_of(&self, desktop_id: &DesktopId) -> Option<&Compatibility> {
        self.find(desktop_id).map(|entry| &entry.compatibility)
    }
}

/// What ranking needs to know beyond the records themselves.
pub struct ChooserRequest<'a> {
    pub resolution: &'a MimeResolution,
    pub associations: &'a MimeAssociations,
    pub history: &'a UsageHistory,
    pub environments: &'a DesktopEnvironments,
    pub locale: Option<&'a Locale>,
}

/// Sorts the catalog's records into the three sections.
///
/// Excluded entries never appear: an application the desktop hides is hidden
/// here too, including behind the expansion.
pub fn rank<'a, I>(records: I, request: &ChooserRequest<'_>) -> ChooserSections
where
    I: IntoIterator<Item = &'a ApplicationRecord>,
{
    let default_id = request
        .associations
        .default_for(&request.resolution.primary);
    let mut sections = ChooserSections::default();
    for record in records {
        if !record.visibility_in(request.environments).is_visible() {
            continue;
        }
        let is_default = default_id == Some(&record.desktop_id);
        let removed = request
            .associations
            .is_removed(&request.resolution.primary, &record.desktop_id);
        let previously_used = request.history.position(&record.desktop_id).is_some();
        let compatibility = classify(record, request, removed, previously_used);

        let entry = RankedApplication {
            desktop_id: record.desktop_id.clone(),
            display_name: record.display_name(request.locale).to_string(),
            compatibility,
            source_kind: record.source.kind,
            scope: record.source.scope,
            is_default,
            previously_used,
            has_canonical_executable: !matches!(
                record.executable,
                app_catalog_core::ExecutableStatus::NotApplicable { .. }
            ),
        };

        // The user's own default belongs at the top of Recommended whatever the
        // entry declares: the user already decided.
        let declares = entry.compatibility.declares_selected_type();
        if is_default || (declares && !removed) {
            sections.recommended.push(entry);
        } else if !removed && !matches!(entry.compatibility, Compatibility::NotDeclared) {
            sections.other.push(entry);
        } else {
            sections.all.push(entry);
        }
    }

    for section in [
        &mut sections.recommended,
        &mut sections.other,
        &mut sections.all,
    ] {
        sort_section(section, request.history);
    }
    sections
}

/// Deterministic order: the default first, then by how strong the evidence is,
/// then by how recently the application was used, then by name, then by ID so
/// two identically named applications never swap places between runs.
fn sort_section(section: &mut [RankedApplication], history: &UsageHistory) {
    section.sort_by(|left, right| {
        let key = |entry: &RankedApplication| {
            (
                !entry.is_default,
                entry.compatibility.strength(),
                history.position(&entry.desktop_id).unwrap_or(usize::MAX),
                entry.display_name.to_lowercase(),
                entry.desktop_id.as_str().to_string(),
            )
        };
        key(left).cmp(&key(right))
    });
}

fn classify(
    record: &ApplicationRecord,
    request: &ChooserRequest<'_>,
    removed: bool,
    previously_used: bool,
) -> Compatibility {
    if !removed {
        if record
            .mime_types
            .iter()
            .any(|declared| request.resolution.is_primary(declared))
        {
            return Compatibility::Declares;
        }
        let related = record
            .mime_types
            .iter()
            .filter_map(|declared| {
                request
                    .resolution
                    .ancestor_distance(declared)
                    .map(|distance| (distance, declared.clone()))
            })
            .min_by_key(|(distance, _)| *distance);
        if let Some((distance, declared)) = related {
            return Compatibility::DeclaresRelatedType { declared, distance };
        }
        if let Some(pattern) = record
            .mime_types
            .iter()
            .find(|declared| matches_wildcard(declared, &request.resolution.primary))
        {
            return Compatibility::DeclaresWildcard {
                pattern: pattern.clone(),
            };
        }
        if request
            .associations
            .is_added(&request.resolution.primary, &record.desktop_id)
        {
            return Compatibility::UserAssociated;
        }
        if previously_used {
            return Compatibility::PreviouslyUsed;
        }
    }
    Compatibility::NotDeclared
}

/// `text/*` covers `text/plain`. Only the subtype may be a wildcard, which is
/// the only form desktop entries use in practice.
fn matches_wildcard(declared: &MimeType, mime: &MimeType) -> bool {
    let Some((declared_media, declared_subtype)) = declared.as_str().split_once('/') else {
        return false;
    };
    let Some((media, _)) = mime.as_str().split_once('/') else {
        return false;
    };
    declared_subtype == "*" && (declared_media == media || declared_media == "*")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::MimeGraph;
    use crate::mimeapps::MimeAppsFile;
    use app_catalog_core::{DesktopFile, EntryScope, NoProbe};
    use std::path::PathBuf;

    fn mime(value: &str) -> MimeType {
        MimeType::parse(value).expect("valid mime type")
    }

    fn record(desktop_id: &str, name: &str, extra: &str) -> ApplicationRecord {
        let body =
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run %U\n{extra}\n");
        let file = DesktopFile::parse(&body).expect("valid entry");
        ApplicationRecord::from_desktop_file(
            DesktopId::new(desktop_id).expect("valid id"),
            PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
            EntryScope::System,
            &file,
            &NoProbe,
        )
        .expect("valid record")
    }

    fn graph() -> MimeGraph {
        MimeGraph::from_data_dirs(Vec::<PathBuf>::new())
    }

    struct Setup {
        resolution: MimeResolution,
        associations: MimeAssociations,
        history: UsageHistory,
        environments: DesktopEnvironments,
    }

    impl Setup {
        fn new(mime_type: &str, associations_text: &str) -> Self {
            let mut graph = graph();
            let _ = &mut graph;
            Self {
                resolution: MimeResolution {
                    requested: mime(mime_type),
                    primary: mime(mime_type),
                    ancestors: vec![mime("text/plain")],
                },
                associations: MimeAppsFile::parse(associations_text).associations(),
                history: UsageHistory::new(),
                environments: DesktopEnvironments::new(["GNOME"]),
            }
        }

        fn request(&self) -> ChooserRequest<'_> {
            ChooserRequest {
                resolution: &self.resolution,
                associations: &self.associations,
                history: &self.history,
                environments: &self.environments,
                locale: None,
            }
        }
    }

    #[test]
    fn declaring_applications_come_first_then_related_then_everything_else() {
        let setup = Setup::new("text/x-rust", "");
        let records = vec![
            record("zed.desktop", "Zed", "MimeType=text/x-rust;"),
            record("gedit.desktop", "Text Editor", "MimeType=text/plain;"),
            record("gimp.desktop", "GIMP", "MimeType=image/png;"),
            record("any.desktop", "Any", "MimeType=text/*;"),
        ];
        let sections = rank(&records, &setup.request());
        assert_eq!(ids(&sections.recommended), vec!["zed.desktop"]);
        assert_eq!(ids(&sections.other), vec!["gedit.desktop", "any.desktop"]);
        assert_eq!(ids(&sections.all), vec!["gimp.desktop"]);
    }

    #[test]
    fn the_users_default_leads_the_recommended_section_even_when_it_declares_nothing() {
        let setup = Setup::new(
            "text/x-rust",
            "[Default Applications]\ntext/x-rust=nano.desktop\n",
        );
        let records = vec![
            record("zed.desktop", "Zed", "MimeType=text/x-rust;"),
            record("nano.desktop", "Nano", "MimeType=application/pdf;"),
        ];
        let sections = rank(&records, &setup.request());
        assert_eq!(
            ids(&sections.recommended),
            vec!["nano.desktop", "zed.desktop"]
        );
        assert!(sections.recommended[0].is_default);
        assert!(
            sections.recommended[0].compatibility.needs_explanation(),
            "a default that declares nothing still has to be explained"
        );
    }

    #[test]
    fn a_user_added_association_lands_in_the_second_section() {
        let setup = Setup::new(
            "text/x-rust",
            "[Added Associations]\ntext/x-rust=nano.desktop\n",
        );
        let records = vec![record("nano.desktop", "Nano", "MimeType=application/pdf;")];
        let sections = rank(&records, &setup.request());
        assert_eq!(ids(&sections.other), vec!["nano.desktop"]);
        assert_eq!(
            sections.compatibility_of(&DesktopId::new("nano.desktop").unwrap()),
            Some(&Compatibility::UserAssociated)
        );
    }

    #[test]
    fn a_previously_used_application_lands_in_the_second_section() {
        let mut setup = Setup::new("text/x-rust", "");
        setup
            .history
            .record(DesktopId::new("nano.desktop").unwrap());
        let records = vec![record("nano.desktop", "Nano", "MimeType=application/pdf;")];
        let sections = rank(&records, &setup.request());
        assert_eq!(ids(&sections.other), vec!["nano.desktop"]);
        assert!(sections.other[0].previously_used);
    }

    #[test]
    fn a_removed_association_is_demoted_out_of_the_recommended_section() {
        let setup = Setup::new(
            "text/x-rust",
            "[Removed Associations]\ntext/x-rust=zed.desktop\n",
        );
        let records = vec![record("zed.desktop", "Zed", "MimeType=text/x-rust;")];
        let sections = rank(&records, &setup.request());
        assert!(sections.recommended.is_empty());
        assert_eq!(ids(&sections.all), vec!["zed.desktop"]);
    }

    #[test]
    fn hidden_and_desktop_incompatible_entries_never_appear_even_behind_the_expansion() {
        let setup = Setup::new("text/x-rust", "");
        let records = vec![
            record("hidden.desktop", "Hidden", "Hidden=true"),
            record("nodisplay.desktop", "No Display", "NoDisplay=true"),
            record("kde.desktop", "KDE Only", "OnlyShowIn=KDE;"),
            record("visible.desktop", "Visible", ""),
        ];
        let sections = rank(&records, &setup.request());
        assert_eq!(ids(&sections.all), vec!["visible.desktop"]);
    }

    #[test]
    fn ranking_is_deterministic_for_identically_named_applications() {
        let setup = Setup::new("text/x-rust", "");
        let records = vec![
            record("b.desktop", "Editor", "MimeType=text/x-rust;"),
            record("a.desktop", "Editor", "MimeType=text/x-rust;"),
        ];
        let forward = rank(&records, &setup.request());
        let reversed: Vec<ApplicationRecord> = records.into_iter().rev().collect();
        let backward = rank(&reversed, &setup.request());
        assert_eq!(ids(&forward.recommended), vec!["a.desktop", "b.desktop"]);
        assert_eq!(ids(&forward.recommended), ids(&backward.recommended));
    }

    #[test]
    fn a_nearer_ancestor_outranks_a_more_distant_one() {
        let setup = Setup {
            resolution: MimeResolution {
                requested: mime("text/x-rust"),
                primary: mime("text/x-rust"),
                ancestors: vec![mime("text/plain"), mime("application/octet-stream")],
            },
            associations: MimeAppsFile::parse("").associations(),
            history: UsageHistory::new(),
            environments: DesktopEnvironments::new(["GNOME"]),
        };
        let records = vec![
            record("far.desktop", "Far", "MimeType=application/octet-stream;"),
            record("near.desktop", "Near", "MimeType=text/plain;"),
        ];
        let sections = rank(&records, &setup.request());
        let distances: Vec<usize> = sections
            .other
            .iter()
            .map(|entry| match &entry.compatibility {
                Compatibility::DeclaresRelatedType { distance, .. } => *distance,
                other => panic!("unexpected compatibility: {other:?}"),
            })
            .collect();
        assert_eq!(ids(&sections.other), vec!["near.desktop", "far.desktop"]);
        assert_eq!(distances, vec![0, 1]);
    }

    #[test]
    fn an_alias_of_the_selected_type_counts_as_an_explicit_declaration() {
        let setup = Setup {
            resolution: MimeResolution {
                requested: mime("application/x-shellscript"),
                primary: mime("text/x-shellscript"),
                ancestors: Vec::new(),
            },
            associations: MimeAppsFile::parse("").associations(),
            history: UsageHistory::new(),
            environments: DesktopEnvironments::new(["GNOME"]),
        };
        let records = vec![record(
            "old.desktop",
            "Old",
            "MimeType=application/x-shellscript;",
        )];
        let sections = rank(&records, &setup.request());
        assert_eq!(ids(&sections.recommended), vec!["old.desktop"]);
    }

    #[test]
    fn an_empty_catalog_produces_empty_sections() {
        let setup = Setup::new("text/x-rust", "");
        let sections = rank(std::iter::empty(), &setup.request());
        assert!(sections.is_empty());
    }

    #[test]
    fn history_records_move_to_the_front_without_duplicating() {
        let mut history = UsageHistory::new();
        history.record(DesktopId::new("a.desktop").unwrap());
        history.record(DesktopId::new("b.desktop").unwrap());
        history.record(DesktopId::new("a.desktop").unwrap());
        assert_eq!(history.entries().len(), 2);
        assert_eq!(
            history.position(&DesktopId::new("a.desktop").unwrap()),
            Some(0)
        );
    }

    fn ids(section: &[RankedApplication]) -> Vec<&str> {
        section
            .iter()
            .map(|entry| entry.desktop_id.as_str())
            .collect()
    }
}

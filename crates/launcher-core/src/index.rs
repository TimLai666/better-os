//! The searchable index, and the ranking that reads it.
//!
//! The index is built from the shared catalog's records. There is no
//! desktop-entry parser here and there never will be: a second parser is a
//! second set of rules about what counts as an application, and Better OS has
//! exactly one.
//!
//! Building folds every searchable field once. A keystroke folds the query
//! once and then compares fixed character slices, so per-keystroke work is
//! comparison, not parsing, normalization, or allocation per record.
//!
//! Nothing here reaches the network, the filesystem, or the clock. The only
//! outside signal ranking can read is a [`UsageStore`], and only when it has
//! been explicitly switched on.

use std::path::Path;

use app_catalog_core::{ApplicationRecord, Catalog, DesktopEnvironments, ExecutableStatus, Locale};

use crate::matcher::{BestMatch, FieldKind, FieldMatch, fuzzy_match, literal_match};
use crate::model::{
    BrowseModel, LauncherApplication, LauncherView, RankingOptions, SearchModel, SearchResult,
};
use crate::text::FoldedText;
use crate::usage::UsageStore;

/// What the index needs to know about the machine it is being built for.
#[derive(Clone, Debug, Default)]
pub struct IndexOptions {
    /// The active locale. Names are resolved through it, and every other
    /// translation the entry carries stays searchable as an alternate name so
    /// a user typing an application's English name still finds it under a
    /// translated one.
    pub locale: Option<Locale>,
    /// The desktop environments a visibility check runs against. An
    /// application this desktop hides is not indexed at all.
    pub environments: DesktopEnvironments,
}

impl IndexOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_locale(mut self, locale: Option<Locale>) -> Self {
        self.locale = locale;
        self
    }

    pub fn with_environments(mut self, environments: DesktopEnvironments) -> Self {
        self.environments = environments;
        self
    }
}

/// One searchable string belonging to one application.
#[derive(Clone, Debug)]
struct IndexedField {
    kind: FieldKind,
    text: FoldedText,
}

/// One application, prepared for matching.
#[derive(Clone, Debug)]
struct IndexedApplication {
    fields: Vec<IndexedField>,
    /// The union of every field's character mask, used to reject an
    /// application in one instruction before any field is examined.
    mask: u64,
}

/// The launcher's index over the shared catalog.
///
/// The browse model is built once, at construction, because it never depends
/// on the query. Emptying the search row therefore costs nothing at all, which
/// is what "clearing the query returns to the application library" has to mean
/// in a list of several thousand.
#[derive(Clone, Debug, Default)]
pub struct SearchIndex {
    entries: Vec<IndexedApplication>,
    browse: BrowseModel,
}

impl SearchIndex {
    /// Builds an index over a catalog, indexing only what this desktop shows.
    pub fn from_catalog(catalog: &Catalog, options: &IndexOptions) -> Self {
        Self::build(catalog.records(), options)
    }

    /// Builds an index over any set of records. Records the desktop hides are
    /// skipped, so a hidden application is not merely ranked last, it is
    /// absent.
    pub fn build<'record, I>(records: I, options: &IndexOptions) -> Self
    where
        I: IntoIterator<Item = &'record ApplicationRecord>,
    {
        let locale = options.locale.as_ref();
        let mut prepared: Vec<(String, LauncherApplication, IndexedApplication)> = records
            .into_iter()
            .filter(|record| record.visibility_in(&options.environments).is_visible())
            .map(|record| {
                let display_name = record.display_name(locale).to_string();
                let sort_key: String = FoldedText::new(&display_name).chars().iter().collect();
                let application = LauncherApplication {
                    desktop_id: record.desktop_id.clone(),
                    display_name,
                    generic_name: record
                        .generic_name
                        .as_ref()
                        .map(|generic| generic.resolve(locale).to_string()),
                    icon: record.icon.clone(),
                    categories: record.categories.clone(),
                };
                (sort_key, application, index_fields(record, locale))
            })
            .collect();

        // One order, decided here and never again: folded name, then desktop
        // ID. Browse presents it directly, and search inherits it as the
        // tie-break, because a stable sort over this list cannot invent an
        // order of its own.
        prepared.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.desktop_id.cmp(&right.1.desktop_id))
        });

        let mut applications = Vec::with_capacity(prepared.len());
        let mut entries = Vec::with_capacity(prepared.len());
        for (_, application, entry) in prepared {
            applications.push(application);
            entries.push(entry);
        }
        Self {
            entries,
            browse: BrowseModel::new(applications),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The application library. Precomputed, so this is a borrow rather than
    /// work.
    pub fn browse(&self) -> &BrowseModel {
        &self.browse
    }

    /// Ranked results for a query.
    ///
    /// A blank query has no ranked answer — it is the browse state — so it
    /// returns no results here. Callers that want the query-driven switch use
    /// [`SearchIndex::view`] or [`crate::LauncherState::view`].
    pub fn search<'index>(
        &'index self,
        query: &str,
        options: &RankingOptions,
        usage: &dyn UsageStore,
    ) -> SearchModel<'index> {
        let folded = FoldedText::new(query.trim());
        if folded.is_empty() {
            return SearchModel::new(query.to_string(), Vec::new());
        }
        let needle = folded.chars();
        let mask = folded.mask();

        let mut results = Vec::new();
        for (position, entry) in self.entries.iter().enumerate() {
            let Some(matched) = entry.best_match(needle, mask) else {
                continue;
            };
            let application = &self.browse.applications()[position];
            let launch_count = usage.launch_count(&application.desktop_id);
            let base_score = matched.score();
            let score = if options.usage_weighting {
                matched.adjusted_score(usage_bonus(launch_count))
            } else {
                base_score
            };
            results.push(SearchResult {
                application,
                match_kind: matched.kind,
                matched_field: matched.field,
                base_score,
                score,
                launch_count,
            });
        }

        // A stable sort over a list already ordered by name and desktop ID is
        // what makes two runs of the same query identical.
        results.sort_by_key(|result| std::cmp::Reverse(result.score));
        if let Some(limit) = options.limit {
            results.truncate(limit);
        }
        SearchModel::new(query.to_string(), results)
    }

    /// Browse or search, decided by the query alone.
    pub fn view<'index>(
        &'index self,
        query: &str,
        options: &RankingOptions,
        usage: &dyn UsageStore,
    ) -> LauncherView<'index> {
        if query.trim().is_empty() {
            LauncherView::Browse(self.browse())
        } else {
            LauncherView::Search(self.search(query, options, usage))
        }
    }
}

impl IndexedApplication {
    /// The strongest match any of this application's fields offers.
    ///
    /// Literal matching runs over every field first. Only if nothing literal
    /// matched does fuzzy matching run at all, which is sound because any
    /// literal match outranks every possible fuzzy one, and is what keeps the
    /// expensive path off the common case.
    fn best_match(&self, needle: &[char], mask: u64) -> Option<FieldMatch> {
        if self.mask & mask != mask {
            return None;
        }
        let mut best = BestMatch::default();
        for field in &self.fields {
            if !field.text.could_contain(mask) {
                continue;
            }
            if let Some(matched) = literal_match(&field.text, needle, field.kind) {
                best.consider(matched);
            }
        }
        if best.is_none() {
            for field in &self.fields {
                if !field.text.could_contain(mask) {
                    continue;
                }
                if let Some(matched) = fuzzy_match(&field.text, needle, field.kind) {
                    best.consider(matched);
                }
            }
        }
        best.get()
    }
}

/// How much launch history is allowed to move a result. Capped well inside one
/// field bucket, and capped again by
/// [`crate::matcher::FieldMatch::adjusted_score`], so the twentieth launch of
/// an application still cannot promote a keyword match over a name match.
fn usage_bonus(launch_count: u32) -> u32 {
    launch_count.min(40) * 25
}

/// Folds every searchable string an entry carries.
///
/// The inputs are exactly the five Issue #2 lists: display name, generic name,
/// localized names, `Keywords`, and the executable name. Nothing is read from
/// a running process, and no command-line argument is indexed — only the
/// program name itself.
fn index_fields(record: &ApplicationRecord, locale: Option<&Locale>) -> IndexedApplication {
    let mut fields: Vec<IndexedField> = Vec::new();

    push_field(&mut fields, FieldKind::Name, record.display_name(locale));
    push_field(
        &mut fields,
        FieldKind::AlternateName,
        record.name.default_value(),
    );
    for translation in record.name.translations().values() {
        push_field(&mut fields, FieldKind::AlternateName, translation);
    }

    if let Some(generic) = &record.generic_name {
        push_field(&mut fields, FieldKind::GenericName, generic.resolve(locale));
        push_field(&mut fields, FieldKind::GenericName, generic.default_value());
        for translation in generic.translations().values() {
            push_field(&mut fields, FieldKind::GenericName, translation);
        }
    }

    for keyword in record.keywords.resolve(locale) {
        push_field(&mut fields, FieldKind::Keyword, keyword);
    }
    for keyword in record.keywords.default_value() {
        push_field(&mut fields, FieldKind::Keyword, keyword);
    }

    for program in executable_names(record) {
        push_field(&mut fields, FieldKind::Executable, &program);
    }

    let mask = fields
        .iter()
        .fold(0, |mask, field| mask | field.text.mask());
    IndexedApplication { fields, mask }
}

/// Every name this application's executable is known by: the `Exec` program,
/// the `TryExec` program, and the resolved path's file name. An entry with no
/// canonical executable — a Flatpak, a Snap, a D-Bus activated entry —
/// contributes nothing rather than a fabricated name.
fn executable_names(record: &ApplicationRecord) -> Vec<String> {
    let mut names = Vec::new();
    if matches!(record.executable, ExecutableStatus::NotApplicable { .. }) {
        // `Exec` here names the sandbox or the wrapper, not the application.
        // Indexing it would make every Flatpak findable by typing `flatpak`
        // and none of them findable by their own program name, which is worse
        // than indexing nothing.
        return names;
    }
    let mut push = |value: Option<&str>| {
        if let Some(name) = value.and_then(base_name)
            && !names.iter().any(|existing| existing == name)
        {
            names.push(name.to_string());
        }
    };
    push(record.exec.as_ref().map(|exec| exec.program()));
    push(record.visibility.try_exec.as_deref());
    match &record.executable {
        ExecutableStatus::Resolved(path) => push(path.to_str()),
        ExecutableStatus::Unresolved { program } => push(Some(program.as_str())),
        ExecutableStatus::NotApplicable { .. } => {}
    }
    names
}

fn base_name(program: &str) -> Option<&str> {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
}

/// Adds a field unless it is empty or a stronger field already carries the
/// same folded text. Without the deduplication, an untranslated name would be
/// matched twice — once as the name and once as its own alternate — and the
/// second, weaker match would be pure noise.
fn push_field(fields: &mut Vec<IndexedField>, kind: FieldKind, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let text = FoldedText::new(value);
    if text.is_empty() {
        return;
    }
    match fields
        .iter_mut()
        .find(|field| field.text.chars() == text.chars())
    {
        Some(existing) if existing.kind >= kind => {}
        Some(existing) => existing.kind = kind,
        None => fields.push(IndexedField { kind, text }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::MatchKind;
    use crate::model::LauncherState;
    use crate::usage::{InMemoryUsage, LaunchEvent, NoUsage};
    use app_catalog_core::{DesktopFile, DesktopId, EntryScope, NoProbe};
    use std::path::PathBuf;

    fn record(desktop_id: &str, body: &str) -> ApplicationRecord {
        let text = format!("[Desktop Entry]\nType=Application\n{body}\n");
        let file = DesktopFile::parse(&text).expect("valid entry");
        ApplicationRecord::from_desktop_file(
            DesktopId::new(desktop_id).expect("valid desktop id"),
            PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
            EntryScope::System,
            &file,
            &NoProbe,
        )
        .expect("valid record")
    }

    fn index(records: &[ApplicationRecord]) -> SearchIndex {
        SearchIndex::build(records, &IndexOptions::new())
    }

    fn ids(model: &SearchModel<'_>) -> Vec<String> {
        model
            .desktop_ids()
            .into_iter()
            .map(|id| id.as_str().to_string())
            .collect()
    }

    fn search<'a>(index: &'a SearchIndex, query: &str) -> SearchModel<'a> {
        index.search(query, &RankingOptions::default(), &NoUsage)
    }

    #[test]
    fn each_matching_input_is_reachable_on_its_own() {
        let records = vec![
            record("name.desktop", "Name=Photoflare\nExec=pf"),
            record(
                "generic.desktop",
                "Name=Alpha\nGenericName=Spreadsheet\nExec=al",
            ),
            record(
                "localized.desktop",
                "Name=Bravo\nName[zh_TW]=文字編輯器\nExec=br",
            ),
            record(
                "keyword.desktop",
                "Name=Charlie\nKeywords=torrent;\nExec=ch",
            ),
            record("exec.desktop", "Name=Delta\nExec=/usr/bin/inkscape %F"),
        ];
        let index = index(&records);
        for (query, expected, field) in [
            ("photoflare", "name.desktop", FieldKind::Name),
            ("spreadsheet", "generic.desktop", FieldKind::GenericName),
            ("文字編輯器", "localized.desktop", FieldKind::AlternateName),
            ("torrent", "keyword.desktop", FieldKind::Keyword),
            ("inkscape", "exec.desktop", FieldKind::Executable),
        ] {
            let model = search(&index, query);
            assert_eq!(ids(&model), vec![expected.to_string()], "query {query}");
            assert_eq!(model.results()[0].matched_field, field, "query {query}");
        }
    }

    #[test]
    fn exact_beats_prefix_beats_word_boundary_beats_fuzzy() {
        let records = vec![
            record("exact.desktop", "Name=Code\nExec=a"),
            record("prefix.desktop", "Name=Coder\nExec=b"),
            record("boundary.desktop", "Name=Visual Code Studio\nExec=c"),
            record("substring.desktop", "Name=Xcodex\nExec=d"),
            record("fuzzy.desktop", "Name=Cheese Order Editor\nExec=e"),
        ];
        let index = index(&records);
        let model = search(&index, "code");
        assert_eq!(
            ids(&model),
            vec![
                "exact.desktop",
                "prefix.desktop",
                "boundary.desktop",
                "substring.desktop",
                "fuzzy.desktop",
            ]
        );
        assert_eq!(model.results()[0].match_kind, MatchKind::Exact);
        assert_eq!(model.results()[4].match_kind, MatchKind::Fuzzy);
    }

    #[test]
    fn a_name_match_outranks_the_same_match_on_secondary_metadata() {
        let records = vec![
            record("keyword.desktop", "Name=Zulu\nKeywords=paint;\nExec=z"),
            record("generic.desktop", "Name=Yankee\nGenericName=Paint\nExec=y"),
            record("name.desktop", "Name=Paint\nExec=p"),
        ];
        let index = index(&records);
        let model = search(&index, "paint");
        assert_eq!(
            ids(&model),
            vec!["name.desktop", "generic.desktop", "keyword.desktop"]
        );
    }

    #[test]
    fn an_exact_keyword_match_still_outranks_a_fuzzy_name_match() {
        // The two ranking rules can disagree. The match kind decides, so
        // typing an application's own keyword does not bury it under every
        // name that happens to contain those letters in order.
        let records = vec![
            record("keyword.desktop", "Name=Zulu\nKeywords=gimp;\nExec=z"),
            record("name.desktop", "Name=Graphics Image Map Program\nExec=g"),
        ];
        let index = index(&records);
        let model = search(&index, "gimp");
        assert_eq!(ids(&model), vec!["keyword.desktop", "name.desktop"]);
    }

    #[test]
    fn the_same_query_returns_the_same_order_however_the_records_arrived() {
        let mut records = vec![
            record("beta.desktop", "Name=Editor\nExec=b"),
            record("alpha.desktop", "Name=Editor\nExec=a"),
            record("gamma.desktop", "Name=editor\nExec=g"),
        ];
        let forward = index(&records);
        records.reverse();
        let backward = index(&records);
        let expected = vec![
            "alpha.desktop".to_string(),
            "beta.desktop".to_string(),
            "gamma.desktop".to_string(),
        ];
        assert_eq!(ids(&search(&forward, "editor")), expected);
        assert_eq!(ids(&search(&backward, "editor")), expected);
        // And repeating the query changes nothing.
        assert_eq!(ids(&search(&forward, "editor")), expected);
    }

    #[test]
    fn identically_named_applications_from_different_entries_both_appear_in_a_fixed_order() {
        let records = vec![
            record("org.gnome.Files.desktop", "Name=Files\nExec=nautilus"),
            record("nemo.desktop", "Name=Files\nExec=nemo"),
        ];
        let index = index(&records);
        let model = search(&index, "files");
        assert_eq!(ids(&model), vec!["nemo.desktop", "org.gnome.Files.desktop"]);
        assert_eq!(model.results()[0].score, model.results()[1].score);
    }

    #[test]
    fn an_emptied_query_returns_the_library_without_a_mode_change() {
        let records = vec![
            record("a.desktop", "Name=Alpha\nExec=a\nCategories=Utility;"),
            record("b.desktop", "Name=Bravo\nExec=b\nCategories=Graphics;"),
        ];
        let index = index(&records);
        let mut state = LauncherState::new();
        state.set_query("alp");
        assert!(
            !state
                .view(&index, &RankingOptions::default(), &NoUsage)
                .is_browse()
        );
        state.set_query("   ");
        let view = state.view(&index, &RankingOptions::default(), &NoUsage);
        assert!(view.is_browse());
        assert_eq!(view.applications().len(), 2);
        state.clear();
        assert!(
            state
                .view(&index, &RankingOptions::default(), &NoUsage)
                .is_browse()
        );
    }

    #[test]
    fn a_query_that_matches_nothing_produces_an_empty_result_set_not_the_library() {
        let records = vec![record("a.desktop", "Name=Alpha\nExec=a")];
        let index = index(&records);
        let model = search(&index, "zzzzzz");
        assert!(model.is_empty());
        assert_eq!(model.query(), "zzzzzz");
    }

    #[test]
    fn browse_is_ordered_by_name_and_grouped_into_category_sections() {
        let records = vec![
            record("c.desktop", "Name=charlie\nExec=c\nCategories=Utility;"),
            record(
                "a.desktop",
                "Name=Alpha\nExec=a\nCategories=Graphics;Utility;",
            ),
            record("b.desktop", "Name=Bravo\nExec=b"),
        ];
        let index = index(&records);
        let browse = index.browse();
        let names: Vec<&str> = browse
            .applications()
            .iter()
            .map(|application| application.display_name.as_str())
            .collect();
        assert_eq!(names, vec!["Alpha", "Bravo", "charlie"]);
        let sections: Vec<(&str, usize)> = browse
            .sections()
            .iter()
            .map(|section| (section.category, section.members.len()))
            .collect();
        assert_eq!(
            sections,
            vec![("Graphics", 1), ("Utility", 2), ("Other", 1)]
        );
    }

    #[test]
    fn an_application_this_desktop_hides_is_never_indexed() {
        let records = vec![
            record("hidden.desktop", "Name=Hidden\nExec=h\nHidden=true"),
            record(
                "nodisplay.desktop",
                "Name=NoDisplay\nExec=n\nNoDisplay=true",
            ),
            record("kde.desktop", "Name=KDE Thing\nExec=k\nOnlyShowIn=KDE;"),
            record("shown.desktop", "Name=Shown\nExec=s"),
        ];
        let index = SearchIndex::build(
            &records,
            &IndexOptions::new().with_environments(DesktopEnvironments::new(["GNOME"])),
        );
        assert_eq!(index.len(), 1);
        assert!(search(&index, "hidden").is_empty());
        assert_eq!(ids(&search(&index, "shown")), vec!["shown.desktop"]);
    }

    #[test]
    fn a_localized_name_is_searchable_in_the_active_locale_and_in_the_original() {
        let records = vec![record(
            "editor.desktop",
            "Name=Text Editor\nName[zh_TW]=文字編輯器\nKeywords[zh_TW]=文字;\nExec=gedit",
        )];
        let index = SearchIndex::build(
            &records,
            &IndexOptions::new().with_locale(Locale::parse("zh_TW.UTF-8")),
        );
        let localized = search(&index, "編輯");
        assert_eq!(ids(&localized), vec!["editor.desktop"]);
        assert_eq!(localized.results()[0].matched_field, FieldKind::Name);
        assert_eq!(localized.results()[0].match_kind, MatchKind::WordPrefix);

        // The untranslated name still finds it, ranked as an alternate name.
        let original = search(&index, "text editor");
        assert_eq!(ids(&original), vec!["editor.desktop"]);
        assert_eq!(
            original.results()[0].matched_field,
            FieldKind::AlternateName
        );

        // And the display name is the translated one.
        assert_eq!(index.browse().applications()[0].display_name, "文字編輯器");
    }

    #[test]
    fn the_locale_fallback_chain_is_the_catalogs_own() {
        let records = vec![record(
            "editor.desktop",
            "Name=Editor\nName[zh]=編輯\nExec=gedit",
        )];
        let index = SearchIndex::build(
            &records,
            &IndexOptions::new().with_locale(Locale::parse("zh_TW")),
        );
        assert_eq!(index.browse().applications()[0].display_name, "編輯");
    }

    #[test]
    fn case_and_diacritics_do_not_have_to_be_typed() {
        let records = vec![
            record("cafe.desktop", "Name=Café Player\nExec=cafe"),
            record("uber.desktop", "Name=ÜBERSICHT\nExec=uber"),
        ];
        let index = index(&records);
        assert_eq!(ids(&search(&index, "cafe")), vec!["cafe.desktop"]);
        assert_eq!(ids(&search(&index, "CAFÉ")), vec!["cafe.desktop"]);
        assert_eq!(ids(&search(&index, "übersicht")), vec!["uber.desktop"]);
        assert_eq!(ids(&search(&index, "ubersicht")), vec!["uber.desktop"]);
    }

    #[test]
    fn usage_history_changes_nothing_unless_it_is_switched_on() {
        let records = vec![
            record("alpha.desktop", "Name=Alpha Tool\nExec=a"),
            record("bravo.desktop", "Name=Alpha Tool\nExec=b"),
        ];
        let index = index(&records);
        let mut usage = InMemoryUsage::new();
        let bravo = DesktopId::new("bravo.desktop").expect("valid desktop id");
        for at in 0..5 {
            usage.record_launch(&bravo, LaunchEvent { at });
        }

        let default_order = index.search("alpha", &RankingOptions::default(), &usage);
        assert_eq!(
            ids(&default_order),
            vec!["alpha.desktop", "bravo.desktop"],
            "the default ranking must not read history"
        );
        assert_eq!(default_order.results()[1].launch_count, 5);

        let weighted = index.search(
            "alpha",
            &RankingOptions {
                usage_weighting: true,
                ..RankingOptions::default()
            },
            &usage,
        );
        assert_eq!(ids(&weighted), vec!["bravo.desktop", "alpha.desktop"]);
    }

    #[test]
    fn usage_weighting_cannot_promote_a_weaker_match() {
        let records = vec![
            record("name.desktop", "Name=Paint\nExec=p"),
            record("keyword.desktop", "Name=Zulu\nKeywords=paint;\nExec=z"),
        ];
        let index = index(&records);
        let mut usage = InMemoryUsage::new();
        let keyword = DesktopId::new("keyword.desktop").expect("valid desktop id");
        for at in 0..1_000 {
            usage.record_launch(&keyword, LaunchEvent { at });
        }
        let weighted = index.search(
            "paint",
            &RankingOptions {
                usage_weighting: true,
                ..RankingOptions::default()
            },
            &usage,
        );
        assert_eq!(ids(&weighted), vec!["name.desktop", "keyword.desktop"]);
    }

    #[test]
    fn a_limit_keeps_the_highest_ranked_results() {
        let records = vec![
            record("exact.desktop", "Name=Code\nExec=a"),
            record("prefix.desktop", "Name=Coder\nExec=b"),
            record("boundary.desktop", "Name=Visual Code Studio\nExec=c"),
        ];
        let index = index(&records);
        let model = index.search(
            "code",
            &RankingOptions {
                limit: Some(2),
                ..RankingOptions::default()
            },
            &NoUsage,
        );
        assert_eq!(ids(&model), vec!["exact.desktop", "prefix.desktop"]);
    }

    #[test]
    fn a_multi_word_query_matches_across_the_whole_name() {
        let records = vec![
            record("a.desktop", "Name=Disk Usage Analyzer\nExec=baobab"),
            record("b.desktop", "Name=Disks\nExec=gnome-disks"),
        ];
        let index = index(&records);
        let model = search(&index, "disk usage");
        assert_eq!(ids(&model), vec!["a.desktop"]);
    }

    #[test]
    fn an_entry_with_no_canonical_executable_contributes_no_executable_name() {
        let flatpak = record(
            "org.gimp.GIMP.desktop",
            "Name=GIMP\nExec=/usr/bin/flatpak run --branch=stable org.gimp.GIMP",
        );
        let index = index(&[flatpak]);
        assert!(
            search(&index, "flatpak").is_empty(),
            "the sandbox launcher is not the application's executable name"
        );
        assert_eq!(ids(&search(&index, "gimp")), vec!["org.gimp.GIMP.desktop"]);
    }

    #[test]
    fn command_line_arguments_are_not_indexed() {
        let records = vec![record(
            "a.desktop",
            "Name=Alpha\nExec=alpha --secret-token=hunter2 %F",
        )];
        let index = index(&records);
        assert!(search(&index, "hunter2").is_empty());
        assert!(search(&index, "secret-token").is_empty());
    }

    #[test]
    fn an_empty_index_answers_every_query_with_nothing() {
        let index = SearchIndex::build(std::iter::empty(), &IndexOptions::new());
        assert!(index.is_empty());
        assert!(index.browse().is_empty());
        assert!(search(&index, "anything").is_empty());
    }

    #[test]
    fn a_blank_query_asked_of_search_directly_returns_no_results() {
        let records = vec![record("a.desktop", "Name=Alpha\nExec=a")];
        let index = index(&records);
        assert!(search(&index, "").is_empty());
        assert!(search(&index, "  ").is_empty());
    }
}

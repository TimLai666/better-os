//! The reusable chooser surface: a WinUI content dialog.
//!
//! One GPUI view that Better Files embeds and the standalone binary opens in
//! a dialog-sized window. It owns no policy: which apps are offered and why,
//! association writing, and executable refusals all come from
//! `app-chooser-core`; what the list shows, where the keyboard goes, and how
//! tall the dialog is come from this crate's plain model (`lib.rs`);
//! launching goes through the shared catalog's launch path. What lives here
//! is drawing, the two actions, and the honesty rules those actions imply —
//! a failure is shown rather than swallowed, and the reason an app is
//! offered is one click away instead of printed as a chip on every row.
//!
//! Reading the catalog, ranking it, and finding each app's icon happen on a
//! background thread. The render thread never parses a desktop entry.

use std::collections::BTreeMap;
use std::path::PathBuf;

use app_catalog_core::{
    ApplicationRecord, Catalog, DesktopId, IconReference, LaunchTarget, MimeType,
};
use app_catalog_platform::{HostProbe, SessionEnvironment, launch::SystemSpawner, load_catalog};
use app_chooser_core::{
    AppSelection, AssociationOutcome, AssociationRollback, AssociationStore, AssociationWarning,
    ChooserRequest, ChooserSections, ExecutableResolution, ExecutableWarning, MimeGraph,
    UsageHistory, rank,
};
use better_ui::AppIcon;
use better_ui::fluent::button::Button;
use better_ui::fluent::dialog::{ContentDialog, DefaultButton, DialogButtons, DialogResult};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::ListRow;
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toggle::{CheckBox, CheckState};
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, Key, label, palette, spacing};
use better_ui::icons::IconSearch;
use gpui::*;
use gpui_component::IconName;
use gpui_component::input::{InputEvent, InputState, MoveDown, MoveUp};

use crate::i18n::{Locale, always_use, copy, other_apps, reason_text, select_title};
use crate::{
    Line, file_kind, initial_selection, lines, mime_comment, reason, step, visible_selection,
};

/// What the chooser is being asked to do. The executable mode is deliberately a
/// separate mode with its own name, not a variation of Open With.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChooserMode {
    OpenWith,
    ChooseExecutable,
}

/// The file the chooser is choosing an application for.
#[derive(Clone, Debug)]
pub struct ChooserTarget {
    pub display_name: String,
    pub target: LaunchTarget,
    pub mime_type: MimeType,
}

impl ChooserTarget {
    pub fn new(display_name: String, target: LaunchTarget, mime_type: MimeType) -> Self {
        Self {
            display_name,
            target,
            mime_type,
        }
    }

    /// Builds a target from a path, taking the type from the installed
    /// `shared-mime-info` data. Returns `None` when the type cannot be
    /// determined, because a chooser that guessed would rank on a lie.
    pub fn for_path(path: PathBuf) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let mime_type = MimeGraph::from_env().guess_from_file_name(&name)?;
        let target = LaunchTarget::path(path).ok()?;
        Some(Self::new(name, target, mime_type))
    }
}

/// What the chooser tells its host. An embedding surface subscribes to these;
/// the standalone window closes on either one.
#[derive(Clone, Debug)]
pub enum ChooserEvent {
    Selected(AppSelection),
    Cancelled,
}

impl EventEmitter<ChooserEvent> for AppChooser {}

/// What the background load produced.
struct CatalogSnapshot {
    catalog: Catalog,
    sections: ChooserSections,
    icons: BTreeMap<DesktopId, AppIcon>,
    /// shared-mime-info's description of the file's type, in the person's
    /// language, when it has one.
    description: Option<String>,
}

/// Whether the catalog has been read yet.
enum LoadState {
    Loading,
    Ready(Box<CatalogSnapshot>),
}

/// A message shown in the dialog, kept as typed variants so the wording
/// stays in the locale files.
#[derive(Clone, Debug)]
enum Notice {
    LaunchFailed(String),
    AssociationWritten(Box<AssociationRollback>),
    AssociationUnchanged,
    AssociationFailed(String),
    AssociationRolledBack,
}

pub struct AppChooser {
    locale: Locale,
    mode: ChooserMode,
    target: ChooserTarget,
    state: LoadState,
    selected: Option<DesktopId>,
    /// The app whose "Why?" is open.
    explained: Option<DesktopId>,
    expanded: bool,
    always: bool,
    search: Entity<InputState>,
    query: String,
    list_scroll: ScrollHandle,
    notice: Option<Notice>,
    warnings: Vec<AssociationWarning>,
    executable: Option<ExecutableResolution>,
    browse_roots: Vec<PathBuf>,
    browsing: Option<PathBuf>,
    chosen_executable: Option<PathBuf>,
    _load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl AppChooser {
    pub fn new(
        target: ChooserTarget,
        mode: ChooserMode,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search_placeholder));
        let search_for_callback = search.clone();
        let subscription = cx.subscribe_in(
            &search,
            window,
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = search_for_callback.read(cx).value().to_string();
                    this.set_query(value, cx);
                }
            },
        );
        search.focus_handle(cx).focus(window, cx);

        let mut chooser = Self {
            locale,
            mode,
            target,
            state: LoadState::Loading,
            selected: None,
            explained: None,
            expanded: false,
            always: false,
            search,
            query: String::new(),
            list_scroll: ScrollHandle::new(),
            notice: None,
            warnings: Vec::new(),
            executable: None,
            browse_roots: Vec::new(),
            browsing: None,
            chosen_executable: None,
            _load: None,
            _subscriptions: vec![subscription],
        };
        chooser.start_load(cx);
        chooser
    }

    /// Reads the catalog, ranks it, and finds the icons on a background
    /// thread, then applies the result on the main thread.
    fn start_load(&mut self, cx: &mut Context<Self>) {
        let mime = self.target.mime_type.clone();
        let entry_locale = self.locale.entry_locale();
        let language = crate::i18n::mime_language(self.locale);
        let work = cx.background_spawn(async move { load_snapshot(mime, entry_locale, language) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let snapshot = work.await;
            this.update(cx, |this, cx| {
                let first = initial_selection(&snapshot.sections);
                this.state = LoadState::Ready(Box::new(snapshot));
                this.browse_roots = app_chooser_core::browse_roots();
                if let Some(first) = first {
                    this.select(first, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn snapshot(&self) -> Option<&CatalogSnapshot> {
        match &self.state {
            LoadState::Ready(snapshot) => Some(snapshot),
            LoadState::Loading => None,
        }
    }

    /// How many lines the list shows now, which is what the standalone
    /// window sizes itself to.
    pub fn line_count(&self) -> usize {
        self.snapshot()
            .map(|snapshot| lines(&snapshot.sections, &self.query, self.expanded).len())
            .unwrap_or(2)
    }

    pub fn mode(&self) -> ChooserMode {
        self.mode
    }

    /// Whether an info bar is showing, which the dialog's height allows for.
    pub fn has_notice(&self) -> bool {
        self.notice.is_some() || !self.warnings.is_empty()
    }

    fn record(&self, desktop_id: &DesktopId) -> Option<&ApplicationRecord> {
        self.snapshot()
            .and_then(|snapshot| snapshot.catalog.get(desktop_id))
    }

    fn selected_record(&self) -> Option<&ApplicationRecord> {
        self.selected
            .as_ref()
            .and_then(|desktop_id| self.record(desktop_id))
    }

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if query == self.query {
            return;
        }
        self.query = query;
        // The selection follows the list: the first match while searching,
        // the first app again when the search is cleared, and nothing when
        // nothing matches.
        let next = self.snapshot().and_then(|snapshot| {
            let lines = lines(&snapshot.sections, &self.query, self.expanded);
            visible_selection(&lines, self.selected.as_ref())
        });
        match next {
            Some(next) => self.select(next, cx),
            None => {
                self.selected = None;
                self.explained = None;
                self.executable = None;
            }
        }
        cx.notify();
    }

    /// Whether the selected app is one the list shows: what Open needs.
    fn selection_visible(&self) -> bool {
        self.selected_line().is_some()
    }

    fn select(&mut self, desktop_id: DesktopId, cx: &mut Context<Self>) {
        self.executable = self
            .record(&desktop_id)
            .map(app_chooser_core::resolve_executable);
        if self.selected.as_ref() != Some(&desktop_id) {
            self.explained = None;
        }
        self.selected = Some(desktop_id);
        self.chosen_executable = None;
        if matches!(self.notice, Some(Notice::LaunchFailed(_))) {
            self.notice = None;
        }
        cx.notify();
    }

    fn toggle_other_apps(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    /// Up and Down from the search box move through the list; Enter on
    /// "Other apps" opens it, Enter on an app opens the file with it.
    fn key(&mut self, key: Key, cx: &mut Context<Self>) -> bool {
        let Some(snapshot) = self.snapshot() else {
            return false;
        };
        let lines = lines(&snapshot.sections, &self.query, self.expanded);
        let current = self.selected.as_ref().and_then(|selected| {
            lines
                .iter()
                .position(|line| line.app().is_some_and(|app| &app.desktop_id == selected))
        });
        let Some(next) = step(&lines, current, key) else {
            return matches!(key, Key::Up | Key::Down);
        };
        match lines[next] {
            Line::App(app) => {
                let id = app.desktop_id.clone();
                self.select(id, cx);
            }
            Line::OtherApps { .. } => {
                // The fold row takes the keyboard on its way: moving onto it
                // opens it, so Down keeps going into the other apps.
                if !self.expanded && key == Key::Down {
                    self.expanded = true;
                }
                let lines = crate::lines(
                    &self.snapshot().expect("loaded").sections,
                    &self.query,
                    self.expanded,
                );
                let beyond = match key {
                    Key::Up => next.checked_sub(1),
                    _ => Some(next + 1),
                };
                if let Some(app) = beyond.and_then(|i| lines.get(i)).and_then(Line::app) {
                    let id = app.desktop_id.clone();
                    self.select(id, cx);
                }
            }
        }
        if let Some(position) = self.selected_line() {
            self.list_scroll.scroll_to_item(position);
        }
        cx.notify();
        true
    }

    fn selected_line(&self) -> Option<usize> {
        let snapshot = self.snapshot()?;
        let selected = self.selected.as_ref()?;
        lines(&snapshot.sections, &self.query, self.expanded)
            .iter()
            .position(|line| line.app().is_some_and(|app| &app.desktop_id == selected))
    }

    /// Open: once, or as the default for this kind of file when the check
    /// box is on.
    fn open(&mut self, cx: &mut Context<Self>) {
        if !self.selection_visible() {
            return;
        }
        if self.always {
            self.always_use(cx);
        } else {
            self.open_once(cx);
        }
    }

    /// Launches through the shared catalog path and writes nothing.
    fn open_once(&mut self, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record().cloned() else {
            return;
        };
        let selection = AppSelection::open_once(record.desktop_id.clone(), None);
        match self.launch(&record) {
            Ok(()) => cx.emit(ChooserEvent::Selected(selection)),
            Err(message) => self.notice = Some(Notice::LaunchFailed(message)),
        }
        cx.notify();
    }

    /// Writes one association with a rollback record, then opens the file.
    /// A failed write stops before the launch so the person is never told a
    /// default was saved that was not.
    fn always_use(&mut self, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record().cloned() else {
            return;
        };
        let store = match AssociationStore::for_user() {
            Ok(store) => store,
            Err(error) => {
                self.notice = Some(Notice::AssociationFailed(error.to_string()));
                cx.notify();
                return;
            }
        };
        match store.set_default(&self.target.mime_type, &record) {
            Ok(AssociationOutcome {
                rollback,
                changed,
                warnings,
                ..
            }) => {
                self.warnings = warnings;
                self.notice = Some(if changed {
                    Notice::AssociationWritten(Box::new(rollback))
                } else {
                    Notice::AssociationUnchanged
                });
                let selection = AppSelection::always_use(record.desktop_id.clone(), None);
                match self.launch(&record) {
                    Ok(()) => cx.emit(ChooserEvent::Selected(selection)),
                    Err(message) => self.notice = Some(Notice::LaunchFailed(message)),
                }
            }
            Err(error) => self.notice = Some(Notice::AssociationFailed(error.to_string())),
        }
        cx.notify();
    }

    /// Undoes the association just written, from the record it wrote.
    fn undo_association(&mut self, rollback: AssociationRollback, cx: &mut Context<Self>) {
        let outcome =
            AssociationStore::for_user().and_then(|store| store.restore(&rollback).map(|()| store));
        self.notice = Some(match outcome {
            Ok(_) => Notice::AssociationRolledBack,
            Err(error) => Notice::AssociationFailed(error.to_string()),
        });
        self.warnings.clear();
        cx.notify();
    }

    fn launch(&self, record: &ApplicationRecord) -> Result<(), String> {
        let spawner = SystemSpawner;
        let launcher = app_catalog_platform::launch::Launcher::new(&spawner);
        let locale = self.locale.entry_locale();
        launcher
            .launch(
                record,
                None,
                std::slice::from_ref(&self.target.target),
                locale.as_ref(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn choose_executable_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match app_chooser_core::accept_executable_path(&path) {
            Ok(path) => {
                let desktop_id = self
                    .selected
                    .clone()
                    .or_else(|| DesktopId::new("better-os-browsed.desktop").ok());
                self.chosen_executable = Some(path.clone());
                if let Some(desktop_id) = desktop_id {
                    cx.emit(ChooserEvent::Selected(AppSelection::executable(
                        desktop_id, path,
                    )));
                }
            }
            Err(warning) => self.executable = Some(ExecutableResolution::Refused(warning)),
        }
        cx.notify();
    }

    fn icon(&self, desktop_id: &DesktopId, name: &str) -> AppIcon {
        self.snapshot()
            .and_then(|snapshot| snapshot.icons.get(desktop_id).cloned())
            .unwrap_or_else(|| AppIcon::Letter(better_ui::icons::initial(name)))
    }
}

/// The blocking half: read every application directory, resolve the file's
/// type against the installed `shared-mime-info` data, read the user's own
/// associations, rank, and find each offered app's icon. All of it off the
/// render thread.
fn load_snapshot(
    mime: MimeType,
    locale: Option<app_catalog_core::Locale>,
    language: Option<&str>,
) -> CatalogSnapshot {
    let session = SessionEnvironment::from_env();
    let probe = HostProbe::from_env();
    let catalog = load_catalog(&session, &probe);
    let resolution = MimeGraph::from_env().resolve(&mime);
    let associations = AssociationStore::for_user()
        .and_then(|store| store.load())
        .map(|file| file.associations())
        .unwrap_or_default();
    let history = UsageHistory::from_associations(&associations, &resolution.primary);
    let environments = session.environments.clone();
    let sections = rank(
        catalog.records(),
        &ChooserRequest {
            resolution: &resolution,
            associations: &associations,
            history: &history,
            environments: &environments,
            locale: locale.as_ref(),
        },
    );
    let search = IconSearch::from_environment();
    let icons = sections
        .iter()
        .map(|app| {
            let icon = catalog
                .get(&app.desktop_id)
                .and_then(|record| match &record.icon {
                    Some(IconReference::Name(name)) => Some(name.clone()),
                    Some(IconReference::Path(path)) => Some(path.display().to_string()),
                    None => None,
                });
            (
                app.desktop_id.clone(),
                AppIcon::for_entry(icon.as_deref(), &app.display_name, &search),
            )
        })
        .collect();
    let description = mime_description(&resolution.primary, language);
    CatalogSnapshot {
        catalog,
        sections,
        icons,
        description,
    }
}

/// The description shared-mime-info's `mime/<type>.xml` gives, from the
/// first data directory that has the file.
fn mime_description(mime: &MimeType, language: Option<&str>) -> Option<String> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let (media, subtype) = mime.as_str().split_once('/')?;
    if subtype.contains('/') || subtype.starts_with('.') || media.starts_with('.') {
        return None;
    }
    home.into_iter()
        .chain(
            dirs.split(':')
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
        )
        .find_map(|dir| {
            std::fs::read_to_string(dir.join("mime").join(media).join(format!("{subtype}.xml")))
                .ok()
        })
        .and_then(|xml| mime_comment(&xml, language))
}

/// What the executable mode says about one application.
pub(crate) fn executable_message(locale: Locale, resolution: &ExecutableResolution) -> String {
    let c = copy(locale);
    match resolution {
        ExecutableResolution::Resolved(path) => {
            format!("{} {}", c.executable_resolved, path.display())
        }
        ExecutableResolution::Refused(warning) => match warning {
            ExecutableWarning::NoSingleExecutable { .. } => c.executable_no_single.to_string(),
            ExecutableWarning::DBusActivated => c.executable_dbus.to_string(),
            ExecutableWarning::ProgramNotFound { program } => {
                format!("{} ({program})", c.executable_not_found)
            }
            ExecutableWarning::NoExecLine => c.executable_no_exec.to_string(),
            ExecutableWarning::ComplexArguments { .. } => c.executable_complex.to_string(),
            ExecutableWarning::NotFound { path } => {
                format!("{} {}", c.executable_not_found, path.display())
            }
            ExecutableWarning::NotExecutable { path } => {
                format!("{} {}", c.executable_no_single, path.display())
            }
        },
    }
}

/// What an association warning says to the person.
pub(crate) fn warning_message(locale: Locale, warning: &AssociationWarning) -> &'static str {
    let c = copy(locale);
    match warning {
        AssociationWarning::ApplicationDoesNotDeclareType => c.warning_does_not_declare,
        AssociationWarning::ListedInRemovedAssociations => c.warning_removed_association,
        AssociationWarning::DuplicateDefaultKey => c.warning_duplicate_key,
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// The list's width inside the dialog: the dialog's width less its 24px
/// padding on each side.
const CONTENT_WIDTH: Pixels = px(crate::DIALOG_WIDTH - 48.0);

impl AppChooser {
    fn app_row(&self, index: usize, line: Line<'_>, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        match line {
            Line::OtherApps { count, expanded } => ListRow::new(
                ElementId::named_usize("other-apps", index),
                other_apps(self.locale, count),
            )
            .glyph(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_other_apps(cx)))
            .into_any_element(),
            Line::App(app) => {
                let selected = self.selected.as_ref() == Some(&app.desktop_id);
                let explained = self.explained.as_ref() == Some(&app.desktop_id);
                let id = app.desktop_id.clone();
                let toggle_id = app.desktop_id.clone();
                let mut row = ListRow::new(
                    ElementId::named_usize("app", index),
                    app.display_name.clone(),
                )
                .app_icon(self.icon(&app.desktop_id, &app.display_name))
                .selected(selected)
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.select(id.clone(), cx);
                    if event.click_count() >= 2 {
                        this.open(cx);
                    }
                }));
                if app.is_default {
                    row = row.caption(c.current_default);
                }
                if selected {
                    row = row.trailing(
                        Button::icon_button(
                            ElementId::named_usize("why", index),
                            IconName::Info,
                            c.why,
                        )
                        .checked(explained)
                        .on_click({
                            let this = cx.entity().downgrade();
                            move |_, cx| {
                                this.update(cx, |this, cx| {
                                    this.explained = if this.explained.as_ref() == Some(&toggle_id)
                                    {
                                        None
                                    } else {
                                        Some(toggle_id.clone())
                                    };
                                    cx.notify();
                                })
                                .ok();
                            }
                        }),
                    );
                }
                if explained {
                    row = row.detail(
                        label(
                            reason_text(self.locale, reason(app)),
                            CAPTION,
                            p.text_secondary,
                        )
                        .pt(spacing::XS)
                        .whitespace_normal(),
                    );
                }
                row.into_any_element()
            }
        }
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(snapshot) = self.snapshot() else {
            return div()
                .flex()
                .items_center()
                .gap(spacing::MD)
                .py(spacing::LG)
                .child(ProgressRing::new("loading", Progress::Indeterminate).size(px(20.0)))
                .child(label(c.loading_title, BODY, p.text_secondary))
                .into_any_element();
        };
        if snapshot.sections.is_empty() {
            return div()
                .py(spacing::LG)
                .child(label(c.empty_title, BODY_STRONG, p.text_primary))
                .child(label(c.empty_detail, CAPTION, p.text_secondary))
                .into_any_element();
        }
        let visible = lines(&snapshot.sections, &self.query, self.expanded);
        if visible.is_empty() {
            return div()
                .py(spacing::LG)
                .child(label(c.no_matches_title, BODY_STRONG, p.text_primary))
                .child(label(c.no_matches_detail, CAPTION, p.text_secondary))
                .into_any_element();
        }
        let rows: Vec<AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(index, line)| self.app_row(index, *line, cx))
            .collect();
        div()
            .id("apps")
            .w(CONTENT_WIDTH)
            .flex()
            .flex_col()
            .max_h(px(match self.mode {
                // Room for the program browser under a short list.
                ChooserMode::ChooseExecutable => crate::metrics::ROW * 3.0,
                ChooserMode::OpenWith => crate::list_height_limit(true, self.has_notice()),
            }))
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .p(px(4.0))
            .mx(px(-4.0))
            .children(rows)
            .into_any_element()
    }

    fn notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = copy(self.locale);
        let words = self.locale.words();
        let mut elements: Vec<AnyElement> = Vec::new();
        if let Some(notice) = &self.notice {
            let element = match notice {
                Notice::LaunchFailed(detail) => ErrorBar::new(
                    "launch-failed",
                    ErrorReport::new(c.launch_failed, detail.clone())
                        .advice(c.launch_failed_advice),
                )
                .locale(words)
                .into_any_element(),
                Notice::AssociationFailed(detail) => ErrorBar::new(
                    "association-failed",
                    ErrorReport::new(c.association_failed, detail.clone()),
                )
                .locale(words)
                .into_any_element(),
                Notice::AssociationWritten(rollback) => {
                    let rollback = rollback.as_ref().clone();
                    InfoBar::new("association", Severity::Success, c.association_written)
                        .locale(words)
                        .action(Button::standard("undo", c.undo).on_click({
                            let this = cx.entity().downgrade();
                            move |_, cx| {
                                this.update(cx, |this, cx| {
                                    this.undo_association(rollback.clone(), cx)
                                })
                                .ok();
                            }
                        }))
                        .into_any_element()
                }
                Notice::AssociationUnchanged => InfoBar::new(
                    "unchanged",
                    Severity::Informational,
                    c.association_unchanged,
                )
                .locale(words)
                .into_any_element(),
                Notice::AssociationRolledBack => {
                    InfoBar::new("rolled-back", Severity::Success, c.association_rolled_back)
                        .locale(words)
                        .into_any_element()
                }
            };
            elements.push(element);
        }
        for (index, warning) in self.warnings.iter().enumerate() {
            elements.push(
                InfoBar::new(
                    ElementId::named_usize("warning", index),
                    Severity::Warning,
                    warning_message(self.locale, warning),
                )
                .locale(words)
                .into_any_element(),
            );
        }
        elements
    }

    fn executable_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let words = self.locale.words();
        let mut panel = div().w(CONTENT_WIDTH).flex().flex_col().gap(spacing::SM);
        if let Some(resolution) = &self.executable {
            let refused = matches!(resolution, ExecutableResolution::Refused(_));
            panel = panel.child(
                InfoBar::new(
                    "executable",
                    if refused {
                        Severity::Warning
                    } else {
                        Severity::Informational
                    },
                    executable_message(self.locale, resolution),
                )
                .locale(words),
            );
            if let ExecutableResolution::Resolved(path) = resolution {
                let path = path.clone();
                panel = panel.child(div().flex().child(
                    Button::accent("use-executable", c.executable_use_path).on_click({
                        let this = cx.entity().downgrade();
                        move |_, cx| {
                            this.update(cx, |this, cx| {
                                this.choose_executable_path(path.clone(), cx)
                            })
                            .ok();
                        }
                    }),
                ));
            }
        }
        if let Some(path) = &self.chosen_executable {
            panel = panel.child(label(
                format!("{}: {}", c.executable_selected, path.display()),
                CAPTION,
                p.text_secondary,
            ));
        }
        panel = panel.child(label(c.executable_browse, BODY_STRONG, p.text_primary));
        if self.browse_roots.is_empty() {
            return panel
                .child(label(c.executable_browse_empty, CAPTION, p.text_secondary))
                .into_any_element();
        }
        panel = panel.child(div().flex().flex_wrap().gap(spacing::SM).children(
            self.browse_roots.iter().enumerate().map(|(index, root)| {
                let root = root.clone();
                let this = cx.entity().downgrade();
                Button::standard(
                    ElementId::named_usize("root", index),
                    root.display().to_string(),
                )
                .on_click(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.browsing = Some(root.clone());
                        cx.notify();
                    })
                    .ok();
                })
            }),
        ));
        if let Some(directory) = &self.browsing {
            let programs = app_chooser_core::list_executables(directory);
            panel =
                panel.child(
                    div()
                        .id("programs")
                        .max_h(px(108.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(programs.into_iter().take(200).enumerate().map(
                            |(index, path)| {
                                let name = path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                ListRow::new(ElementId::named_usize("program", index), name)
                                    .glyph(IconName::SquareTerminal)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.choose_executable_path(path.clone(), cx)
                                    }))
                            },
                        )),
                );
        }
        panel.into_any_element()
    }
}

impl Render for AppChooser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let c = copy(self.locale);
        // The search box's clear button empties it without a change event.
        let typed = self.search.read(cx).value().to_string();
        self.set_query(typed, cx);

        let title = match self.mode {
            ChooserMode::OpenWith => select_title(self.locale, &self.target.display_name),
            ChooserMode::ChooseExecutable => c.executable_title.to_string(),
        };
        let list = self.list(cx);
        let notices = self.notices(cx);
        let buttons = match self.mode {
            ChooserMode::OpenWith => DialogButtons {
                primary: Some(c.open.into()),
                secondary: None,
                close: Some(c.cancel.into()),
                default: DefaultButton::Primary,
            },
            ChooserMode::ChooseExecutable => DialogButtons {
                primary: None,
                secondary: None,
                close: Some(c.cancel.into()),
                default: DefaultButton::None,
            },
        };
        let this = cx.entity().downgrade();
        let mut dialog = ContentDialog::new("open-with", title)
            .buttons(buttons)
            .primary_enabled(self.selection_visible())
            .inline()
            .on_result(move |result, _, cx| {
                this.update(cx, |this, cx| match result {
                    DialogResult::Primary => this.open(cx),
                    DialogResult::Secondary => {}
                    DialogResult::Close => cx.emit(ChooserEvent::Cancelled),
                })
                .ok();
            })
            .child(
                div()
                    .w(CONTENT_WIDTH)
                    .child(TextBox::search("search", &self.search)),
            )
            .children(notices)
            .child(list);
        if self.mode == ChooserMode::ChooseExecutable {
            dialog = dialog
                .child(label(c.executable_subtitle, CAPTION, p.text_secondary))
                .child(self.executable_panel(cx));
        } else {
            let always = self.always;
            dialog = dialog.child(
                CheckBox::new("always", CheckState::from_bool(always))
                    .label(always_use(
                        self.locale,
                        &file_kind(
                            self.snapshot().and_then(|s| s.description.clone()),
                            &self.target.display_name,
                        ),
                    ))
                    .on_change({
                        let this = cx.entity().downgrade();
                        move |state, _, cx| {
                            this.update(cx, |this, cx| {
                                this.always = state == CheckState::Checked;
                                cx.notify();
                            })
                            .ok();
                        }
                    }),
            );
        }

        div()
            .id("app-chooser")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(p.window_background)
            .text_color(p.text_primary)
            // Up and Down are the search box's own cursor actions; they are
            // taken on their way down and walk the list instead.
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if this.key(Key::Up, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if this.key(Key::Down, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(dialog)
    }
}

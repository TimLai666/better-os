//! The reusable chooser surface.
//!
//! One GPUI view that Better Files, and any later Better OS surface, either
//! embeds or opens as a window. It owns no policy: sectioning, compatibility,
//! association writing, and executable refusals all come from
//! `app-chooser-core`, and launching goes through the shared catalog's launch
//! path. What lives here is presentation, the two actions, and the honesty
//! rules those actions imply — a failure is shown rather than swallowed, and an
//! application that does not declare the file's type is explained rather than
//! silently offered.
//!
//! Reading the catalog and ranking it happen on a background thread. The render
//! thread never parses a desktop entry.

use std::path::PathBuf;

use app_catalog_core::{
    ApplicationRecord, Catalog, DesktopId, EntryScope, LaunchTarget, MimeType, SourceKind,
};
use app_catalog_platform::{HostProbe, SessionEnvironment, launch::SystemSpawner, load_catalog};
use app_chooser_core::{
    AppSelection, AssociationOutcome, AssociationRollback, AssociationStore, AssociationWarning,
    ChooserRequest, ChooserSections, Compatibility, ExecutableResolution, ExecutableWarning,
    MimeGraph, RankedApplication, UsageHistory, rank,
};
use better_ui::{BadgeStyle, TileStyle};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, Icon, IconName, button::Button, button::ButtonVariants, *};

use crate::i18n::{Locale, copy};

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
}

/// Whether the catalog has been read yet.
enum LoadState {
    Loading,
    Ready(Box<CatalogSnapshot>),
}

/// A message shown under the header. Kept as typed variants so the wording
/// stays in the locale files.
#[derive(Clone, Debug)]
enum Notice {
    Launched,
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
    expanded: bool,
    search: Entity<InputState>,
    query: String,
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
                    this.query = search_for_callback.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

        let mut chooser = Self {
            locale,
            mode,
            target,
            state: LoadState::Loading,
            selected: None,
            expanded: false,
            search,
            query: String::new(),
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

    /// Reads the catalog and ranks it on a background thread, then applies the
    /// result on the main thread. Nothing here runs during a frame.
    fn start_load(&mut self, cx: &mut Context<Self>) {
        let mime = self.target.mime_type.clone();
        let entry_locale = self.locale.entry_locale();
        let work = cx.background_spawn(async move { load_snapshot(mime, entry_locale) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let snapshot = work.await;
            this.update(cx, |this, cx| {
                this.state = LoadState::Ready(Box::new(snapshot));
                this.browse_roots = app_chooser_core::browse_roots();
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

    fn record(&self, desktop_id: &DesktopId) -> Option<&ApplicationRecord> {
        self.snapshot()
            .and_then(|snapshot| snapshot.catalog.get(desktop_id))
    }

    fn selected_record(&self) -> Option<&ApplicationRecord> {
        self.selected
            .as_ref()
            .and_then(|desktop_id| self.record(desktop_id))
    }

    fn select(&mut self, desktop_id: DesktopId, cx: &mut Context<Self>) {
        self.executable = self
            .record(&desktop_id)
            .map(app_chooser_core::resolve_executable);
        self.selected = Some(desktop_id);
        self.chosen_executable = None;
        self.notice = None;
        self.warnings.clear();
        cx.notify();
    }

    /// Open Once. Launches through the shared catalog path and writes nothing.
    fn open_once(&mut self, cx: &mut Context<Self>) {
        let Some(record) = self.selected_record().cloned() else {
            return;
        };
        let selection = AppSelection::open_once(record.desktop_id.clone(), None);
        match self.launch(&record) {
            Ok(()) => {
                self.notice = Some(Notice::Launched);
                cx.emit(ChooserEvent::Selected(selection));
            }
            Err(message) => self.notice = Some(Notice::LaunchFailed(message)),
        }
        cx.notify();
    }

    /// Always Use. Writes one association with a rollback record, then opens
    /// the file. A failed write stops before the launch so the user is never
    /// told a default was saved that was not.
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
                if let Err(message) = self.launch(&record) {
                    self.notice = Some(Notice::LaunchFailed(message));
                } else {
                    cx.emit(ChooserEvent::Selected(selection));
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

    fn matches_query(&self, entry: &RankedApplication) -> bool {
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || entry.display_name.to_lowercase().contains(&query)
            || entry.desktop_id.as_str().to_lowercase().contains(&query)
    }
}

/// The blocking half: read every application directory, resolve the file's
/// type against the installed `shared-mime-info` data, read the user's own
/// associations, and rank. All of it off the render thread.
fn load_snapshot(mime: MimeType, locale: Option<app_catalog_core::Locale>) -> CatalogSnapshot {
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
    CatalogSnapshot { catalog, sections }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl AppChooser {
    fn tile_style(&self, selected: bool, cx: &App) -> TileStyle {
        TileStyle {
            foreground: cx.theme().foreground,
            muted_foreground: cx.theme().muted_foreground,
            border: if selected {
                cx.theme().primary
            } else {
                cx.theme().border
            },
            background: if selected {
                cx.theme().accent
            } else {
                cx.theme().background
            },
            glyph_foreground: cx.theme().primary_foreground,
            glyph_background: cx.theme().primary,
            radius: cx.theme().radius,
        }
    }

    fn badge_style(&self, strong: bool, cx: &App) -> BadgeStyle {
        BadgeStyle {
            foreground: if strong {
                cx.theme().primary_foreground
            } else {
                cx.theme().muted_foreground
            },
            background: if strong {
                cx.theme().primary
            } else {
                cx.theme().muted
            },
            border: cx.theme().border,
        }
    }

    fn compatibility_label_for(&self, compatibility: &Compatibility) -> &'static str {
        compatibility_label(self.locale, compatibility)
    }
}

/// The badge wording for one compatibility reason.
pub(crate) fn compatibility_label(locale: Locale, compatibility: &Compatibility) -> &'static str {
    {
        let c = copy(locale);
        match compatibility {
            Compatibility::Declares => c.badge_declares,
            Compatibility::DeclaresRelatedType { .. } => c.badge_related,
            Compatibility::DeclaresWildcard { .. } => c.badge_wildcard,
            Compatibility::PreviouslyUsed => c.badge_previously_used,
            Compatibility::UserAssociated => c.badge_user_associated,
            Compatibility::NotDeclared => c.badge_not_declared,
        }
    }
}

/// The sentence shown when a chosen application does not simply declare the
/// selected type. `None` means there is nothing to explain.
pub(crate) fn compatibility_explanation(
    locale: Locale,
    compatibility: &Compatibility,
) -> Option<&'static str> {
    {
        let c = copy(locale);
        match compatibility {
            Compatibility::Declares => None,
            Compatibility::DeclaresRelatedType { .. } => Some(c.explain_related),
            Compatibility::DeclaresWildcard { .. } => Some(c.explain_wildcard),
            Compatibility::PreviouslyUsed => Some(c.explain_previously_used),
            Compatibility::UserAssociated => Some(c.explain_user_associated),
            Compatibility::NotDeclared => Some(c.explain_not_declared),
        }
    }
}

/// The source badge wording: how the application is packaged, and who it is
/// installed for.
pub(crate) fn source_label(locale: Locale, kind: SourceKind, scope: EntryScope) -> String {
    {
        let c = copy(locale);
        let kind = match kind {
            SourceKind::Native => c.source_native,
            SourceKind::Flatpak => c.source_flatpak,
            SourceKind::Snap => c.source_snap,
            SourceKind::AppImage => c.source_appimage,
            SourceKind::Wrapper => c.source_wrapper,
        };
        let scope = match scope {
            EntryScope::User => c.scope_user,
            EntryScope::System => c.scope_system,
        };
        format!("{kind} · {scope}")
    }
}

/// What the executable mode says about one application.
pub(crate) fn executable_message(locale: Locale, resolution: &ExecutableResolution) -> String {
    {
        let c = copy(locale);
        match resolution {
            ExecutableResolution::Resolved(path) => {
                format!("{} — {}", c.executable_resolved, path.display())
            }
            ExecutableResolution::Refused(warning) => match warning {
                ExecutableWarning::NoSingleExecutable { .. } => c.executable_no_single.to_string(),
                ExecutableWarning::DBusActivated => c.executable_dbus.to_string(),
                ExecutableWarning::ProgramNotFound { program } => {
                    format!("{} ({program})", c.executable_not_found)
                }
                ExecutableWarning::NoExecLine => c.executable_no_exec.to_string(),
                ExecutableWarning::ComplexArguments { dropped, .. } => {
                    format!("{} ({})", c.executable_complex, dropped.join(" "))
                }
                ExecutableWarning::NotFound { path } => {
                    format!("{} — {}", c.executable_not_found, path.display())
                }
                ExecutableWarning::NotExecutable { path } => {
                    format!("{} — {}", c.executable_no_single, path.display())
                }
            },
        }
    }
}

impl AppChooser {
    fn notice_message(&self, notice: &Notice) -> String {
        let c = copy(self.locale);
        match notice {
            Notice::Launched => c.launched.to_string(),
            Notice::LaunchFailed(detail) => format!("{} {detail}", c.launch_failed),
            Notice::AssociationWritten(_) => c.association_written.to_string(),
            Notice::AssociationUnchanged => c.association_unchanged.to_string(),
            Notice::AssociationFailed(detail) => format!("{} {detail}", c.association_failed),
            Notice::AssociationRolledBack => c.association_rolled_back.to_string(),
        }
    }

    fn warning_message_for(&self, warning: &AssociationWarning) -> &'static str {
        warning_message(self.locale, warning)
    }
}

/// What an association warning says to the user.
pub(crate) fn warning_message(locale: Locale, warning: &AssociationWarning) -> &'static str {
    {
        let c = copy(locale);
        match warning {
            AssociationWarning::ApplicationDoesNotDeclareType => c.warning_does_not_declare,
            AssociationWarning::ListedInRemovedAssociations => c.warning_removed_association,
            AssociationWarning::DuplicateDefaultKey => c.warning_duplicate_key,
        }
    }
}

impl AppChooser {
    fn row(&self, entry: &RankedApplication, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&entry.desktop_id);
        let c = copy(self.locale);
        let mut badges = vec![
            better_ui::badge(
                self.compatibility_label_for(&entry.compatibility),
                self.badge_style(entry.compatibility.declares_selected_type(), cx),
            )
            .into_any_element(),
            better_ui::badge(
                source_label(self.locale, entry.source_kind, entry.scope),
                self.badge_style(false, cx),
            )
            .into_any_element(),
        ];
        if entry.is_default {
            badges.insert(
                0,
                better_ui::badge(c.badge_default, self.badge_style(true, cx)).into_any_element(),
            );
        }
        let glyph = entry
            .display_name
            .chars()
            .next()
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let desktop_id = entry.desktop_id.clone();

        div()
            .id(SharedString::from(entry.desktop_id.as_str().to_string()))
            .w_full()
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select(desktop_id.clone(), cx);
            }))
            .child(better_ui::application_list_row(
                glyph,
                entry.display_name.clone(),
                entry.desktop_id.as_str().to_string(),
                badges,
                self.tile_style(selected, cx),
            ))
            .into_any_element()
    }

    fn section(
        &self,
        title: &'static str,
        entries: &[RankedApplication],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let visible: Vec<&RankedApplication> = entries
            .iter()
            .filter(|entry| self.matches_query(entry))
            .collect();
        if visible.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(title),
                )
                .children(
                    visible
                        .into_iter()
                        .map(|entry| self.row(entry, cx))
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
        )
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let (title, subtitle) = match self.mode {
            ChooserMode::OpenWith => (c.open_with_title, c.open_with_subtitle),
            ChooserMode::ChooseExecutable => (c.executable_title, c.executable_subtitle),
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(div().text_xl().font_bold().child(title))
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{subtitle}  {}  ({})",
                        self.target.display_name,
                        self.target.mime_type.as_str()
                    )),
            )
            .into_any_element()
    }

    fn actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let has_selection = self.selected_record().is_some();
        h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .flex_wrap()
            .justify_end()
            .child(
                Button::new("chooser-cancel")
                    .label(c.cancel)
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(ChooserEvent::Cancelled);
                    })),
            )
            .when(self.mode == ChooserMode::OpenWith, |row| {
                row.child(
                    Button::new("chooser-always")
                        .label(c.always_use)
                        .disabled(!has_selection)
                        .on_click(cx.listener(|this, _, _, cx| this.always_use(cx))),
                )
                .child(
                    Button::new("chooser-once")
                        .primary()
                        .label(c.open_once)
                        .disabled(!has_selection)
                        .on_click(cx.listener(|this, _, _, cx| this.open_once(cx))),
                )
            })
            .into_any_element()
    }

    fn explanation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.snapshot()?;
        let desktop_id = self.selected.as_ref()?;
        let compatibility = snapshot.sections.compatibility_of(desktop_id)?;
        let message = compatibility_explanation(self.locale, compatibility)?;
        Some(
            better_ui::notice(
                message,
                cx.theme().warning_foreground,
                cx.theme().warning,
                cx.theme().radius,
            )
            .into_any_element(),
        )
    }

    fn notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = copy(self.locale);
        let mut elements: Vec<AnyElement> = Vec::new();
        if let Some(notice) = &self.notice {
            let failed = matches!(
                notice,
                Notice::LaunchFailed(_) | Notice::AssociationFailed(_)
            );
            elements.push(
                better_ui::notice(
                    self.notice_message(notice),
                    if failed {
                        cx.theme().danger_foreground
                    } else {
                        cx.theme().foreground
                    },
                    if failed {
                        cx.theme().danger
                    } else {
                        cx.theme().muted
                    },
                    cx.theme().radius,
                )
                .into_any_element(),
            );
            if let Notice::AssociationWritten(rollback) = notice {
                let rollback = rollback.as_ref().clone();
                elements.push(
                    Button::new("chooser-undo")
                        .label(c.undo)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.undo_association(rollback.clone(), cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        for warning in &self.warnings {
            elements.push(
                better_ui::notice(
                    self.warning_message_for(warning),
                    cx.theme().warning_foreground,
                    cx.theme().warning,
                    cx.theme().radius,
                )
                .into_any_element(),
            );
        }
        elements
    }

    fn executable_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let mut panel = v_flex().w_full().min_w_0().gap_3();
        if let Some(resolution) = &self.executable {
            let refused = matches!(resolution, ExecutableResolution::Refused(_));
            panel = panel.child(better_ui::notice(
                executable_message(self.locale, resolution),
                if refused {
                    cx.theme().warning_foreground
                } else {
                    cx.theme().foreground
                },
                if refused {
                    cx.theme().warning
                } else {
                    cx.theme().muted
                },
                cx.theme().radius,
            ));
            if let ExecutableResolution::Resolved(path) = resolution {
                let path = path.clone();
                panel = panel.child(
                    Button::new("chooser-use-executable")
                        .primary()
                        .label(c.executable_use_path)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_executable_path(path.clone(), cx);
                        })),
                );
            }
        }
        if let Some(path) = &self.chosen_executable {
            panel = panel.child(better_ui::notice(
                format!("{}: {}", c.executable_selected, path.display()),
                cx.theme().foreground,
                cx.theme().muted,
                cx.theme().radius,
            ));
        }

        panel = panel.child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(c.executable_browse),
        );
        if self.browse_roots.is_empty() {
            return panel
                .child(better_ui::state_message(
                    c.executable_browse_empty,
                    c.executable_browse_hint,
                    cx.theme().foreground,
                    cx.theme().muted_foreground,
                ))
                .into_any_element();
        }
        panel = panel.child(
            h_flex().gap_2().flex_wrap().children(
                self.browse_roots
                    .iter()
                    .enumerate()
                    .map(|(index, root)| {
                        let root = root.clone();
                        Button::new(("chooser-browse-root", index))
                            .label(root.display().to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.browsing = Some(root.clone());
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            ),
        );
        if let Some(directory) = &self.browsing {
            let programs = app_chooser_core::list_executables(directory);
            panel = panel.child(
                v_flex().w_full().min_w_0().gap_1().children(
                    programs
                        .into_iter()
                        .take(200)
                        .enumerate()
                        .map(|(index, path)| {
                            let label = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            Button::new(("chooser-program", index))
                                .label(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.choose_executable_path(path.clone(), cx);
                                }))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                ),
            );
        }
        panel.into_any_element()
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let Some(snapshot) = self.snapshot() else {
            return better_ui::state_message(
                c.loading_title,
                c.loading_detail,
                cx.theme().foreground,
                cx.theme().muted_foreground,
            )
            .into_any_element();
        };
        if snapshot.sections.is_empty() {
            return better_ui::state_message(
                c.empty_title,
                c.empty_detail,
                cx.theme().foreground,
                cx.theme().muted_foreground,
            )
            .into_any_element();
        }

        let mut sections: Vec<AnyElement> = Vec::new();
        if let Some(section) =
            self.section(c.section_recommended, &snapshot.sections.recommended, cx)
        {
            sections.push(section);
        }
        if let Some(section) = self.section(c.section_other, &snapshot.sections.other, cx) {
            sections.push(section);
        }
        if self.expanded {
            if let Some(section) = self.section(c.section_all, &snapshot.sections.all, cx) {
                sections.push(section);
            }
        }
        if sections.is_empty() {
            sections.push(
                better_ui::state_message(
                    c.no_matches_title,
                    c.no_matches_detail,
                    cx.theme().foreground,
                    cx.theme().muted_foreground,
                )
                .into_any_element(),
            );
        }

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .children(sections)
            .child(
                Button::new("chooser-expand")
                    .label(if self.expanded {
                        c.hide_all
                    } else {
                        c.show_all
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expanded = !this.expanded;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

impl Render for AppChooser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = copy(self.locale);
        v_flex()
            .size_full()
            .min_w_0()
            .gap_4()
            .p_5()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.header(cx))
            .child(
                Input::new(&self.search)
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).small()),
            )
            .children(self.notices(cx))
            .children(self.explanation(cx))
            .when(self.selected.is_none(), |view| {
                view.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.nothing_selected),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(self.body(cx)),
            )
            .when(self.mode == ChooserMode::ChooseExecutable, |view| {
                view.child(self.executable_panel(cx))
            })
            .child(self.actions(cx))
    }
}

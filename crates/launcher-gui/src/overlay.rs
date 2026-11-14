//! The Start menu itself.
//!
//! One GPUI view drawn with `better_ui::fluent`: the search box at the top,
//! the Start page, All apps, or search results under it, and a bar at the
//! bottom with the appearance choice. It renders [`crate::model::OverlayModel`]
//! and routes keys, clicks, drags, and menu choices back into it; what each
//! of them means is decided there.
//!
//! Three things happen off the render thread. Reading every application
//! directory, building the index, finding each app's icon in the icon theme,
//! and reading the stored Start layout is one background task, so the window
//! opens and the search box takes focus before the list exists. Watching
//! those directories is a second background task that blocks on the kernel's
//! notifications rather than re-reading on a timer. Opening a file or a
//! folder hands it to `xdg-open` as a detached process.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use app_catalog_core::{DesktopId, IconReference};
use app_catalog_platform::SessionEnvironment;
use app_catalog_platform::launch::SystemSpawner;
use better_ui::AppIcon;
use better_ui::fluent::button::{Button, ButtonKind};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::{GridTile, ListRow, TILE_WIDTH};
use better_ui::fluent::menu::{ContextMenu, MenuEntry, MenuFlyout, MenuItem, MenuOpen, open_menu};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::theme::Appearance;
use better_ui::fluent::{
    BODY, BODY_STRONG, CAPTION, Palette, SUBTITLE, TITLE, label, palette, radius, spacing,
};
use better_ui::icons::IconSearch;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::IconName;
use gpui_component::input::{
    InputEvent, InputState, MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp,
    MoveRight, MoveUp,
};
use launcher_core::LauncherApplication;
use launcher_platform::catalog::{LauncherSnapshot, MetadataWatch, load_snapshot};
use launcher_platform::{
    CatalogLauncher, ManagedApplications, SessionCapabilities, open_location_now,
    open_manager_component,
};

use crate::i18n::{
    Locale, command_label, copied, copy, group_label, no_matches, uninstall_detail, when,
};
use crate::model::{
    Activation, AllAppsRow, Command, Effect, GroupKey, Highlight, JUMP_COLUMNS, LoadState, NavKey,
    Notice, OverlayModel, PINNED_COLUMNS, Place, RECOMMENDED_COLUMNS, Screen, Target, nav_key,
};
use crate::pins::{Pins, Recent, StartStore, now};
use crate::ranking::{FileSystem, PATH_TIMEOUT, PathKind, PathProbe, SearchItem, path_hits_within};

/// How long a watch waits before re-arming. Long on purpose: the wait is
/// blocked on the kernel's event channel, so a short timeout would be a wake-up
/// that learns nothing. Nothing happens on this task while the directories are
/// idle.
const WATCH_TIMEOUT: Duration = Duration::from_secs(3600);

/// The Start menu's side margins and the pinned grid's visible rows.
const MARGIN: Pixels = px(32.0);
const PINNED_ROWS: usize = 3;
/// The width of the actions pane beside search results.
const DETAIL_WIDTH: Pixels = px(232.0);
/// Room around a scrolling list for the keyboard focus ring, which is drawn
/// 3px outside an item and would otherwise be clipped by the list's edge.
const RING: Pixels = px(4.0);

/// What the overlay tells whoever opened it.
#[derive(Clone, Debug)]
pub enum OverlayEvent {
    /// Escape, or something was opened. The launcher has finished.
    Closed,
}

impl EventEmitter<OverlayEvent> for LauncherOverlay {}

/// What the background read produced.
struct Loaded {
    snapshot: LauncherSnapshot,
    icons: BTreeMap<DesktopId, AppIcon>,
    managed: ManagedApplications,
    /// `Err` when a pins file exists and cannot be read.
    pins: Result<Option<Pins>, String>,
    recent: Recent,
}

/// Where the launcher reads from and writes to: the application folders, the
/// icon themes, the stored Start layout and appearance, the filesystem a
/// path query browses, and whether the folders are watched. The real ones
/// come from the session; the key-path tests give it scratch ones, so a test
/// never reads the person's own files or watches their folders.
#[derive(Clone)]
pub struct OverlaySources {
    pub session: SessionEnvironment,
    pub store: StartStore,
    /// Where the appearance choice is stored; `None` keeps it in memory.
    pub appearance: Option<std::path::PathBuf>,
    pub icons: Arc<dyn Fn() -> IconSearch + Send + Sync>,
    pub paths: Arc<dyn PathProbe + Send + Sync>,
    pub watch: bool,
}

impl OverlaySources {
    pub fn from_environment() -> Self {
        Self {
            session: SessionEnvironment::from_env(),
            store: StartStore::from_environment(),
            appearance: Appearance::default_path("launcher"),
            icons: Arc::new(IconSearch::from_environment),
            paths: Arc::new(FileSystem),
            watch: true,
        }
    }
}

/// A context menu that is open, and what it is for.
struct OpenMenu {
    state: Entity<MenuOpen>,
    position: Point<Pixels>,
    target: Target,
    commands: Vec<Command>,
    /// Opened with the pointer: the item it was opened on is not left
    /// highlighted once a command has run, since nobody is using the
    /// keyboard.
    pointer: bool,
}

pub struct LauncherOverlay {
    locale: Locale,
    model: OverlayModel,
    starter: Option<CatalogLauncher>,
    capabilities: SessionCapabilities,
    search: Entity<InputState>,
    focus: FocusHandle,
    icons: BTreeMap<DesktopId, AppIcon>,
    sources: OverlaySources,
    started: bool,
    appearance: Appearance,
    menu: Option<OpenMenu>,
    appearance_menu: Entity<MenuOpen>,
    pinned_scroll: ScrollHandle,
    recommended_scroll: ScrollHandle,
    list_scroll: ScrollHandle,
    results_scroll: ScrollHandle,
    _appearance: Option<Subscription>,
    _load: Option<Task<()>>,
    _watch: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    /// Watches for an update and restarts into it (ticket 105). Inactive
    /// until [`LauncherOverlay::watch_for_updates`] is called, which only the
    /// installed program does.
    update_restart: better_ui::restart::UpdateRestart,
}

/// The window's application id, which the desktop entry is named after.
pub const APP_ID: &str = "better-launcher";

/// What the launcher carries over when an update restarts it: what was typed
/// and which page was showing. Everything else is read again.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LauncherView {
    pub query: String,
    pub all_apps: bool,
}

impl LauncherOverlay {
    pub fn new(locale: Locale, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_sources(locale, OverlaySources::from_environment(), window, cx)
    }

    /// The same, reading from and writing to the given places.
    pub fn with_sources(
        locale: Locale,
        sources: OverlaySources,
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
                    this.query_changed(value, cx);
                }
            },
        );
        let appearance = sources
            .appearance
            .as_deref()
            .map(Appearance::load)
            .unwrap_or_default();
        let watch = sources.watch;

        let mut overlay = Self {
            locale,
            model: OverlayModel::new(),
            starter: None,
            capabilities: SessionCapabilities::from_env(),
            search,
            focus: cx.focus_handle(),
            icons: BTreeMap::new(),
            sources,
            started: false,
            appearance,
            menu: None,
            appearance_menu: better_ui::fluent::menu::new_state(cx),
            pinned_scroll: ScrollHandle::new(),
            recommended_scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            results_scroll: ScrollHandle::new(),
            _appearance: None,
            _load: None,
            _watch: None,
            _subscriptions: vec![subscription],
            update_restart: better_ui::restart::UpdateRestart::inactive(APP_ID, cx),
        };
        overlay._appearance = appearance.apply_and_follow(window, cx);
        // Issue #2's first requirement about focus: the search row has it
        // before anything has been read, so someone can start typing into a
        // list that is still loading.
        overlay.search.focus_handle(cx).focus(window, cx);
        overlay.start_load(cx);
        if watch {
            overlay.start_watch(cx);
        }
        overlay
    }

    /// Starts watching for an update, and puts back what a restart after one
    /// handed over.
    pub fn watch_for_updates(
        &mut self,
        handoff: Option<&better_ui::restart::Handoff>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = handoff.and_then(|h| h.state::<LauncherView>()) {
            if view.all_apps {
                self.model.open_all_apps();
            }
            if !view.query.is_empty() {
                let query = view.query.clone();
                self.search
                    .update(cx, |state, cx| state.set_value(query, window, cx));
                self.query_changed(view.query, cx);
            }
        }
        self.update_restart = better_ui::restart::UpdateRestart::new(
            APP_ID,
            "Better Launcher",
            "better-launcher",
            env!("CARGO_PKG_VERSION"),
            handoff,
            window,
            cx,
        );
    }

    /// The search box changed: answer the query, and when it is a path, read
    /// the filesystem for it off the render thread with a deadline.
    fn query_changed(&mut self, value: String, cx: &mut Context<Self>) {
        self.model.set_query(value);
        self.close_menu(cx);
        if let Some((generation, query)) = self.model.path_request(self.sources.paths.home()) {
            let probe = Arc::clone(&self.sources.paths);
            let work =
                cx.background_spawn(async move { path_hits_within(query, probe, PATH_TIMEOUT) });
            cx.spawn(async move |this, cx| {
                if let Some(hits) = work.await {
                    this.update(cx, |this, cx| {
                        if this.model.apply_path_hits(generation, hits) {
                            cx.notify();
                        }
                    })
                    .ok();
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// What the current session can offer.
    pub fn capabilities(&self) -> &SessionCapabilities {
        &self.capabilities
    }

    pub fn model(&self) -> &OverlayModel {
        &self.model
    }

    /// Reads the application directories, the icons, and the stored Start
    /// layout off the render thread, then swaps the result in.
    fn start_load(&mut self, cx: &mut Context<Self>) {
        let entry_locale = self.locale.entry_locale();
        let store = self.sources.store.clone();
        let session = self.sources.session.clone();
        let icon_search = Arc::clone(&self.sources.icons);
        let work = cx.background_spawn(async move {
            let snapshot = load_snapshot(&session, entry_locale);
            let search = icon_search();
            let icons = snapshot
                .index
                .browse()
                .applications()
                .iter()
                .map(|application| {
                    let icon = match &application.icon {
                        Some(IconReference::Name(name)) => Some(name.clone()),
                        Some(IconReference::Path(path)) => Some(path.display().to_string()),
                        None => None,
                    };
                    (
                        application.desktop_id.clone(),
                        AppIcon::for_entry(icon.as_deref(), &application.display_name, &search),
                    )
                })
                .collect();
            Loaded {
                snapshot,
                icons,
                managed: ManagedApplications::built_in(),
                pins: store.load_pins(),
                recent: store.load_recent(),
            }
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let loaded = work.await;
            this.update(cx, |this, cx| this.adopt_loaded(loaded, cx))
                .ok();
        }));
    }

    fn adopt_loaded(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.icons = loaded.icons;
        self.model.set_managed(loaded.managed);
        let first = !self.started;
        self.adopt(loaded.snapshot, cx);
        if first {
            self.started = true;
            match loaded.pins {
                Ok(pins) => self.model.set_start(pins, loaded.recent),
                Err(error) => {
                    // The file is there and cannot be read: show the
                    // starting set for now, and never write over the
                    // person's arrangement.
                    self.sources.store = self.sources.store.without_pins();
                    self.model.set_start(None, loaded.recent);
                    self.model.set_notice(Some(Notice::NotSaved(error)));
                }
            }
        }
        cx.notify();
    }

    /// Swaps a snapshot in. Public to the crate so the key-path tests can
    /// give the overlay a library of their own.
    pub(crate) fn adopt(&mut self, snapshot: LauncherSnapshot, cx: &mut Context<Self>) {
        self.starter = Some(CatalogLauncher::new(
            Arc::clone(&snapshot.catalog),
            self.locale.entry_locale(),
        ));
        self.model.apply_snapshot(snapshot);
        crate::startup::mark(
            crate::startup::STAGE_LIBRARY_READY,
            &format!("applications={}", self.model.library().len()),
        );
        cx.notify();
    }

    /// Gives the model a stored layout, for the key-path tests.
    #[cfg(test)]
    pub(crate) fn set_start(&mut self, pins: Option<Pins>, recent: Recent) {
        self.started = true;
        self.model.set_start(pins, recent);
    }

    /// Waits for the application directories to change, then re-reads them.
    ///
    /// The wait blocks on the watcher's channel, so an idle desktop costs
    /// nothing here. A watcher that could not start is not an error the user
    /// needs to see: the list is simply the one read at open.
    fn start_watch(&mut self, cx: &mut Context<Self>) {
        let session = self.sources.session.clone();
        let work = cx.background_spawn(async move {
            MetadataWatch::start(&session)
                .ok()
                .and_then(|watch| watch.next_change(WATCH_TIMEOUT).map(|_| ()))
        });
        self._watch = Some(cx.spawn(async move |this, cx| {
            let changed = work.await;
            this.update(cx, |this, cx| {
                if changed.is_some() {
                    this.model.begin_refresh();
                    this.start_load(cx);
                    cx.notify();
                }
                // Re-arm either way: a timeout means nothing changed, not that
                // nothing will.
                this.start_watch(cx);
            })
            .ok();
        }));
    }

    fn icon(&self, application: &LauncherApplication) -> AppIcon {
        self.icons
            .get(&application.desktop_id)
            .cloned()
            .unwrap_or_else(|| {
                AppIcon::Letter(better_ui::icons::initial(&application.display_name))
            })
    }

    // ------------------------------------------------------------- actions

    fn save_pins(&mut self) {
        if let Err(error) = self.sources.store.save_pins(&self.model.pins()) {
            self.model
                .set_notice(Some(Notice::NotSaved(error.to_string())));
        }
    }

    fn save_recent(&mut self) {
        if let Err(error) = self.sources.store.save_recent(self.model.recent()) {
            self.model
                .set_notice(Some(Notice::NotSaved(error.to_string())));
        }
    }

    /// Does `command` to `target`. Everything that leaves the launcher closes
    /// it; everything that changes the Start page saves it.
    fn run(
        &mut self,
        target: Target,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.as_ref().is_some_and(|menu| menu.pointer) {
            self.model.set_highlight(None);
        }
        self.close_menu(cx);
        match (&target, &command) {
            (Target::AllApps, _) => self.model.open_all_apps(),
            (Target::Letters, _) => self.model.open_letters(),
            (Target::App(id), Command::Open | Command::RunInTerminal) => {
                let Some(starter) = self.starter.clone() else {
                    return;
                };
                if let Activation::Launched(_) = self.model.launch(id, &starter, now()) {
                    self.save_recent();
                    cx.emit(OverlayEvent::Closed);
                }
            }
            (Target::App(id), Command::PinToStart) => {
                if self.model.pin(id) {
                    self.save_pins();
                }
            }
            (Target::App(id), Command::UnpinFromStart) => {
                if self.model.unpin(id) {
                    self.save_pins();
                }
            }
            (Target::App(id), Command::MoveToFront) => {
                if self.model.move_to_front(id) {
                    self.save_pins();
                }
            }
            (Target::App(id), Command::RemoveFromList) => {
                if self.model.forget(id) {
                    self.save_recent();
                }
            }
            (Target::App(id), Command::OpenFileLocation) => {
                let folder = self
                    .model
                    .record(id)
                    .and_then(|record| record.source.path.parent().map(|p| p.to_path_buf()));
                if let Some(folder) = folder {
                    self.open_location(&folder, cx);
                }
            }
            (Target::App(_), Command::Uninstall(component)) => {
                let manager = self
                    .model
                    .managed()
                    .manager_entry()
                    .and_then(|id| self.model.record(id))
                    .cloned();
                let outcome = match &manager {
                    Some(manager) => open_manager_component(manager, &component.id, &SystemSpawner),
                    None => Err(launcher_platform::PlatformError::UnknownApplication(
                        "io.betteros.Manager.desktop".to_string(),
                    )),
                };
                match outcome {
                    Ok(()) => cx.emit(OverlayEvent::Closed),
                    Err(error) => self
                        .model
                        .set_notice(Some(Notice::LaunchFailed(error.to_string()))),
                }
            }
            (Target::Calculation(value), _) => {
                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                self.model.set_notice(Some(Notice::Copied(value.clone())));
            }
            (Target::Path(hit), Command::OpenContainingFolder) => {
                if let Some(parent) = hit.path.parent() {
                    let parent = parent.to_path_buf();
                    self.open_location(&parent, cx);
                }
            }
            (Target::Path(hit), _) => {
                let path = hit.path.clone();
                self.open_location(&path, cx);
            }
            _ => {}
        }
        self.search.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn open_location(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match open_location_now(path) {
            Ok(()) => cx.emit(OverlayEvent::Closed),
            Err(error) => self
                .model
                .set_notice(Some(Notice::LaunchFailed(error.to_string()))),
        }
    }

    fn click(&mut self, highlight: Highlight, window: &mut Window, cx: &mut Context<Self>) {
        self.model.set_highlight(Some(highlight));
        if let Some(target) = self.model.target(highlight) {
            let command = self.model.primary(&target);
            self.run(target, command, window, cx);
        }
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            menu.state.update(cx, |state, cx| {
                state.close();
                cx.notify();
            });
        }
    }

    /// Opens the context menu for an item, at a point in the window.
    fn open_context_menu(
        &mut self,
        highlight: Highlight,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.set_highlight(Some(highlight));
        let Some(target) = self.model.target(highlight) else {
            return;
        };
        let place = self.model.place(highlight);
        let commands = self.model.commands(&target, place);
        if commands.is_empty() {
            return;
        }
        self.close_menu(cx);
        let state = better_ui::fluent::menu::new_state(cx);
        open_menu(&state, Some(0), window, cx);
        self.menu = Some(OpenMenu {
            state,
            position,
            target,
            commands,
            pointer: !window.last_input_was_keyboard(),
        });
        cx.notify();
    }

    fn menu_is_open(&self, cx: &App) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|menu| menu.state.read(cx).is_open())
            || self.appearance_menu.read(cx).is_open()
    }

    /// Where a menu opened from the keyboard appears: under the item the
    /// keyboard is on, read from the layout of the last frame.
    fn anchor(&self, highlight: Highlight, window: &Window) -> Point<Pixels> {
        let bounds = match highlight {
            Highlight::Pinned(i) => self.pinned_scroll.bounds_for_item(i),
            Highlight::Recommended(i) => self.recommended_scroll.bounds_for_item(i),
            Highlight::Row(i) => self.list_scroll.bounds_for_item(i),
            Highlight::Result(i) => result_lines(&self.model)
                .iter()
                .position(|line| *line == ResultLine::Item(i))
                .and_then(|child| self.results_scroll.bounds_for_item(child)),
            Highlight::AllAppsButton => None,
        };
        match bounds {
            Some(bounds) => point(bounds.left() + px(24.0), bounds.bottom() - px(8.0)),
            None => {
                let size = window.viewport_size();
                point(size.width / 2.0, size.height / 3.0)
            }
        }
    }

    /// One key, from either a key press or a captured search-box action.
    /// Returns whether the launcher took it; a key it leaves goes on to the
    /// search box.
    fn take(&mut self, key: NavKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.menu_is_open(cx) {
            return false;
        }
        match self.model.handle_key(key) {
            Effect::Pass => return false,
            Effect::Redraw => {}
            Effect::Close => cx.emit(OverlayEvent::Closed),
            Effect::Run(target, command) => self.run(target, command, window, cx),
            Effect::Menu(_, _) => {
                if let Some(highlight) = self.model.highlight() {
                    let position = self.anchor(highlight, window);
                    self.open_context_menu(highlight, position, window, cx);
                }
            }
        }
        cx.notify();
        true
    }

    /// Keys that reach the overlay as key presses: Escape, Enter, Shift+F10,
    /// and the Menu key. The search box binds the arrows, Home, End, Page
    /// Up, and Page Down to actions of its own, which run before any key
    /// listener, so those arrive through [`Self::capture`] instead.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = &event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        if let Some(key) = nav_key(event.keystroke.key.as_str(), modifiers.shift)
            && self.take(key, window, cx)
        {
            cx.stop_propagation();
        }
    }

    fn capture(&mut self, key: NavKey, window: &mut Window, cx: &mut Context<Self>) {
        if self.take(key, window, cx) {
            cx.stop_propagation();
        }
    }

    fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance = appearance;
        if let Some(path) = self.sources.appearance.clone()
            && let Err(error) = appearance.save(&path)
        {
            self.model
                .set_notice(Some(Notice::NotSaved(error.to_string())));
        }
        self._appearance = appearance.apply_and_follow(window, cx);
        self.search.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

/// One line of the search results list: a section heading or a result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResultLine {
    BestMatch,
    Apps,
    Files,
    Item(usize),
}

/// The results list as it is drawn: "Best match" over the first result,
/// then the rest under "Apps" or "Files and folders" where the kind changes.
fn result_lines(model: &OverlayModel) -> Vec<ResultLine> {
    let mut lines = Vec::new();
    let mut previous: Option<ResultLine> = None;
    for (index, item) in model.results().iter().enumerate() {
        let heading = if index == 0 {
            ResultLine::BestMatch
        } else {
            match item {
                SearchItem::App(_) => ResultLine::Apps,
                SearchItem::Path(_) | SearchItem::Calculation(_) => ResultLine::Files,
            }
        };
        if previous != Some(heading) {
            lines.push(heading);
            previous = Some(heading);
        }
        lines.push(ResultLine::Item(index));
    }
    lines
}

/// The glyph beside a command.
fn command_icon(command: &Command) -> IconName {
    match command {
        Command::Open | Command::OpenPath => IconName::ExternalLink,
        Command::RunInTerminal => IconName::SquareTerminal,
        Command::PinToStart => IconName::Star,
        Command::UnpinFromStart => IconName::StarOff,
        Command::MoveToFront => IconName::ArrowLeft,
        Command::RemoveFromList => IconName::Close,
        Command::OpenFileLocation | Command::OpenContainingFolder => IconName::FolderOpen,
        Command::Uninstall(_) => IconName::Delete,
        Command::CopyResult => IconName::Copy,
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// A tile's height, re-exported for the pinned grid's visible rows.
const TILE_HEIGHT: Pixels = better_ui::fluent::list::TILE_HEIGHT;

impl LauncherOverlay {
    /// The letter tile for something that is not an app: `=` for a sum.
    fn letter(&self, glyph: &str, size: Pixels, p: &Palette) -> AnyElement {
        AppIcon::Letter(glyph.to_string().into()).element(
            size,
            p.text_on_accent.into(),
            p.accent.into(),
            radius::SM,
        )
    }

    fn heading(&self, text: &'static str, p: &Palette) -> Div {
        label(text, BODY_STRONG, p.text_primary)
    }

    fn state_message(&self, title: String, detail: &'static str, p: &Palette) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(spacing::SM)
            .pt(px(64.0))
            .child(label(title, BODY_STRONG, p.text_primary))
            .child(label(detail, CAPTION, p.text_secondary))
            .into_any_element()
    }

    fn home(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let highlight = self.model.highlight();
        let this = cx.entity().downgrade();

        let all_apps = Button::new("all-apps", ButtonKind::Standard)
            .label(c.all_apps)
            .trailing_icon(IconName::ChevronRight)
            .shown(if highlight == Some(Highlight::AllAppsButton) {
                better_ui::fluent::ShownState::Focused
            } else {
                better_ui::fluent::ShownState::Live
            })
            .on_click({
                let this = this.clone();
                move |window, cx| {
                    this.update(cx, |o, cx| o.click(Highlight::AllAppsButton, window, cx))
                        .ok();
                }
            });

        let pinned = self.model.pinned();
        let pinned_grid: AnyElement = if pinned.is_empty() {
            label(c.pinned_empty, CAPTION, p.text_secondary)
                .h(TILE_HEIGHT)
                .into_any_element()
        } else {
            div()
                .id("pinned")
                .w(TILE_WIDTH * PINNED_COLUMNS as f32 + RING * 2.0)
                .mx(-RING)
                .p(RING)
                .max_h(TILE_HEIGHT * PINNED_ROWS as f32 + RING * 2.0)
                .flex()
                .flex_wrap()
                .overflow_y_scroll()
                .track_scroll(&self.pinned_scroll)
                .children(pinned.iter().enumerate().map(|(index, application)| {
                    let at = Highlight::Pinned(index);
                    let click = this.clone();
                    let secondary = this.clone();
                    let reorder = this.clone();
                    GridTile::new(
                        ElementId::named_usize("pin", index),
                        self.icon(application),
                        application.display_name.clone(),
                    )
                    .tooltip(application.display_name.clone())
                    .highlighted(highlight == Some(at))
                    .on_click(move |_, window, cx| {
                        click.update(cx, |o, cx| o.click(at, window, cx)).ok();
                    })
                    .on_secondary(move |position, window, cx| {
                        secondary
                            .update(cx, |o, cx| o.open_context_menu(at, position, window, cx))
                            .ok();
                    })
                    .reorderable("pinned", index, move |from, to, _, cx| {
                        reorder
                            .update(cx, |o, cx| {
                                if o.model.move_pinned(from, to) {
                                    o.save_pins();
                                }
                                cx.notify();
                            })
                            .ok();
                    })
                }))
                .into_any_element()
        };

        let recommended = self.model.recommended();
        let now = now();
        let recommended_grid: AnyElement = if recommended.is_empty() {
            label(c.recommended_empty, CAPTION, p.text_secondary).into_any_element()
        } else {
            div()
                .id("recommended")
                .mx(-RING)
                .p(RING)
                .flex()
                .flex_wrap()
                .overflow_y_scroll()
                .track_scroll(&self.recommended_scroll)
                .children(recommended.iter().enumerate().map(|(index, item)| {
                    let at = Highlight::Recommended(index);
                    let click = this.clone();
                    let secondary = this.clone();
                    div()
                        .w(relative(1.0 / RECOMMENDED_COLUMNS as f32))
                        .pr(spacing::XS)
                        .child(
                            ListRow::new(
                                ElementId::named_usize("recent", index),
                                item.application.display_name.clone(),
                            )
                            .caption(when(self.locale, now, item.launch.last))
                            .app_icon(self.icon(item.application))
                            .highlighted(highlight == Some(at))
                            .on_click(move |_, window, cx| {
                                click.update(cx, |o, cx| o.click(at, window, cx)).ok();
                            })
                            .on_secondary(
                                move |position, window, cx| {
                                    secondary
                                        .update(cx, |o, cx| {
                                            o.open_context_menu(at, position, window, cx)
                                        })
                                        .ok();
                                },
                            ),
                        )
                }))
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(spacing::SM)
                    .child(self.heading(c.pinned, p))
                    .child(all_apps),
            )
            .child(pinned_grid)
            .child(
                self.heading(c.recommended, p)
                    .mt(spacing::XL)
                    .mb(spacing::SM),
            )
            .child(recommended_grid)
            .into_any_element()
    }

    fn all_apps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let this = cx.entity().downgrade();
        let back = Button::new("back", ButtonKind::Standard)
            .label(c.back)
            .icon(IconName::ChevronLeft)
            .on_click({
                let this = this.clone();
                move |window, cx| {
                    this.update(cx, |o, cx| {
                        o.model.go_home();
                        o.search.focus_handle(cx).focus(window, cx);
                        cx.notify();
                    })
                    .ok();
                }
            });
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .mb(spacing::SM)
            .child(self.heading(c.all_apps, p))
            .child(back);

        if let Some(at) = self.model.jump() {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(header)
                .child(self.letters(at, cx))
                .into_any_element();
        }

        let highlight = self.model.highlight();
        if self.model.take_scroll()
            && let Some(Highlight::Row(row)) = highlight
        {
            // A letter jumped to goes to the top, as Windows puts it; any
            // other row is only brought into view.
            if matches!(
                self.model.all_apps_rows().get(row),
                Some(AllAppsRow::Header(_))
            ) {
                self.list_scroll.scroll_to_top_of_item(row);
            } else {
                self.list_scroll.scroll_to_item(row);
            }
        }
        let library = self.model.library();
        let rows = self.model.all_apps_rows();
        let list = div()
            .id("all-apps")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .mx(-RING)
            .p(RING)
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .children(rows.iter().enumerate().map(|(index, row)| {
                let at = Highlight::Row(index);
                let highlighted = highlight == Some(at);
                match row {
                    AllAppsRow::Header(key) => {
                        let click = this.clone();
                        div()
                            .flex()
                            .pt(if index == 0 { px(0.0) } else { spacing::SM })
                            .pb(spacing::XS)
                            .child(
                                Button::new(
                                    ElementId::named_usize("group", index),
                                    ButtonKind::Subtle,
                                )
                                .label(group_label(self.locale, *key))
                                .tooltip(c.letters_hint)
                                .shown(if highlighted {
                                    better_ui::fluent::ShownState::Focused
                                } else {
                                    better_ui::fluent::ShownState::Live
                                })
                                .on_click(move |window, cx| {
                                    click.update(cx, |o, cx| o.click(at, window, cx)).ok();
                                }),
                            )
                            .into_any_element()
                    }
                    AllAppsRow::App(app) => {
                        let application = &library[*app];
                        let click = this.clone();
                        let secondary = this.clone();
                        ListRow::new(
                            ElementId::named_usize("app", index),
                            application.display_name.clone(),
                        )
                        .app_icon(self.icon(application))
                        .highlighted(highlighted)
                        .on_click(move |_, window, cx| {
                            click.update(cx, |o, cx| o.click(at, window, cx)).ok();
                        })
                        .on_secondary(move |position, window, cx| {
                            secondary
                                .update(cx, |o, cx| o.open_context_menu(at, position, window, cx))
                                .ok();
                        })
                        .into_any_element()
                    }
                }
            }));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(list)
            .into_any_element()
    }

    /// The letter grid: `#`, A to Z, and Other, the letters without a
    /// section disabled.
    fn letters(&self, at: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let this = cx.entity().downgrade();
        let keys = GroupKey::all();
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(spacing::MD)
            .pt(spacing::XL)
            .child(label(c.letters_hint, CAPTION, p.text_secondary))
            .child(
                div()
                    .w(px(56.0) * JUMP_COLUMNS as f32)
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .children(keys.into_iter().enumerate().map(|(index, key)| {
                        let click = this.clone();
                        div().w(px(56.0)).h(px(48.0)).flex().justify_center().child(
                            Button::new(
                                ElementId::named_usize("letter", index),
                                ButtonKind::Subtle,
                            )
                            .label(group_label(self.locale, key))
                            .disabled(!self.model.has_group(key))
                            .shown(if index == at {
                                better_ui::fluent::ShownState::Focused
                            } else {
                                better_ui::fluent::ShownState::Live
                            })
                            .on_click(move |window, cx| {
                                click
                                    .update(cx, |o, cx| {
                                        o.model.jump_to(key);
                                        o.search.focus_handle(cx).focus(window, cx);
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                        )
                    })),
            )
            .into_any_element()
    }

    fn result_row(&self, index: usize, item: &SearchItem, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let this = cx.entity().downgrade();
        let at = Highlight::Result(index);
        let best = index == 0;
        let highlighted = self.model.highlight() == Some(at);
        let id = ElementId::named_usize("result", index);
        let row = match item {
            SearchItem::App(application) => {
                let kind = if self.model.runs_in_terminal(&application.desktop_id) {
                    c.kind_terminal_app
                } else {
                    c.kind_app
                };
                ListRow::new(id, application.display_name.clone())
                    .app_icon(self.icon(application))
                    .when(best, |row| row.caption(kind))
            }
            SearchItem::Calculation(calculation) => {
                ListRow::new(id, format!("= {}", calculation.display))
                    .app_icon(AppIcon::Letter("=".into()))
                    .caption(calculation.expression.clone())
            }
            SearchItem::Path(hit) => ListRow::new(id, hit.name())
                .glyph(if hit.kind == PathKind::Folder {
                    IconName::Folder
                } else {
                    IconName::File
                })
                .caption(crate::ranking::display_path(
                    &hit.path,
                    self.sources.paths.home().as_deref(),
                )),
        };
        let click = this.clone();
        let secondary = this.clone();
        let _ = p;
        row.selected(highlighted)
            .on_click(move |_, window, cx| {
                click.update(cx, |o, cx| o.click(at, window, cx)).ok();
            })
            .on_secondary(move |position, window, cx| {
                secondary
                    .update(cx, |o, cx| o.open_context_menu(at, position, window, cx))
                    .ok();
            })
            .into_any_element()
    }

    /// The pane beside the results: the selected result, large, and what
    /// can be done with it — the same entries as its context menu.
    fn detail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = palette(cx);
        let c = copy(self.locale);
        let highlight = self.model.highlight().unwrap_or(Highlight::Result(0));
        let Highlight::Result(index) = highlight else {
            return None;
        };
        let item = self.model.results().get(index)?;
        let target = self.model.target(highlight)?;
        let (icon, title, kind): (AnyElement, String, String) = match item {
            SearchItem::App(application) => (
                self.icon(application).element(
                    px(64.0),
                    p.text_on_accent.into(),
                    p.accent.into(),
                    radius::SM,
                ),
                application.display_name.clone(),
                if self.model.runs_in_terminal(&application.desktop_id) {
                    c.kind_terminal_app
                } else {
                    c.kind_app
                }
                .to_string(),
            ),
            SearchItem::Calculation(calculation) => (
                self.letter("=", px(64.0), p),
                format!("= {}", calculation.display),
                format!("{} · {}", c.kind_calculation, calculation.expression),
            ),
            SearchItem::Path(hit) => (
                gpui_component::Icon::new(if hit.kind == PathKind::Folder {
                    IconName::FolderOpen
                } else {
                    IconName::File
                })
                .size(px(64.0))
                .text_color(p.text_secondary)
                .into_any_element(),
                hit.name(),
                if hit.kind == PathKind::Folder {
                    c.kind_folder
                } else {
                    c.kind_file
                }
                .to_string(),
            ),
        };
        let title_style = if matches!(item, SearchItem::Calculation(_)) {
            TITLE
        } else {
            SUBTITLE
        };
        let commands = self.model.commands(&target, Place::Search);
        let this = cx.entity().downgrade();
        Some(
            div()
                .w(DETAIL_WIDTH)
                .flex_none()
                .flex()
                .flex_col()
                .rounded(radius::SM)
                .border_1()
                .border_color(p.card_stroke)
                .bg(p.card)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(spacing::SM)
                        .p(spacing::LG)
                        .child(icon)
                        .child(
                            label(title, title_style, p.text_primary)
                                .text_center()
                                .w_full()
                                .line_clamp(2),
                        )
                        .child(label(kind, CAPTION, p.text_secondary).text_center()),
                )
                .child(div().h(px(1.0)).bg(p.divider))
                .child(div().flex().flex_col().p(spacing::XS).children(
                    commands.into_iter().enumerate().map(|(position, command)| {
                        let run = this.clone();
                        let target = target.clone();
                        let detail = match &command {
                            Command::Uninstall(component) => {
                                Some(uninstall_detail(self.locale, &component.display_name))
                            }
                            _ => None,
                        };
                        let run_command = command.clone();
                        ListRow::new(
                            ElementId::named_usize("action", position),
                            command_label(self.locale, &command),
                        )
                        .glyph(command_icon(&command))
                        .when_some(detail, |row, detail| row.caption(detail))
                        .on_click(move |_, window, cx| {
                            run.update(cx, |o, cx| {
                                o.run(target.clone(), run_command.clone(), window, cx)
                            })
                            .ok();
                        })
                    }),
                ))
                .into_any_element(),
        )
    }

    fn search_results(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        if self.model.is_empty_result() {
            return self.state_message(
                no_matches(self.locale, self.model.query()),
                c.no_matches_detail,
                p,
            );
        }
        let lines = result_lines(&self.model);
        if self.model.take_scroll()
            && let Some(Highlight::Result(index)) = self.model.highlight()
            && let Some(child) = lines
                .iter()
                .position(|line| *line == ResultLine::Item(index))
        {
            self.results_scroll.scroll_to_item(child);
        }
        let results: Vec<SearchItem> = self.model.results().to_vec();
        let list = div()
            .id("results")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .ml(-RING)
            .p(RING)
            .overflow_y_scroll()
            .track_scroll(&self.results_scroll)
            .children(lines.into_iter().map(|line| {
                match line {
                    ResultLine::BestMatch => self
                        .heading(c.best_match, p)
                        .pb(spacing::XS)
                        .into_any_element(),
                    ResultLine::Apps => self
                        .heading(c.apps, p)
                        .pt(spacing::MD)
                        .pb(spacing::XS)
                        .into_any_element(),
                    ResultLine::Files => self
                        .heading(c.files_and_folders, p)
                        .pt(spacing::MD)
                        .pb(spacing::XS)
                        .into_any_element(),
                    ResultLine::Item(index) => self.result_row(index, &results[index], cx),
                }
            }));
        div()
            .flex()
            .gap(spacing::LG)
            .flex_1()
            .min_h_0()
            .child(list)
            .children(self.detail(cx))
            .into_any_element()
    }

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        if self.model.load_state() == LoadState::Loading {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing::MD)
                .pt(px(96.0))
                .child(ProgressRing::new("loading", Progress::Indeterminate))
                .child(label(c.loading_title, BODY, p.text_secondary))
                .into_any_element();
        }
        if self.model.is_empty_library() {
            return self.state_message(
                c.empty_library_title.to_string(),
                c.empty_library_detail,
                p,
            );
        }
        match self.model.screen() {
            Screen::Home => self.home(cx),
            Screen::AllApps => self.all_apps(cx),
            Screen::Search => self.search_results(cx),
        }
    }

    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let this = cx.entity().downgrade();
        let dismiss = move |_: &mut Window, cx: &mut App| {
            this.update(cx, |o, cx| {
                o.model.set_notice(None);
                cx.notify();
            })
            .ok();
        };
        Some(match self.model.notice()? {
            Notice::LaunchFailed(key) => ErrorBar::new(
                "launch-failed",
                ErrorReport::new(c.launch_failed, key.clone()).advice(c.launch_failed_advice),
            )
            .locale(self.locale.words())
            .on_close(dismiss)
            .into_any_element(),
            Notice::Copied(value) => {
                InfoBar::new("copied", Severity::Success, copied(self.locale, value))
                    .locale(self.locale.words())
                    .on_close(dismiss)
                    .into_any_element()
            }
            Notice::NotSaved(detail) => {
                ErrorBar::new("not-saved", ErrorReport::new(c.not_saved, detail.clone()))
                    .locale(self.locale.words())
                    .on_close(dismiss)
                    .into_any_element()
            }
        })
    }

    /// The bar along the bottom: the appearance choice, where Windows has
    /// the account and power buttons.
    fn bottom_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let this = cx.entity().downgrade();
        let open = self.appearance_menu.clone();
        let status = (self.model.load_state() == LoadState::Refreshing)
            .then(|| label(c.refreshing, CAPTION, p.text_secondary));
        let entries: Vec<MenuEntry> = [
            (Appearance::Dark, c.appearance_dark),
            (Appearance::Light, c.appearance_light),
            (Appearance::System, c.appearance_system),
        ]
        .into_iter()
        .map(|(appearance, name)| {
            let this = this.clone();
            let mut item = MenuItem::new(name).on_select(move |window, cx| {
                this.update(cx, |o, cx| o.set_appearance(appearance, window, cx))
                    .ok();
            });
            if appearance == self.appearance {
                item = item.icon(IconName::Check);
            }
            item.entry()
        })
        .collect();
        let button = Button::icon_button("appearance", IconName::Palette, c.appearance).on_click({
            let open = open.clone();
            move |window, cx| {
                if open.read(cx).is_open() {
                    open.update(cx, |state, cx| {
                        state.close();
                        cx.notify();
                    });
                } else {
                    open_menu(&open, Some(0), window, cx);
                }
            }
        });
        let flyout = open.read(cx).is_open().then(|| {
            MenuFlyout::new("appearance-menu", entries)
                .state(open.clone())
                .return_focus(self.search.focus_handle(cx))
        });
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(56.0))
            .px(MARGIN)
            .border_t_1()
            .border_color(p.divider)
            .child(div().children(status))
            .child(div().relative().child(button).children(flyout))
            .into_any_element()
    }
}

impl Focusable for LauncherOverlay {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LauncherOverlay {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        // The search box's clear button empties it without a change event;
        // the box's text is the query either way.
        let typed = self.search.read(cx).value().to_string();
        if typed != self.model.query() {
            self.query_changed(typed, cx);
        }
        let body = self.body(cx);
        let notice = self.notice(cx);
        let bottom = self.bottom_bar(cx);
        let menu = self.menu.as_ref().and_then(|menu| {
            if !menu.state.read(cx).is_open() {
                return None;
            }
            let this = cx.entity().downgrade();
            let entries: Vec<MenuEntry> = menu
                .commands
                .iter()
                .map(|command| {
                    let this = this.clone();
                    let target = menu.target.clone();
                    let run = command.clone();
                    MenuItem::new(command_label(self.locale, command))
                        .icon(command_icon(command))
                        .on_select(move |window, cx| {
                            this.update(cx, |o, cx| o.run(target.clone(), run.clone(), window, cx))
                                .ok();
                        })
                        .entry()
                })
                .collect();
            Some(
                ContextMenu::new("item-menu", entries, menu.state.clone(), menu.position)
                    .return_focus(self.search.focus_handle(cx)),
            )
        });

        div()
            .id("better-launcher")
            .track_focus(&self.focus)
            .key_context("BetterLauncher")
            .capture_key_down(cx.listener(Self::on_key))
            // The search box binds the plain navigation keys to its own
            // cursor actions, and a binding runs before any key listener, so
            // each is taken on its way down to the box. The launcher keeps it
            // when it means something on this page (Up and Down always; Left
            // and Right only in the pinned grid); otherwise it goes on and
            // moves the text cursor. Shift and Ctrl with these keys are other
            // actions and stay with the box.
            .capture_action(
                cx.listener(|this, _: &MoveLeft, w, cx| this.capture(NavKey::Left, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveRight, w, cx| this.capture(NavKey::Right, w, cx)),
            )
            .capture_action(cx.listener(|this, _: &MoveUp, w, cx| this.capture(NavKey::Up, w, cx)))
            .capture_action(
                cx.listener(|this, _: &MoveDown, w, cx| this.capture(NavKey::Down, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveHome, w, cx| this.capture(NavKey::Home, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveEnd, w, cx| this.capture(NavKey::End, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MovePageUp, w, cx| this.capture(NavKey::PageUp, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MovePageDown, w, cx| this.capture(NavKey::PageDown, w, cx)),
            )
            .relative()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.window_background)
            .text_color(p.text_primary)
            // The Start menu is a 640 by 720 panel. On a desktop that gives
            // the window the size it asks for, the panel is the window; a
            // tiling compositor that stretches the window gets the panel
            // centred in it rather than a Start menu two metres wide.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .max_w(px(crate::START_WIDTH))
                    .max_h(px(crate::START_HEIGHT))
                    .bg(p.flyout)
                    .border_1()
                    .border_color(p.flyout_stroke)
                    .rounded(radius::MD)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .px(MARGIN)
                            .pt(spacing::XL)
                            .pb(spacing::LG)
                            .gap(spacing::MD)
                            .child(TextBox::search("launcher-search", &self.search))
                            .children(notice)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .pt(spacing::SM)
                                    .child(body),
                            ),
                    )
                    .child(bottom),
            )
            .children(menu)
            .children(self.update_restart.toast_layer(self.locale.words(), cx))
    }
}

impl better_ui::restart::Restartable for LauncherOverlay {
    type State = LauncherView;

    fn update_restart(&mut self) -> &mut better_ui::restart::UpdateRestart {
        &mut self.update_restart
    }

    /// The overlay holds nothing a restart would lose: opening something
    /// closes it, and what is typed is carried over.
    fn restart_blocked(&self, _cx: &App) -> Option<better_ui::restart::Blocker> {
        None
    }

    fn restart_state(&self, _window: &Window, cx: &App) -> LauncherView {
        LauncherView {
            query: self.search.read(cx).value().to_string(),
            all_apps: self.model.page() == crate::model::Page::AllApps,
        }
    }
}

#[cfg(test)]
mod tests {
    //! The key path itself, through GPUI's real dispatch.
    //!
    //! `handle_key` deciding what a key means is not enough: a key only does
    //! that if it reaches the launcher. The search box's own key bindings run
    //! before any key listener, so these tests open the launcher in a
    //! headless window, press keys the way the platform would, and read the
    //! highlight and the query back.

    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AnyWindowHandle, AppContext as _, AsyncApp, Bounds, Entity, Keystroke, WindowBounds,
        WindowOptions, point, px, size,
    };
    use gpui_component::Root;

    use super::LauncherOverlay;
    use crate::i18n::Locale;
    use crate::model::Highlight;
    use crate::model::tests::library;
    use crate::pins::{Recent, StartStore};
    use crate::ranking::NoPaths;
    use app_catalog_platform::{ApplicationDirectories, SessionEnvironment};
    use better_ui::icons::IconSearch;

    /// Nothing from the person's machine: application folders that do not
    /// exist, no icon themes, no stored layout or appearance, no filesystem
    /// for path queries, and no watcher.
    fn scratch_sources() -> super::OverlaySources {
        let nowhere = std::env::temp_dir().join(format!(
            "better-launcher-key-tests-{}-nonexistent",
            std::process::id()
        ));
        super::OverlaySources {
            session: SessionEnvironment {
                directories: ApplicationDirectories::from_values(
                    Some(&nowhere.join("home")),
                    None,
                    Some(nowhere.join("system").to_str().unwrap()),
                ),
                ..SessionEnvironment::default()
            },
            store: StartStore::default(),
            appearance: None,
            icons: std::sync::Arc::new(|| IconSearch::new(Vec::new(), Vec::new())),
            paths: std::sync::Arc::new(NoPaths),
            watch: false,
        }
    }

    #[test]
    fn the_key_tests_read_nothing_from_the_machine() {
        let sources = scratch_sources();
        assert!(sources.store.pinned.is_none() && sources.store.recent.is_none());
        assert!(sources.appearance.is_none());
        assert!(!sources.watch);
        assert!((sources.icons)().data_dirs.is_empty());
        assert!(
            sources
                .session
                .directories
                .directories()
                .iter()
                .all(|dir| !dir.path.exists())
        );
    }

    #[derive(Debug, Default)]
    struct Observed {
        highlights: Vec<(String, Option<Highlight>)>,
        query: String,
        results: Vec<String>,
    }

    /// Opens the launcher over the test library, presses each keystroke in
    /// turn, and records the highlight after each.
    fn press(keys: &'static [&'static str], typed: &'static [&'static str]) -> Observed {
        let observed = Rc::new(RefCell::new(Observed::default()));
        let out = observed.clone();
        gpui_platform::headless().run(move |cx| {
            gpui_component::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(640.0), px(720.0)),
                ))),
                ..WindowOptions::default()
            };
            let slot: Rc<RefCell<Option<Entity<LauncherOverlay>>>> = Rc::default();
            let slot_in_window = slot.clone();
            let window = cx
                .open_window(options, move |window, cx| {
                    let overlay = cx.new(|cx| {
                        LauncherOverlay::with_sources(Locale::EnUs, scratch_sources(), window, cx)
                    });
                    *slot_in_window.borrow_mut() = Some(overlay.clone());
                    cx.new(|cx| Root::new(overlay, window, cx))
                })
                .expect("a headless window opens");
            let overlay = slot.borrow_mut().take().expect("the overlay was built");

            cx.spawn(async move |cx| {
                cx.update(|cx| {
                    overlay.update(cx, |overlay, cx| {
                        overlay.adopt(library(), cx);
                        overlay.set_start(None, Recent::default());
                    })
                });
                let window: AnyWindowHandle = window.into();
                let step = |key: &str, cx: &mut AsyncApp| {
                    cx.update_window(window, |_, window, cx| {
                        window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                    })
                    .expect("the window is still open");
                };
                for key in keys {
                    step(key, cx);
                    let highlight = cx.update(|cx| overlay.read(cx).model().highlight());
                    out.borrow_mut()
                        .highlights
                        .push((key.to_string(), highlight));
                }
                for key in typed {
                    step(key, cx);
                }
                cx.update(|cx| {
                    let overlay = overlay.read(cx);
                    let mut out = out.borrow_mut();
                    out.query = overlay.search.read(cx).value().to_string();
                    out.results = overlay
                        .model()
                        .rows()
                        .iter()
                        .map(|application| application.display_name.clone())
                        .collect();
                    cx.quit();
                });
            })
            .detach();
        });
        Rc::try_unwrap(observed)
            .expect("the application has released the observations")
            .into_inner()
    }

    #[test]
    fn every_navigation_key_reaches_the_start_page_through_the_focused_search_box() {
        // Pinned: Browser, Files, Text Editor, Terminal, Calculator, Calendar.
        let observed = press(
            &[
                "down", "right", "right", "left", "end", "home", "up", "down",
            ],
            &[],
        );
        let expected = [
            ("down", Some(Highlight::Pinned(0))),
            ("right", Some(Highlight::Pinned(1))),
            ("right", Some(Highlight::Pinned(2))),
            ("left", Some(Highlight::Pinned(1))),
            ("end", Some(Highlight::Pinned(5))),
            ("home", Some(Highlight::Pinned(0))),
            ("up", Some(Highlight::AllAppsButton)),
            ("down", Some(Highlight::Pinned(0))),
        ]
        .map(|(key, highlight)| (key.to_string(), highlight));
        assert_eq!(observed.highlights, expected);
    }

    #[test]
    fn the_search_box_still_takes_typing_and_its_own_cursor_keys() {
        // With nothing highlighted, Left and Right move the text cursor, so
        // a letter typed after Left lands before the last one.
        let observed = press(&[], &["c", "a", "l", "left", "x"]);
        assert_eq!(observed.query, "caxl");
        let observed = press(&[], &["c", "a", "l", "shift-left", "x"]);
        assert_eq!(observed.query, "cax");
        let observed = press(&[], &["c", "a", "l", "backspace", "l"]);
        assert_eq!(observed.query, "cal");
        assert_eq!(observed.results, vec!["Calendar", "Calculator"]);
    }

    #[test]
    fn up_and_down_walk_search_results_while_typing_continues() {
        let observed = press(&["c", "a", "l", "down", "down", "up"], &["e"]);
        let highlights: Vec<Option<Highlight>> = observed
            .highlights
            .iter()
            .skip(3)
            .map(|(_, h)| *h)
            .collect();
        assert_eq!(
            highlights,
            [
                Some(Highlight::Result(1)),
                Some(Highlight::Result(1)),
                Some(Highlight::Result(0))
            ]
        );
        assert_eq!(observed.query, "cale");
        assert_eq!(observed.results, vec!["Calendar"]);
    }
}

#[cfg(test)]
mod restart_tests {
    use super::LauncherView;

    #[test]
    fn what_was_typed_and_the_page_come_back() {
        let view = LauncherView {
            query: "firef".into(),
            all_apps: true,
        };
        let text = serde_json::to_string(&view).unwrap();
        let read: LauncherView = serde_json::from_str(&text).unwrap();
        assert_eq!(read, view);
        let older: LauncherView = serde_json::from_str("{}").unwrap();
        assert_eq!(older, LauncherView::default());
    }
}

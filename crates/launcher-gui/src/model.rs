//! What the launcher is showing, with no GPUI in it.
//!
//! Better Launcher is Windows 11's Start menu. With nothing typed it shows
//! the Start page — the pinned apps in a six-column grid with an "All apps"
//! button above them, and Recommended, the apps opened most recently — or,
//! after "All apps", every app from A to Z under letter headers, where a
//! header opens a grid of letters to jump to. Typing anything replaces
//! either with search results, and clearing the search brings back the page
//! that was there, so browsing and searching are still one screen driven by
//! the query (Issue #2), not modes to get out of.
//!
//! Everything a key, a click, or a menu does is decided here and tested
//! here: which item the keyboard is on and where each arrow takes it, what
//! Enter does, what the context menu offers for each item, pinning and
//! reordering, and the launch outcome. The overlay draws this model and
//! performs what it asks for; it decides nothing.

use app_catalog_core::{ApplicationRecord, DesktopId};
use launcher_core::{FoldedText, LauncherApplication, LauncherState};
use launcher_platform::{
    ApplicationStarter, LauncherSnapshot, ManagedApplications, ManagedComponent,
};

use crate::pins::{Pins, RECOMMENDED, Recent, RecentLaunch, default_pins};
use crate::ranking::{NoPaths, PathHit, PathProbe, SearchItem, insert_paths, query_path, search};

/// The pinned grid's width, as Windows 11's Start menu lays it out.
pub const PINNED_COLUMNS: usize = 6;
/// Recommended's width.
pub const RECOMMENDED_COLUMNS: usize = 2;
/// The letter grid's width.
pub const JUMP_COLUMNS: usize = 7;
/// How far Page Up and Page Down move in a list.
pub const PAGE: usize = 10;

/// Whether the application list has been read yet, and whether it is being
/// re-read.
///
/// `Refreshing` is deliberately not `Loading`: an index rebuild after an
/// application was installed must not blank a screen the user is already using.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LoadState {
    #[default]
    Loading,
    Ready,
    Refreshing,
}

/// Something the overlay has to tell the user about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// The launch failed, carrying the platform's stable machine key. The
    /// wording is the locale's job; the key is what makes it diagnosable.
    LaunchFailed(String),
    /// A calculation was copied.
    Copied(String),
    /// Pins or history could not be saved; the change holds for this
    /// opening only.
    NotSaved(String),
}

/// The page shown while nothing is typed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Page {
    #[default]
    Home,
    AllApps,
}

/// What the screen is, as the query and the page make it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    Home,
    AllApps,
    Search,
}

/// Which section of All apps a name sits under.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GroupKey {
    /// Names starting with a digit or a symbol: `#`.
    Symbol,
    /// A Latin letter, after case and accents are folded away.
    Letter(char),
    /// Every other script, such as a Chinese name: after Z.
    Other,
}

impl GroupKey {
    /// The section a name belongs to.
    pub fn of(name: &str) -> GroupKey {
        let folded = FoldedText::new(name.trim());
        match folded.chars().first() {
            Some(c) if c.is_ascii_lowercase() => GroupKey::Letter(c.to_ascii_uppercase()),
            Some(c) if c.is_ascii() => GroupKey::Symbol,
            Some(_) => GroupKey::Other,
            None => GroupKey::Symbol,
        }
    }

    /// Every key the letter grid shows, in order.
    pub fn all() -> Vec<GroupKey> {
        let mut keys = vec![GroupKey::Symbol];
        keys.extend(('A'..='Z').map(GroupKey::Letter));
        keys.push(GroupKey::Other);
        keys
    }
}

/// One line of All apps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllAppsRow {
    Header(GroupKey),
    /// An index into the library ([`LauncherSnapshot`]'s browse list).
    App(usize),
}

/// The item the keyboard is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Highlight {
    Pinned(usize),
    AllAppsButton,
    Recommended(usize),
    /// An index into [`OverlayModel::all_apps_rows`].
    Row(usize),
    /// An index into [`OverlayModel::results`].
    Result(usize),
}

/// A key the launcher handles, before the search box sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Escape,
    /// Shift+F10 or the Menu key: the highlighted item's context menu.
    Menu,
}

/// Reads a key by GPUI's key name. Shift with Left, Right, Home, and End
/// selects text, so those stay with the search box.
pub fn nav_key(key: &str, shift: bool) -> Option<NavKey> {
    Some(match (key, shift) {
        ("up", _) => NavKey::Up,
        ("down", _) => NavKey::Down,
        ("left", false) => NavKey::Left,
        ("right", false) => NavKey::Right,
        ("home", false) => NavKey::Home,
        ("end", false) => NavKey::End,
        ("pageup", _) => NavKey::PageUp,
        ("pagedown", _) => NavKey::PageDown,
        ("enter", _) => NavKey::Enter,
        ("escape", _) => NavKey::Escape,
        ("f10", true) | ("menu", _) | ("contextmenu", _) => NavKey::Menu,
        _ => return None,
    })
}

/// What an item is, for activating it and for its menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    App(DesktopId),
    Calculation(String),
    Path(PathHit),
    AllApps,
    /// A letter header: open the letter grid.
    Letters,
}

/// Where an item's context menu was opened, which decides some entries:
/// "Move to front" belongs to a pinned app, "Remove from list" to a
/// recommended one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Place {
    Pinned,
    Recommended,
    AllApps,
    Search,
}

/// One entry of a context menu, or of the actions beside a search result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Open,
    /// "Open" for an app whose desktop entry asks for a terminal: it runs
    /// in one, and the menu says so.
    RunInTerminal,
    PinToStart,
    UnpinFromStart,
    MoveToFront,
    RemoveFromList,
    OpenFileLocation,
    /// Removal happens in Better Manager; this opens it on the component's
    /// own page.
    Uninstall(ManagedComponent),
    CopyResult,
    OpenPath,
    OpenContainingFolder,
}

/// What the overlay has to do after a key or a click.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Nothing: the key goes on to the search box.
    Pass,
    /// Handled; redraw.
    Redraw,
    /// Close the launcher.
    Close,
    /// Do this to this item.
    Run(Target, Command),
    /// Open the context menu for the highlighted item.
    Menu(Target, Place),
}

/// What activating an app did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Activation {
    /// Started. The overlay has done its job and should close.
    Launched(DesktopId),
    /// Nothing was selected, so nothing happened and the overlay stays.
    NothingSelected,
    /// It failed. The overlay stays open, showing why.
    Failed(String),
}

/// One recommended app.
#[derive(Clone, Debug)]
pub struct RecommendedApp<'a> {
    pub application: &'a LauncherApplication,
    pub launch: &'a RecentLaunch,
}

/// The launcher's state.
#[derive(Debug, Default)]
pub struct OverlayModel {
    snapshot: Option<LauncherSnapshot>,
    managed: ManagedApplications,
    query: LauncherState,
    results: Vec<SearchItem>,
    rows: Vec<AllAppsRow>,
    load: LoadState,
    page: Page,
    highlight: Option<Highlight>,
    /// The letter grid, open, with the letter the keyboard is on.
    jump: Option<usize>,
    /// Set when the keyboard moved, so the overlay scrolls the item into
    /// view once.
    scroll: bool,
    /// Counts query changes, so files and folders found for an older query
    /// are dropped when they arrive.
    path_generation: u64,
    pins: Option<Pins>,
    recent: Recent,
    notice: Option<Notice>,
}

impl OverlayModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_state(&self) -> LoadState {
        self.load
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn set_notice(&mut self, notice: Option<Notice>) {
        self.notice = notice;
    }

    pub fn query(&self) -> &str {
        self.query.query()
    }

    /// Whether nothing is typed. A question about the query, not a mode.
    pub fn is_browsing(&self) -> bool {
        self.query.is_browsing()
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn screen(&self) -> Screen {
        match (self.is_browsing(), self.page) {
            (false, _) => Screen::Search,
            (true, Page::Home) => Screen::Home,
            (true, Page::AllApps) => Screen::AllApps,
        }
    }

    pub fn highlight(&self) -> Option<Highlight> {
        self.highlight
    }

    pub fn jump(&self) -> Option<usize> {
        self.jump
    }

    /// Whether the keyboard moved since the last call: scroll the
    /// highlighted item into view.
    pub fn take_scroll(&mut self) -> bool {
        std::mem::take(&mut self.scroll)
    }

    /// The whole library, by name.
    pub fn library(&self) -> &[LauncherApplication] {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.index.browse().applications())
            .unwrap_or(&[])
    }

    /// The applications on screen: search results while searching, the
    /// library otherwise. What the benchmark harness counts.
    pub fn rows(&self) -> Vec<&LauncherApplication> {
        if self.is_browsing() {
            self.library().iter().collect()
        } else {
            self.results
                .iter()
                .filter_map(SearchItem::application)
                .collect()
        }
    }

    pub fn results(&self) -> &[SearchItem] {
        &self.results
    }

    pub fn all_apps_rows(&self) -> &[AllAppsRow] {
        &self.rows
    }

    pub fn application(&self, id: &DesktopId) -> Option<&LauncherApplication> {
        self.library()
            .iter()
            .find(|application| &application.desktop_id == id)
    }

    pub fn record(&self, id: &DesktopId) -> Option<&ApplicationRecord> {
        self.snapshot.as_ref()?.catalog.get(id)
    }

    pub fn managed(&self) -> &ManagedApplications {
        &self.managed
    }

    pub fn set_managed(&mut self, managed: ManagedApplications) {
        self.managed = managed;
    }

    /// Whether a typed query matched nothing.
    pub fn is_empty_result(&self) -> bool {
        !self.is_browsing() && self.load != LoadState::Loading && self.results.is_empty()
    }

    /// Whether the machine has no applications to show at all.
    pub fn is_empty_library(&self) -> bool {
        self.is_browsing() && self.load != LoadState::Loading && self.library().is_empty()
    }

    // ---------------------------------------------------------------- data

    /// The application list has been read. Keeps the query and the
    /// highlighted app when it survived, and clears a stale failure.
    pub fn apply_snapshot(&mut self, snapshot: LauncherSnapshot) {
        let kept = self.highlighted_app();
        self.snapshot = Some(snapshot);
        self.load = LoadState::Ready;
        if matches!(self.notice, Some(Notice::LaunchFailed(_))) {
            self.notice = None;
        }
        self.rows = build_rows(self.library());
        if self.pins.is_none() {
            // No stored arrangement has been given: start from the
            // categories. The overlay replaces this with the stored one.
            self.pins = Some(default_pins(self.library()));
        }
        self.recompute(&NoPaths);
        self.restore(kept);
    }

    /// The application directories changed and a re-read is under way. The
    /// current rows stay on screen while it runs.
    pub fn begin_refresh(&mut self) {
        if self.load == LoadState::Ready {
            self.load = LoadState::Refreshing;
        }
    }

    /// The stored arrangement. `None` pins means never arranged: the
    /// starting set is chosen from the library by category.
    pub fn set_start(&mut self, pins: Option<Pins>, recent: Recent) {
        self.pins = pins.or_else(|| self.snapshot.as_ref().map(|_| default_pins(self.library())));
        self.recent = recent;
        self.clamp_highlight();
    }

    pub fn pins(&self) -> Pins {
        self.pins.clone().unwrap_or_default()
    }

    pub fn recent(&self) -> &Recent {
        &self.recent
    }

    /// The pinned apps that are installed, in the person's order. A pin
    /// whose app is gone is kept, in case it comes back, and not drawn.
    pub fn pinned(&self) -> Vec<&LauncherApplication> {
        let Some(pins) = &self.pins else {
            return Vec::new();
        };
        pins.ids()
            .iter()
            .filter_map(|id| self.application(id))
            .collect()
    }

    pub fn recommended(&self) -> Vec<RecommendedApp<'_>> {
        self.recent
            .entries()
            .iter()
            .filter_map(|launch| {
                self.application(&launch.id)
                    .map(|application| RecommendedApp {
                        application,
                        launch,
                    })
            })
            .take(RECOMMENDED)
            .collect()
    }

    pub fn is_pinned(&self, id: &DesktopId) -> bool {
        self.pins.as_ref().is_some_and(|pins| pins.contains(id))
    }

    pub fn pin(&mut self, id: &DesktopId) -> bool {
        self.pins.get_or_insert_with(Pins::default).pin(id.clone())
    }

    pub fn unpin(&mut self, id: &DesktopId) -> bool {
        let changed = self.pins.as_mut().is_some_and(|pins| pins.unpin(id));
        self.clamp_highlight();
        changed
    }

    pub fn move_to_front(&mut self, id: &DesktopId) -> bool {
        self.pins
            .as_mut()
            .is_some_and(|pins| pins.move_to_front(id))
    }

    /// A pinned tile dragged from one drawn position onto another.
    pub fn move_pinned(&mut self, from: usize, to: usize) -> bool {
        let pinned: Vec<DesktopId> = self
            .pinned()
            .into_iter()
            .map(|application| application.desktop_id.clone())
            .collect();
        match (pinned.get(from), pinned.get(to)) {
            (Some(from), Some(to)) => self
                .pins
                .as_mut()
                .is_some_and(|pins| pins.move_pin(from, to)),
            _ => false,
        }
    }

    pub fn forget(&mut self, id: &DesktopId) -> bool {
        let changed = self.recent.forget(id);
        self.clamp_highlight();
        changed
    }

    // --------------------------------------------------------------- query

    /// A new query. Applications and a calculation are answered now; files
    /// and folders are not read here, on the render thread, but asked for
    /// through [`OverlayModel::path_request`] and added by
    /// [`OverlayModel::apply_path_hits`] when a background read returns.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.set_query_with(query, &NoPaths);
    }

    /// The same, with the filesystem a path query reads given.
    pub fn set_query_with(&mut self, query: impl Into<String>, probe: &dyn PathProbe) {
        let was_browsing = self.is_browsing();
        self.query.set_query(query);
        self.path_generation = self.path_generation.wrapping_add(1);
        if matches!(
            self.notice,
            Some(Notice::LaunchFailed(_) | Notice::Copied(_))
        ) {
            self.notice = None;
        }
        self.jump = None;
        self.recompute(probe);
        // The best match is where Enter goes, as Windows 11 has it.
        self.highlight = if self.is_browsing() {
            if was_browsing { self.highlight } else { None }
        } else if self.results.is_empty() {
            None
        } else {
            Some(Highlight::Result(0))
        };
        self.clamp_highlight();
    }

    /// Emptying the search box: back to the page that was showing.
    pub fn clear_query(&mut self) {
        self.set_query("");
    }

    /// The files-and-folders read the current query needs, if it is a path
    /// (it begins with `/` or `~`): its generation and the query. `home` is
    /// what `~` stands for.
    pub fn path_request(&self, home: Option<std::path::PathBuf>) -> Option<(u64, String)> {
        struct Home(Option<std::path::PathBuf>);
        impl PathProbe for Home {
            fn kind(&self, _: &std::path::Path) -> Option<crate::ranking::PathKind> {
                None
            }
            fn children(
                &self,
                _: &std::path::Path,
                _: usize,
            ) -> Vec<(String, crate::ranking::PathKind)> {
                Vec::new()
            }
            fn home(&self) -> Option<std::path::PathBuf> {
                self.0.clone()
            }
        }
        query_path(self.query.query(), &Home(home))
            .map(|_| (self.path_generation, self.query.query().to_string()))
    }

    /// Files and folders a background read found. Dropped when the query has
    /// changed since it was asked for; otherwise they go after a calculation
    /// and before the applications. Returns whether they were used.
    pub fn apply_path_hits(&mut self, generation: u64, hits: Vec<PathHit>) -> bool {
        if generation != self.path_generation || self.is_browsing() {
            return false;
        }
        insert_paths(&mut self.results, hits);
        if self.highlight.is_none() && !self.results.is_empty() {
            self.highlight = Some(Highlight::Result(0));
        }
        self.clamp_highlight();
        true
    }

    fn recompute(&mut self, probe: &dyn PathProbe) {
        self.results = match &self.snapshot {
            Some(snapshot) if !self.is_browsing() => {
                search(self.query.query(), &snapshot.index, probe)
            }
            _ => Vec::new(),
        };
    }

    // ---------------------------------------------------------- navigation

    pub fn open_all_apps(&mut self) {
        self.page = Page::AllApps;
        self.highlight = None;
        self.jump = None;
    }

    pub fn go_home(&mut self) {
        self.page = Page::Home;
        self.highlight = None;
        self.jump = None;
    }

    /// Opens the letter grid on the section the keyboard is in.
    pub fn open_letters(&mut self) {
        let keys = GroupKey::all();
        let current = match self.highlight {
            Some(Highlight::Row(row)) => self.group_of_row(row),
            _ => None,
        };
        self.jump = Some(
            current
                .and_then(|key| keys.iter().position(|k| *k == key))
                .or_else(|| (0..keys.len()).find(|&i| self.has_group(keys[i])))
                .unwrap_or(0),
        );
    }

    pub fn close_letters(&mut self) {
        self.jump = None;
    }

    /// Whether All apps has a section for this key: the letter grid draws
    /// the others disabled, as Windows does.
    pub fn has_group(&self, key: GroupKey) -> bool {
        self.rows.contains(&AllAppsRow::Header(key))
    }

    /// Jumps to a section: closes the letter grid and puts the keyboard on
    /// its header, scrolled into view.
    pub fn jump_to(&mut self, key: GroupKey) {
        self.jump = None;
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| *row == AllAppsRow::Header(key))
        {
            self.page = Page::AllApps;
            self.highlight = Some(Highlight::Row(row));
            self.scroll = true;
        }
    }

    fn group_of_row(&self, row: usize) -> Option<GroupKey> {
        if self.rows.is_empty() {
            return None;
        }
        self.rows[..=row.min(self.rows.len() - 1)]
            .iter()
            .rev()
            .find_map(|row| match row {
                AllAppsRow::Header(key) => Some(*key),
                AllAppsRow::App(_) => None,
            })
    }

    /// Puts the keyboard on an item, as a pointer does before a menu opens.
    pub fn set_highlight(&mut self, highlight: Option<Highlight>) {
        self.highlight = highlight;
        self.clamp_highlight();
    }

    fn pinned_len(&self) -> usize {
        self.pinned().len()
    }

    fn recommended_len(&self) -> usize {
        self.recommended().len()
    }

    /// Drops a highlight whose item is gone, or moves it to the last one.
    fn clamp_highlight(&mut self) {
        self.highlight = match self.highlight {
            Some(Highlight::Pinned(i)) => {
                let len = self.pinned_len();
                (len > 0).then(|| Highlight::Pinned(i.min(len - 1)))
            }
            Some(Highlight::Recommended(i)) => {
                let len = self.recommended_len();
                (len > 0).then(|| Highlight::Recommended(i.min(len - 1)))
            }
            Some(Highlight::Row(i)) => {
                (!self.rows.is_empty()).then(|| Highlight::Row(i.min(self.rows.len() - 1)))
            }
            Some(Highlight::Result(i)) => {
                (!self.results.is_empty()).then(|| Highlight::Result(i.min(self.results.len() - 1)))
            }
            other => other,
        };
    }

    fn highlighted_app(&self) -> Option<DesktopId> {
        match self.target(self.highlight?)? {
            Target::App(id) => Some(id),
            _ => None,
        }
    }

    /// After the list changed under it, puts the keyboard back on the same
    /// app if it is still there.
    fn restore(&mut self, kept: Option<DesktopId>) {
        let Some(kept) = kept else {
            self.clamp_highlight();
            return;
        };
        let found = match self.highlight {
            Some(Highlight::Result(_)) => self
                .results
                .iter()
                .position(|item| item.application().is_some_and(|a| a.desktop_id == kept))
                .map(Highlight::Result),
            Some(Highlight::Pinned(_)) => self
                .pinned()
                .iter()
                .position(|a| a.desktop_id == kept)
                .map(Highlight::Pinned),
            Some(Highlight::Recommended(_)) => self
                .recommended()
                .iter()
                .position(|r| r.application.desktop_id == kept)
                .map(Highlight::Recommended),
            Some(Highlight::Row(_)) => {
                let library = self.library();
                self.rows
                    .iter()
                    .position(
                        |row| matches!(row, AllAppsRow::App(i) if library[*i].desktop_id == kept),
                    )
                    .map(Highlight::Row)
            }
            other => other,
        };
        self.highlight = found;
        self.clamp_highlight();
    }

    /// What an item is.
    pub fn target(&self, highlight: Highlight) -> Option<Target> {
        Some(match highlight {
            Highlight::Pinned(i) => Target::App(self.pinned().get(i)?.desktop_id.clone()),
            Highlight::Recommended(i) => {
                Target::App(self.recommended().get(i)?.application.desktop_id.clone())
            }
            Highlight::AllAppsButton => Target::AllApps,
            Highlight::Row(i) => match self.rows.get(i)? {
                AllAppsRow::Header(_) => Target::Letters,
                AllAppsRow::App(app) => Target::App(self.library().get(*app)?.desktop_id.clone()),
            },
            Highlight::Result(i) => match self.results.get(i)? {
                SearchItem::App(application) => Target::App(application.desktop_id.clone()),
                SearchItem::Calculation(calculation) => {
                    Target::Calculation(calculation.display.clone())
                }
                SearchItem::Path(hit) => Target::Path(hit.clone()),
            },
        })
    }

    /// Where an item sits, for its menu.
    pub fn place(&self, highlight: Highlight) -> Place {
        match highlight {
            Highlight::Pinned(_) | Highlight::AllAppsButton => Place::Pinned,
            Highlight::Recommended(_) => Place::Recommended,
            Highlight::Row(_) => Place::AllApps,
            Highlight::Result(_) => Place::Search,
        }
    }

    /// The default action for an item: what Enter and a click do.
    pub fn primary(&self, target: &Target) -> Command {
        match target {
            Target::App(id) if self.runs_in_terminal(id) => Command::RunInTerminal,
            Target::App(_) | Target::AllApps | Target::Letters => Command::Open,
            Target::Calculation(_) => Command::CopyResult,
            Target::Path(_) => Command::OpenPath,
        }
    }

    pub fn runs_in_terminal(&self, id: &DesktopId) -> bool {
        self.record(id)
            .is_some_and(|record| record.capabilities.terminal)
    }

    /// Everything a context menu offers for an item, in order.
    pub fn commands(&self, target: &Target, place: Place) -> Vec<Command> {
        match target {
            Target::App(id) => {
                let mut commands = vec![self.primary(target)];
                if self.is_pinned(id) {
                    if place == Place::Pinned && self.pins().position(id) != Some(0) {
                        commands.push(Command::MoveToFront);
                    }
                    commands.push(Command::UnpinFromStart);
                } else {
                    commands.push(Command::PinToStart);
                }
                if place == Place::Recommended {
                    commands.push(Command::RemoveFromList);
                }
                if self.record(id).is_some() {
                    commands.push(Command::OpenFileLocation);
                }
                if let Some(component) = self.managed.component_for(id)
                    && self.managed.manager_entry().is_some()
                {
                    commands.push(Command::Uninstall(component.clone()));
                }
                commands
            }
            Target::Calculation(_) => vec![Command::CopyResult],
            Target::Path(_) => vec![Command::OpenPath, Command::OpenContainingFolder],
            Target::AllApps | Target::Letters => Vec::new(),
        }
    }

    /// Applies a key. [`Effect::Pass`] leaves it to the search box.
    pub fn handle_key(&mut self, key: NavKey) -> Effect {
        if let Some(at) = self.jump {
            return self.letters_key(at, key);
        }
        match key {
            NavKey::Escape => {
                return match self.screen() {
                    Screen::AllApps => {
                        self.go_home();
                        Effect::Redraw
                    }
                    Screen::Home | Screen::Search => Effect::Close,
                };
            }
            NavKey::Enter => {
                let highlight = match (self.highlight, self.screen()) {
                    (Some(highlight), _) => highlight,
                    (None, Screen::Search) if !self.results.is_empty() => Highlight::Result(0),
                    _ => return Effect::Redraw,
                };
                return match self.target(highlight) {
                    Some(Target::AllApps) => {
                        self.open_all_apps();
                        Effect::Redraw
                    }
                    Some(Target::Letters) => {
                        self.open_letters();
                        Effect::Redraw
                    }
                    Some(target) => {
                        let command = self.primary(&target);
                        Effect::Run(target, command)
                    }
                    None => Effect::Redraw,
                };
            }
            NavKey::Menu => {
                let Some(highlight) = self.highlight else {
                    return Effect::Redraw;
                };
                let place = self.place(highlight);
                return match self.target(highlight) {
                    Some(target) if !self.commands(&target, place).is_empty() => {
                        Effect::Menu(target, place)
                    }
                    _ => Effect::Redraw,
                };
            }
            _ => {}
        }
        let next = match self.screen() {
            Screen::Home => self.home_move(key),
            Screen::AllApps => self.list_move(key, self.rows.len(), Highlight::Row, |h| match h {
                Highlight::Row(i) => Some(i),
                _ => None,
            }),
            Screen::Search => {
                self.list_move(key, self.results.len(), Highlight::Result, |h| match h {
                    Highlight::Result(i) => Some(i),
                    _ => None,
                })
            }
        };
        match next {
            Move::Pass => Effect::Pass,
            Move::Stay => Effect::Redraw,
            Move::To(highlight) => {
                self.highlight = highlight;
                self.scroll = true;
                Effect::Redraw
            }
        }
    }

    /// Arrow keys on the Start page: a six-column grid of pins with the "All
    /// apps" button above it and the two-column Recommended below it. Rows
    /// do not wrap; with nothing highlighted, Left and Right belong to the
    /// search box.
    fn home_move(&self, key: NavKey) -> Move {
        let pinned = self.pinned_len();
        let recommended = self.recommended_len();
        let first = if pinned > 0 {
            Highlight::Pinned(0)
        } else if recommended > 0 {
            Highlight::Recommended(0)
        } else {
            Highlight::AllAppsButton
        };
        let last_row_start = |len: usize, columns: usize| ((len - 1) / columns) * columns;
        match (self.highlight, key) {
            (None, NavKey::Down) => Move::To(Some(first)),
            (None, _) => Move::Pass,
            (Some(Highlight::AllAppsButton), NavKey::Down) if first != Highlight::AllAppsButton => {
                Move::To(Some(first))
            }
            (Some(Highlight::AllAppsButton), NavKey::Up) => Move::To(None),
            (Some(Highlight::AllAppsButton), _) => Move::Stay,
            (Some(Highlight::Pinned(i)), key) => {
                let column = i % PINNED_COLUMNS;
                match key {
                    NavKey::Left if column > 0 => Move::To(Some(Highlight::Pinned(i - 1))),
                    NavKey::Right if column + 1 < PINNED_COLUMNS && i + 1 < pinned => {
                        Move::To(Some(Highlight::Pinned(i + 1)))
                    }
                    NavKey::Up if i >= PINNED_COLUMNS => {
                        Move::To(Some(Highlight::Pinned(i - PINNED_COLUMNS)))
                    }
                    NavKey::Up => Move::To(Some(Highlight::AllAppsButton)),
                    NavKey::Down if i + PINNED_COLUMNS < pinned => {
                        Move::To(Some(Highlight::Pinned(i + PINNED_COLUMNS)))
                    }
                    NavKey::Down if i < last_row_start(pinned, PINNED_COLUMNS) => {
                        // A short last row: drop to its last tile.
                        Move::To(Some(Highlight::Pinned(pinned - 1)))
                    }
                    NavKey::Down if recommended > 0 => {
                        let target = if column < PINNED_COLUMNS / 2 { 0 } else { 1 };
                        Move::To(Some(Highlight::Recommended(target.min(recommended - 1))))
                    }
                    NavKey::Home => Move::To(Some(Highlight::Pinned(0))),
                    NavKey::End => Move::To(Some(Highlight::Pinned(pinned - 1))),
                    _ => Move::Stay,
                }
            }
            (Some(Highlight::Recommended(i)), key) => {
                let column = i % RECOMMENDED_COLUMNS;
                match key {
                    NavKey::Left if column > 0 => Move::To(Some(Highlight::Recommended(i - 1))),
                    NavKey::Right if column + 1 < RECOMMENDED_COLUMNS && i + 1 < recommended => {
                        Move::To(Some(Highlight::Recommended(i + 1)))
                    }
                    NavKey::Down if i + RECOMMENDED_COLUMNS < recommended => {
                        Move::To(Some(Highlight::Recommended(i + RECOMMENDED_COLUMNS)))
                    }
                    NavKey::Up if i >= RECOMMENDED_COLUMNS => {
                        Move::To(Some(Highlight::Recommended(i - RECOMMENDED_COLUMNS)))
                    }
                    NavKey::Up if pinned > 0 => {
                        let start = last_row_start(pinned, PINNED_COLUMNS);
                        let wanted = start + column * (PINNED_COLUMNS / 2);
                        Move::To(Some(Highlight::Pinned(wanted.min(pinned - 1))))
                    }
                    NavKey::Up => Move::To(Some(Highlight::AllAppsButton)),
                    _ => Move::Stay,
                }
            }
            _ => Move::Pass,
        }
    }

    /// Arrow keys in a list: All apps or search results. Up from the top of
    /// All apps goes back to the search box; Left and Right stay with it.
    fn list_move(
        &self,
        key: NavKey,
        len: usize,
        make: fn(usize) -> Highlight,
        index: impl Fn(Highlight) -> Option<usize>,
    ) -> Move {
        if len == 0 {
            return match key {
                NavKey::Left | NavKey::Right | NavKey::Home | NavKey::End => Move::Pass,
                _ => Move::Stay,
            };
        }
        let current = self.highlight.and_then(index);
        let last = len - 1;
        match (current, key) {
            (_, NavKey::Left | NavKey::Right) => Move::Pass,
            (None, NavKey::Home | NavKey::End) => Move::Pass,
            (None, NavKey::Down | NavKey::PageDown) => Move::To(Some(make(0))),
            (None, _) => Move::Stay,
            (Some(0), NavKey::Up) if self.screen() == Screen::AllApps => Move::To(None),
            (Some(i), NavKey::Up) => Move::To(Some(make(i.saturating_sub(1)))),
            (Some(i), NavKey::Down) => Move::To(Some(make((i + 1).min(last)))),
            (Some(i), NavKey::PageUp) => Move::To(Some(make(i.saturating_sub(PAGE)))),
            (Some(i), NavKey::PageDown) => Move::To(Some(make((i + PAGE).min(last)))),
            (Some(_), NavKey::Home) => Move::To(Some(make(0))),
            (Some(_), NavKey::End) => Move::To(Some(make(last))),
            _ => Move::Stay,
        }
    }

    /// Keys in the letter grid: arrows move over the letters that have a
    /// section, Enter jumps, Escape closes it.
    fn letters_key(&mut self, at: usize, key: NavKey) -> Effect {
        let keys = GroupKey::all();
        let enabled: Vec<bool> = keys.iter().map(|k| self.has_group(*k)).collect();
        let step = |from: usize, delta: isize| -> Option<usize> {
            let mut i = from as isize + delta;
            while i >= 0 && (i as usize) < keys.len() {
                if enabled[i as usize] {
                    return Some(i as usize);
                }
                if delta.abs() != 1 {
                    break;
                }
                i += delta;
            }
            None
        };
        let next = match key {
            NavKey::Escape => {
                self.jump = None;
                return Effect::Redraw;
            }
            NavKey::Enter => {
                self.jump_to(keys[at]);
                return Effect::Redraw;
            }
            NavKey::Left => step(at, -1),
            NavKey::Right => step(at, 1),
            NavKey::Up => step(at, -(JUMP_COLUMNS as isize)).or_else(|| step(at, -1)),
            NavKey::Down => step(at, JUMP_COLUMNS as isize).or_else(|| step(at, 1)),
            NavKey::Home => (0..keys.len()).find(|&i| enabled[i]),
            NavKey::End => (0..keys.len()).rev().find(|&i| enabled[i]),
            _ => None,
        };
        if let Some(next) = next {
            self.jump = Some(next);
        }
        Effect::Redraw
    }

    // -------------------------------------------------------------- launch

    /// Launches an app through the shared platform path and remembers it for
    /// Recommended.
    ///
    /// A failure is recorded and reported rather than returned and forgotten:
    /// the overlay stays open showing what went wrong, because closing on a
    /// failed launch looks exactly like a successful one.
    pub fn launch(
        &mut self,
        id: &DesktopId,
        starter: &dyn ApplicationStarter,
        at: u64,
    ) -> Activation {
        match starter.start(id) {
            Ok(_) => {
                self.notice = None;
                self.recent.record(id, at);
                Activation::Launched(id.clone())
            }
            Err(error) => {
                let key = error.to_string();
                self.notice = Some(Notice::LaunchFailed(key.clone()));
                Activation::Failed(key)
            }
        }
    }

    /// Launches what the keyboard is on, or the best match.
    pub fn activate(&mut self, starter: &dyn ApplicationStarter, at: u64) -> Activation {
        let highlight = self
            .highlight
            .or((!self.results.is_empty() && !self.is_browsing()).then_some(Highlight::Result(0)));
        match highlight.and_then(|h| self.target(h)) {
            Some(Target::App(id)) => self.launch(&id, starter, at),
            _ => Activation::NothingSelected,
        }
    }
}

enum Move {
    /// Not the launcher's key here: the search box gets it.
    Pass,
    /// The launcher's key, and nothing moves.
    Stay,
    To(Option<Highlight>),
}

/// All apps as rows: `#`, A to Z, then other scripts, each under its header,
/// each in the library's own name order.
pub fn build_rows(library: &[LauncherApplication]) -> Vec<AllAppsRow> {
    let mut groups: std::collections::BTreeMap<GroupKey, Vec<usize>> = Default::default();
    for (index, application) in library.iter().enumerate() {
        groups
            .entry(GroupKey::of(&application.display_name))
            .or_default()
            .push(index);
    }
    let mut rows = Vec::new();
    for (key, members) in groups {
        rows.push(AllAppsRow::Header(key));
        rows.extend(members.into_iter().map(AllAppsRow::App));
    }
    rows
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ranking::PathKind;
    use crate::ranking::tests::FakeTree;
    use app_catalog_core::{CatalogBuilder, DirectoryRank, EntryScope, NoProbe};
    use launcher_core::{IndexOptions, SearchIndex};
    use launcher_platform::{PlatformError, RecordingStarter};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn id(value: &str) -> DesktopId {
        DesktopId::new(value).unwrap()
    }

    /// Builds a snapshot from `(desktop id, desktop-entry lines)`, through
    /// the real catalog builder and the real index.
    pub(crate) fn snapshot_of(entries: &[(&str, &str)]) -> LauncherSnapshot {
        let mut builder = CatalogBuilder::new(&NoProbe);
        for (desktop_id, body) in entries {
            let text = format!("[Desktop Entry]\nType=Application\n{body}\n");
            builder.add_entry(
                DesktopId::new(*desktop_id).unwrap(),
                PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
                &DirectoryRank {
                    rank: 0,
                    scope: EntryScope::System,
                },
                text.as_bytes(),
            );
        }
        let catalog = builder.build();
        let index = SearchIndex::from_catalog(&catalog, &IndexOptions::new());
        LauncherSnapshot {
            catalog: Arc::new(catalog),
            index: Arc::new(index),
        }
    }

    pub(crate) fn library() -> LauncherSnapshot {
        snapshot_of(&[
            (
                "archive.desktop",
                "Name=Archive Manager\nExec=archive\nCategories=Utility;",
            ),
            (
                "browser.desktop",
                "Name=Browser\nExec=browser\nCategories=Network;WebBrowser;",
            ),
            (
                "calculator.desktop",
                "Name=Calculator\nExec=calc\nCategories=Utility;Calculator;",
            ),
            (
                "calendar.desktop",
                "Name=Calendar\nExec=cal\nCategories=Office;Calendar;",
            ),
            (
                "editor.desktop",
                "Name=Text Editor\nKeywords=notes;\nExec=edit\nCategories=Utility;TextEditor;",
            ),
            (
                "files.desktop",
                "Name=Files\nExec=files\nCategories=System;FileManager;",
            ),
            (
                "htop.desktop",
                "Name=htop\nExec=htop\nTerminal=true\nCategories=System;Monitor;",
            ),
            (
                "terminal.desktop",
                "Name=Terminal\nKeywords=shell;commandline;\nExec=term\nCategories=System;TerminalEmulator;",
            ),
            ("zh.desktop", "Name=檔案管理員\nExec=zh"),
            ("7zip.desktop", "Name=7-Zip\nExec=7z"),
            (
                "io.betteros.Files.desktop",
                "Name=Better Files\nExec=better-files",
            ),
            (
                "io.betteros.Manager.desktop",
                "Name=Better Manager\nExec=better-manager",
            ),
        ])
    }

    fn ready() -> OverlayModel {
        let mut model = OverlayModel::new();
        model.apply_snapshot(library());
        model.set_managed(ManagedApplications::built_in());
        model
    }

    fn pinned_names(model: &OverlayModel) -> Vec<String> {
        model
            .pinned()
            .iter()
            .map(|application| application.display_name.clone())
            .collect()
    }

    #[test]
    fn the_launcher_keys_and_the_keys_left_to_the_search_box() {
        assert_eq!(nav_key("escape", false), Some(NavKey::Escape));
        assert_eq!(nav_key("enter", false), Some(NavKey::Enter));
        assert_eq!(nav_key("f10", true), Some(NavKey::Menu));
        assert_eq!(nav_key("menu", false), Some(NavKey::Menu));
        assert_eq!(nav_key("down", true), Some(NavKey::Down));
        for key in [
            "a",
            "space",
            "backspace",
            "tab",
            "f10",
            "delete",
            "Escape",
            "",
        ] {
            assert_eq!(nav_key(key, false), None, "{key:?}");
        }
        // Shift with Left, Right, Home, and End selects text in the box.
        for key in ["left", "right", "home", "end"] {
            assert_eq!(nav_key(key, true), None, "{key}");
        }
    }

    #[test]
    fn a_first_start_page_is_pinned_by_category_and_recommends_nothing_yet() {
        let model = ready();
        assert_eq!(model.screen(), Screen::Home);
        assert_eq!(
            pinned_names(&model),
            [
                "Browser",
                "Files",
                "Text Editor",
                "Terminal",
                "Calculator",
                "Calendar"
            ]
        );
        assert!(model.recommended().is_empty());
    }

    #[test]
    fn an_empty_stored_arrangement_stays_empty() {
        let mut model = ready();
        model.set_start(Some(Pins::default()), Recent::default());
        assert!(model.pinned().is_empty());
        model.set_start(None, Recent::default());
        assert_eq!(
            model.pinned().len(),
            6,
            "never arranged starts from categories"
        );
    }

    #[test]
    fn a_pin_whose_app_was_removed_is_kept_but_not_drawn() {
        let mut model = ready();
        model.set_start(
            Some(Pins::new(vec![id("gone.desktop"), id("files.desktop")])),
            Recent::default(),
        );
        assert_eq!(pinned_names(&model), ["Files"]);
        assert_eq!(model.pins().ids().len(), 2);
    }

    #[test]
    fn pinning_unpinning_and_dragging_reorder_the_grid() {
        let mut model = ready();
        model.set_start(Some(Pins::default()), Recent::default());
        for app in ["files.desktop", "browser.desktop", "editor.desktop"] {
            assert!(model.pin(&id(app)));
        }
        assert_eq!(pinned_names(&model), ["Files", "Browser", "Text Editor"]);
        assert!(model.move_pinned(2, 0), "drag Text Editor onto Files");
        assert_eq!(pinned_names(&model), ["Text Editor", "Files", "Browser"]);
        assert!(!model.move_pinned(0, 9));
        assert!(model.move_to_front(&id("browser.desktop")));
        assert_eq!(pinned_names(&model), ["Browser", "Text Editor", "Files"]);
        assert!(model.unpin(&id("editor.desktop")));
        assert_eq!(pinned_names(&model), ["Browser", "Files"]);
    }

    #[test]
    fn recommended_shows_the_latest_launches_and_forgets_on_request() {
        let mut model = ready();
        let starter = RecordingStarter::succeeding();
        for (at, app) in ["files.desktop", "editor.desktop", "files.desktop"]
            .iter()
            .enumerate()
        {
            model.launch(&id(app), &starter, at as u64);
        }
        let names: Vec<&str> = model
            .recommended()
            .iter()
            .map(|r| r.application.display_name.as_str())
            .collect();
        assert_eq!(names, ["Files", "Text Editor"]);
        assert!(model.forget(&id("files.desktop")));
        assert_eq!(model.recommended().len(), 1);
    }

    #[test]
    fn all_apps_groups_names_under_hash_letters_and_other_scripts() {
        let model = ready();
        let headers: Vec<GroupKey> = model
            .all_apps_rows()
            .iter()
            .filter_map(|row| match row {
                AllAppsRow::Header(key) => Some(*key),
                AllAppsRow::App(_) => None,
            })
            .collect();
        assert_eq!(
            headers,
            [
                GroupKey::Symbol,
                GroupKey::Letter('A'),
                GroupKey::Letter('B'),
                GroupKey::Letter('C'),
                GroupKey::Letter('F'),
                GroupKey::Letter('H'),
                GroupKey::Letter('T'),
                GroupKey::Other,
            ]
        );
        assert_eq!(GroupKey::of("Évolution"), GroupKey::Letter('E'));
        assert_eq!(GroupKey::of("檔案"), GroupKey::Other);
        assert_eq!(GroupKey::of("0 A.D."), GroupKey::Symbol);
        assert_eq!(GroupKey::all().len(), 28);
    }

    #[test]
    fn the_letter_grid_jumps_to_a_section_and_skips_empty_letters() {
        let mut model = ready();
        model.open_all_apps();
        assert_eq!(model.screen(), Screen::AllApps);
        // Enter on the `#` header opens the grid on `#`.
        model.handle_key(NavKey::Down);
        assert_eq!(model.highlight(), Some(Highlight::Row(0)));
        assert_eq!(model.handle_key(NavKey::Enter), Effect::Redraw);
        assert_eq!(model.jump(), Some(0));
        // Right skips to A, the next letter with a section.
        model.handle_key(NavKey::Right);
        assert_eq!(
            GroupKey::all()[model.jump().unwrap()],
            GroupKey::Letter('A')
        );
        model.handle_key(NavKey::Right);
        model.handle_key(NavKey::Right);
        model.handle_key(NavKey::Right);
        assert_eq!(
            GroupKey::all()[model.jump().unwrap()],
            GroupKey::Letter('F')
        );
        model.handle_key(NavKey::Enter);
        assert_eq!(model.jump(), None);
        let Some(Highlight::Row(row)) = model.highlight() else {
            panic!("the keyboard is on the header");
        };
        assert_eq!(
            model.all_apps_rows()[row],
            AllAppsRow::Header(GroupKey::Letter('F'))
        );
        assert!(model.take_scroll());
        assert!(!model.take_scroll());
        assert!(model.has_group(GroupKey::Other));
        assert!(!model.has_group(GroupKey::Letter('Q')));
        // Escape closes the grid, then leaves All apps, then closes.
        model.open_letters();
        assert_eq!(model.handle_key(NavKey::Escape), Effect::Redraw);
        assert_eq!(model.jump(), None);
        assert_eq!(model.handle_key(NavKey::Escape), Effect::Redraw);
        assert_eq!(model.screen(), Screen::Home);
        assert_eq!(model.handle_key(NavKey::Escape), Effect::Close);
    }

    #[test]
    fn arrows_walk_the_start_page_grid_and_hand_back_to_the_search_box() {
        let mut model = ready();
        let starter = RecordingStarter::succeeding();
        for (at, app) in ["files.desktop", "editor.desktop", "browser.desktop"]
            .iter()
            .enumerate()
        {
            model.launch(&id(app), &starter, at as u64);
        }
        // Six pins: one row. Three recommended: two rows of two.
        assert_eq!(
            model.handle_key(NavKey::Left),
            Effect::Pass,
            "the box keeps Left"
        );
        model.handle_key(NavKey::Down);
        assert_eq!(model.highlight(), Some(Highlight::Pinned(0)));
        model.handle_key(NavKey::Right);
        model.handle_key(NavKey::Right);
        model.handle_key(NavKey::Right);
        assert_eq!(model.highlight(), Some(Highlight::Pinned(3)));
        model.handle_key(NavKey::Down);
        assert_eq!(
            model.highlight(),
            Some(Highlight::Recommended(1)),
            "right half of the row"
        );
        model.handle_key(NavKey::Down);
        assert_eq!(
            model.highlight(),
            Some(Highlight::Recommended(1)),
            "nothing below"
        );
        model.handle_key(NavKey::Left);
        model.handle_key(NavKey::Down);
        assert_eq!(model.highlight(), Some(Highlight::Recommended(2)));
        model.handle_key(NavKey::Up);
        model.handle_key(NavKey::Up);
        assert_eq!(model.highlight(), Some(Highlight::Pinned(0)));
        model.handle_key(NavKey::End);
        assert_eq!(model.highlight(), Some(Highlight::Pinned(5)));
        model.handle_key(NavKey::Right);
        assert_eq!(
            model.highlight(),
            Some(Highlight::Pinned(5)),
            "rows do not wrap"
        );
        model.handle_key(NavKey::Up);
        assert_eq!(model.highlight(), Some(Highlight::AllAppsButton));
        assert_eq!(model.handle_key(NavKey::Enter), Effect::Redraw);
        assert_eq!(model.screen(), Screen::AllApps);
        model.go_home();
        model.handle_key(NavKey::Down);
        model.handle_key(NavKey::Up);
        model.handle_key(NavKey::Up);
        assert_eq!(model.highlight(), None, "back in the search box");
    }

    #[test]
    fn enter_opens_what_the_keyboard_is_on_in_the_way_it_runs() {
        let mut model = ready();
        model.set_query("htop");
        assert_eq!(model.highlight(), Some(Highlight::Result(0)));
        assert_eq!(
            model.handle_key(NavKey::Enter),
            Effect::Run(Target::App(id("htop.desktop")), Command::RunInTerminal),
            "a terminal app says it runs in a terminal"
        );
        model.set_query("files");
        assert_eq!(
            model.handle_key(NavKey::Enter),
            Effect::Run(Target::App(id("files.desktop")), Command::Open)
        );
        model.set_query("12*(3+4)");
        assert_eq!(
            model.handle_key(NavKey::Enter),
            Effect::Run(Target::Calculation("84".into()), Command::CopyResult)
        );
        model.set_query("qqqq");
        assert!(model.is_empty_result());
        assert_eq!(model.handle_key(NavKey::Enter), Effect::Redraw);
        assert_eq!(model.handle_key(NavKey::Escape), Effect::Close);
    }

    #[test]
    fn search_results_move_with_up_and_down_and_clearing_returns_to_the_page() {
        let mut model = ready();
        model.open_all_apps();
        model.set_query("cal");
        assert_eq!(model.screen(), Screen::Search);
        let names: Vec<&str> = model
            .rows()
            .iter()
            .map(|a| a.display_name.as_str())
            .collect();
        assert_eq!(names, ["Calendar", "Calculator"]);
        model.handle_key(NavKey::Down);
        assert_eq!(model.highlight(), Some(Highlight::Result(1)));
        model.handle_key(NavKey::Down);
        assert_eq!(model.highlight(), Some(Highlight::Result(1)));
        assert_eq!(model.handle_key(NavKey::Left), Effect::Pass);
        model.clear_query();
        assert_eq!(model.screen(), Screen::AllApps, "the page that was there");
        assert_eq!(model.highlight(), None);
    }

    #[test]
    fn a_path_query_offers_the_path_with_open_and_its_folder() {
        let mut model = ready();
        let tree = FakeTree::with(&[
            ("/home/someone", PathKind::Folder),
            ("/home/someone/Documents", PathKind::Folder),
        ]);
        model.set_query_with("~/Doc", &tree);
        let Some(Target::Path(hit)) = model.target(Highlight::Result(0)) else {
            panic!("a path result first");
        };
        assert_eq!(hit.path, PathBuf::from("/home/someone/Documents"));
        assert_eq!(
            model.commands(&Target::Path(hit.clone()), Place::Search),
            [Command::OpenPath, Command::OpenContainingFolder]
        );
        assert_eq!(
            model.handle_key(NavKey::Enter),
            Effect::Run(Target::Path(hit), Command::OpenPath)
        );
    }

    #[test]
    fn files_and_folders_arrive_later_and_a_stale_answer_is_dropped() {
        let mut model = ready();
        let home = Some(PathBuf::from("/home/someone"));
        model.set_query("~/Doc");
        // Typing reads nothing from disk: the answer is asked for.
        assert!(model.results().is_empty());
        let (asked, query) = model.path_request(home.clone()).expect("a path query");
        assert_eq!(query, "~/Doc");
        let documents = PathHit {
            path: PathBuf::from("/home/someone/Documents"),
            kind: PathKind::Folder,
        };
        // The person kept typing: the old answer must not land.
        model.set_query("~/Docu");
        assert!(!model.apply_path_hits(asked, vec![documents.clone()]));
        assert!(model.results().is_empty());
        let (current, _) = model.path_request(home.clone()).unwrap();
        assert!(model.apply_path_hits(current, vec![documents.clone()]));
        assert_eq!(model.results(), [SearchItem::Path(documents.clone())]);
        assert_eq!(model.highlight(), Some(Highlight::Result(0)));
        // A path after a calculation, before applications.
        model.set_query("cal");
        assert_eq!(model.path_request(home.clone()), None, "not a path");
        let (generation, _) = {
            model.set_query("/");
            model.path_request(home).unwrap()
        };
        model.clear_query();
        assert!(!model.apply_path_hits(generation, vec![documents]));
    }

    #[test]
    fn each_place_offers_its_own_menu() {
        let mut model = ready();
        model.set_start(
            Some(Pins::new(vec![
                id("files.desktop"),
                id("io.betteros.Files.desktop"),
            ])),
            Recent::default(),
        );
        let files = Target::App(id("files.desktop"));
        assert_eq!(
            model.commands(&files, Place::Pinned),
            [
                Command::Open,
                Command::UnpinFromStart,
                Command::OpenFileLocation
            ],
            "the first pin has no Move to front"
        );
        let better_files = Target::App(id("io.betteros.Files.desktop"));
        assert_eq!(
            model.commands(&better_files, Place::Pinned),
            [
                Command::Open,
                Command::MoveToFront,
                Command::UnpinFromStart,
                Command::OpenFileLocation,
                Command::Uninstall(ManagedComponent {
                    id: "better-files".into(),
                    display_name: "Better Files".into(),
                }),
            ],
            "Better Manager manages Better Files, so it can be uninstalled there"
        );
        let htop = Target::App(id("htop.desktop"));
        assert_eq!(
            model.commands(&htop, Place::Recommended),
            [
                Command::RunInTerminal,
                Command::PinToStart,
                Command::RemoveFromList,
                Command::OpenFileLocation,
            ]
        );
        let manager = Target::App(id("io.betteros.Manager.desktop"));
        assert!(
            !model
                .commands(&manager, Place::AllApps)
                .iter()
                .any(|c| matches!(c, Command::Uninstall(_))),
            "Better Manager is not removed from inside itself"
        );
        assert_eq!(
            model.commands(&Target::Calculation("2".into()), Place::Search),
            [Command::CopyResult]
        );
        // The menu key opens the menu for what the keyboard is on.
        model.handle_key(NavKey::Down);
        assert_eq!(
            model.handle_key(NavKey::Menu),
            Effect::Menu(files, Place::Pinned)
        );
    }

    #[test]
    fn a_launch_is_remembered_and_a_failure_keeps_the_launcher_open() {
        let mut model = ready();
        model.set_query("cal");
        let starter = RecordingStarter::succeeding();
        assert_eq!(
            model.activate(&starter, 7),
            Activation::Launched(id("calendar.desktop"))
        );
        assert_eq!(model.recent().entries()[0].last, 7);

        let failing =
            RecordingStarter::failing(PlatformError::UnknownApplication("calendar.desktop".into()));
        let Activation::Failed(key) = model.activate(&failing, 8) else {
            panic!("a failed launch must not report a launch");
        };
        assert!(key.starts_with("launcher.platform.error.unknown_application"));
        assert_eq!(model.notice(), Some(&Notice::LaunchFailed(key)));
        model.set_query("ca");
        assert!(model.notice().is_none(), "the next keystroke clears it");
        model.set_query("qqqq");
        assert_eq!(model.activate(&starter, 9), Activation::NothingSelected);
    }

    #[test]
    fn a_catalog_change_keeps_the_query_and_the_highlighted_app() {
        let mut model = ready();
        model.set_query("t");
        let editor = model
            .results()
            .iter()
            .position(|item| {
                item.application()
                    .is_some_and(|a| a.display_name == "Text Editor")
            })
            .unwrap();
        model.set_highlight(Some(Highlight::Result(editor)));
        model.begin_refresh();
        assert_eq!(model.load_state(), LoadState::Refreshing);
        assert!(
            !model.rows().is_empty(),
            "a re-read does not blank the screen"
        );
        model.apply_snapshot(snapshot_of(&[
            ("tango.desktop", "Name=Tango\nExec=tango"),
            ("editor.desktop", "Name=Text Editor\nExec=edit"),
        ]));
        assert_eq!(model.query(), "t");
        assert_eq!(model.load_state(), LoadState::Ready);
        let Some(Highlight::Result(i)) = model.highlight() else {
            panic!("still on a result");
        };
        assert_eq!(
            model.results()[i].application().unwrap().display_name,
            "Text Editor"
        );
    }

    #[test]
    fn an_overlay_starts_loading_and_an_empty_machine_says_so() {
        let model = OverlayModel::new();
        assert_eq!(model.load_state(), LoadState::Loading);
        assert!(model.rows().is_empty());
        assert!(!model.is_empty_library(), "not read is not empty");
        let mut empty = OverlayModel::new();
        empty.apply_snapshot(snapshot_of(&[]));
        assert!(empty.is_empty_library());
        assert!(empty.pinned().is_empty());
    }
}

//! The GPUI window: the session, the text fields, and the frame pump.
//!
//! Everything this type decides, it decides by calling
//! [`crate::session::FilesSession`]. What is left here is what genuinely needs
//! a window: focus handles, the two text inputs, the repeating task that
//! drains the reader, and the viewport arithmetic that tells the session how
//! many tiles fit across.
//!
//! **No directory is read on this thread.** The pane's reader spawns its own
//! thread; the pump only drains a channel and merges what has already arrived.

use std::sync::Arc;
use std::time::Duration;

use better_ui::fluent::menu::MenuOpen;
use better_ui::fluent::theme::Appearance;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use smol::Timer;

use crate::apps::CatalogHandle;
use crate::devicelink::StorageLink;
use crate::i18n::Locale;
use crate::keys::{Modifiers, command_for};
use crate::launch::StartPlan;
use crate::layout::{COMPACT_VIEWPORT_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, visible_rows};
use crate::openwith::SessionDefaults;
use crate::prefs::PreferenceStore;
use crate::preview::PreviewPanel;
use crate::reader::FilesReader;
use crate::session::{FilesSession, Notice, PendingDialog, SessionSetup};
use crate::toolbar::{FilesystemValidator, display_path};

/// How often the window drains the reader and re-reads the job engine.
///
/// A frame's worth while something is moving, and a quarter of a second when
/// nothing is: an idle file manager must not wake the CPU sixty times a second
/// to discover that nothing has changed.
const BUSY_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_INTERVAL: Duration = Duration::from_millis(250);

/// A context menu that is open: what it was opened on, where, and its
/// keyboard state.
pub(crate) struct OpenMenu {
    pub(crate) target: crate::context_menu::MenuTarget,
    pub(crate) nav: Option<crate::shell::NavTarget>,
    pub(crate) position: Point<Pixels>,
    pub(crate) state: Entity<MenuOpen>,
}

pub struct FilesApp {
    pub(crate) session: FilesSession,
    /// The appearance the person chose, saved under
    /// `$XDG_CONFIG_HOME/better-os/files/appearance` (ADR 0004).
    pub(crate) appearance: Appearance,
    _follow_desktop: Option<Subscription>,
    /// The navigation pane's open or compact state when the person chose
    /// one with its toggle; `None` follows the window's width.
    pub(crate) nav_compact: Option<bool>,
    pub(crate) context_menu: Option<OpenMenu>,
    pub(crate) show_shortcuts: bool,
    pub(crate) weak: WeakEntity<Self>,
    /// The file type names and themed icons, read once per type.
    pub(crate) type_names: Arc<files_platform::MimeDescriptions>,
    pub(crate) icons: crate::views::IconCache,
    pub(crate) thumbnails: crate::views::Thumbnails,
    /// Split view: the right pane, a Better Files of its own.
    pub(crate) split: Option<Entity<FilesApp>>,
    /// True for that right pane: no title bar and no sidebar of its own,
    /// and Ctrl+W closes it.
    pub(crate) embedded: bool,
    /// The window's own Better Files, for a right pane to close itself.
    pub(crate) parent: Option<WeakEntity<FilesApp>>,
    /// For a right pane: the width it really has, set by the window's own
    /// Better Files each frame.
    pub(crate) pane_width: Option<f32>,
    /// The whole window's width, which decides whether the navigation pane
    /// is compact. `viewport` is this pane's share of the window.
    pub(crate) window_width: Pixels,
    /// The tailnet's machines and the Dropbox folder, refreshed in the
    /// background.
    pub(crate) sharing: Sharing,
    /// Plugins and their latest answers.
    pub(crate) plugins: crate::plugin_host::PluginHost,
    /// Whether the search field shows its short hint (a narrow pane).
    pub(crate) search_short: bool,
    /// A plugin's own page, when one is open in the main pane.
    pub(crate) plugin_view: Option<crate::plugin_page::PluginPage>,
    /// The items opened from each plugin, most recent first, for sections
    /// that list only those.
    pub(crate) plugin_opened: std::collections::BTreeMap<String, Vec<String>>,
    /// The content list's scroll position, which rubber-band selection
    /// reads to turn the pointer into content coordinates.
    pub(crate) content_scroll: UniformListScrollHandle,
    /// A rubber band being dragged: where it started and where the pointer
    /// is, in content coordinates, and what was selected before (kept when
    /// Ctrl is held).
    pub(crate) band: Option<crate::views::Band>,
    _subscriptions: Vec<Subscription>,
    pub(crate) path_input: Entity<InputState>,
    pub(crate) dialog_input: Entity<InputState>,
    pub(crate) focus_handle: FocusHandle,
    /// True while the path field is being edited, so the frame does not
    /// overwrite what is being typed with the current location.
    pub(crate) editing_path: bool,
    /// Applies to the conflict dialog: the next answer covers every remaining
    /// conflict of the same kind.
    pub(crate) apply_to_remaining: bool,
    /// The last viewport, so a keyboard page is the size of the page that is
    /// actually on screen.
    pub(crate) viewport: Size<Pixels>,
    /// The search field. A second text input beside the path field, focused by
    /// `Ctrl+F` the way the path field is focused by `Ctrl+L`.
    pub(crate) search_input: Entity<InputState>,
    pub(crate) editing_search: bool,
    /// The type-to-filter field, and whether it has the keyboard.
    pub(crate) filter_input: Entity<InputState>,
    /// The password field of a server sign-in.
    pub(crate) password_input: Entity<InputState>,
    pub(crate) filtering: bool,
    /// Set after a sign-in is sent, so the next frame empties the password
    /// field (the submit has no window to do it with).
    pub(crate) clear_password: bool,
    pub(crate) login_prepared: bool,
    /// The chooser entity, while Open With is showing one. Held so the window
    /// can keep it alive and hear its event; the chooser performs the launch
    /// and any association write itself.
    pub(crate) chooser: Option<Entity<app_chooser_gui::AppChooser>>,
    _chooser_subscription: Option<Subscription>,
    /// The catalog generation the last frame drew, so a background reload is
    /// noticed without comparing two catalogs.
    pub(crate) catalog_generation: u64,
    /// The LocalSend list's generation the window last drew.
    pub(crate) localsend_generation: u64,
    _pump: Option<Task<()>>,
}

impl FilesApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>, start: StartPlan) -> Self {
        let preference_store = PreferenceStore::from_env();
        let loaded = preference_store.load();
        let preferences = loaded.preferences;
        let directories = files_platform::UserDirectories::from_env();
        let reader = Arc::new(FilesReader::from_env());
        reader.set_include_hidden_applications(preferences.show_hidden);
        let catalog = reader.catalog().clone();
        // Reading every XDG application directory is blocking I/O, so it
        // happens on its own thread and the Applications location fills in when
        // it lands. The watcher then keeps it current: `notify` delivers the
        // change, and the answer to any change is a whole reload, because
        // precedence means one new file can reveal a different application.
        start_catalog_thread(catalog.clone());
        // One link, two holders: the session draws device states from it, and
        // the job tracker reports this process's writes through it — from a
        // job's own thread, and for as long as the job runs.
        let link: Arc<dyn crate::devices::DeviceLink> = Arc::new(StorageLink::start());
        crate::shared_tracker().attach(link.clone());
        // With no folder on the command line, the window opens where it was
        // last closed, when that folder is still there; otherwise at home.
        let view_store = crate::settings_store::ViewStateStore::from_env();
        let last = view_store
            .load()
            .last_folder()
            .and_then(|path| files_core::Location::local(path).ok());
        let default = last.unwrap_or_else(|| {
            directories
                .home()
                .cloned()
                .unwrap_or_else(|| files_core::Location::Local(files_core::LocalPath::root()))
        });

        let mut session = FilesSession::new(SessionSetup {
            start: start.location(default),
            preferences,
            preference_store,
            view_store,
            bookmark_store: crate::bookmarks::BookmarkStore::from_env(),
            directories,
            mounts: files_platform::MountTable::from_env(),
            reader,
            engine: crate::shared_engine(),
            catalog,
            defaults: Box::new(SessionDefaults::from_env()),
            // The real thing: the platform crate spawns from an argument
            // vector built out of the desktop entry.
            spawner: Box::new(app_catalog_platform::SystemSpawner),
            link: Box::new(crate::devices::SharedLink(link)),
            preview: PreviewPanel::default(),
        });
        session.preview.open = session.preferences.preview_open;
        session.search.include_hidden = session.preferences.search_hidden;
        // Work an earlier Better Files did not live to finish is offered
        // first thing, in the operation center.
        session.interrupted = crate::interrupted_jobs();
        if !session.interrupted.is_empty() {
            session.operations_open = true;
        }
        if let Some(problem) = loaded.problem {
            session.notice = Some(Notice::Key(problem));
        }
        // A file handed to the window opens in its folder, selected. The
        // selection is set before the listing has delivered anything and holds
        // until the entry arrives.
        if let Some(name) = start.selection() {
            session.select_by_name(name);
        }
        if start.needs_notice() {
            session.notice = Some(Notice::Start {
                problem: start.problem,
                ignored: start.ignored,
            });
        }

        let locale = session.locale;
        let path_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).path_placeholder)
        });
        let dialog_input = cx.new(|cx| InputState::new(window, cx));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).search_placeholder)
        });
        let password_input = cx.new(|cx| InputState::new(window, cx).masked(true));
        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).filter_placeholder)
        });

        // Better OS is dark-first; the person's own choice wins. The kit
        // repaints gpui-component in the Fluent palette.
        let appearance = Appearance::default_path("files")
            .map(|path| Appearance::load(&path))
            .unwrap_or_default();
        let follow = appearance.apply_and_follow(window, cx);

        let locale_for_plugins = session.locale.resolved();
        let language = match session.locale.resolved() {
            Locale::ZhTw => "zh_TW".to_string(),
            _ => "en".to_string(),
        };
        let subscriptions = vec![
            cx.subscribe_in(
                &path_input,
                window,
                |app, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => app.submit_path(window, cx),
                    InputEvent::Blur => {
                        app.editing_path = false;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &dialog_input,
                window,
                |app, _, event: &InputEvent, _window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        app.submit_dialog(cx);
                    }
                },
            ),
            cx.subscribe_in(
                &filter_input,
                window,
                |app, state, event: &InputEvent, _w, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        app.session.set_filter(&text);
                        cx.notify();
                    }
                    InputEvent::Focus => {
                        app.filtering = true;
                        cx.notify();
                    }
                    InputEvent::Blur => {
                        app.filtering = false;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &search_input,
                window,
                |app, state, event: &InputEvent, _w, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        if text != app.session.search.text {
                            app.session.set_search_text(text);
                            cx.notify();
                        }
                    }
                    InputEvent::Focus => {
                        app.editing_search = true;
                        cx.notify();
                    }
                    InputEvent::Blur => {
                        app.editing_search = false;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];

        let mut app = Self {
            session,
            appearance,
            _follow_desktop: follow,
            nav_compact: None,
            context_menu: None,
            show_shortcuts: false,
            weak: cx.entity().downgrade(),
            type_names: Arc::new(files_platform::MimeDescriptions::from_env(language)),
            icons: crate::views::IconCache::from_env(),
            thumbnails: crate::views::Thumbnails::from_env(),
            split: None,
            embedded: false,
            parent: None,
            pane_width: None,
            window_width: px(1_200.0),
            sharing: Sharing::start(),
            plugins: crate::plugin_host::PluginHost::start(match locale_for_plugins {
                Locale::ZhTw => "zh-TW",
                _ => "en",
            }),
            plugin_view: None,
            search_short: false,
            plugin_opened: crate::plugins::opened_path()
                .map(|path| crate::plugins::load_opened(&path))
                .unwrap_or_default(),
            content_scroll: UniformListScrollHandle::new(),
            band: None,
            _subscriptions: subscriptions,
            path_input,
            dialog_input,
            focus_handle: cx.focus_handle(),
            editing_path: false,
            apply_to_remaining: false,
            viewport: size(px(1_200.0), px(800.0)),
            search_input,
            editing_search: false,
            filter_input,
            password_input,
            filtering: false,
            clear_password: false,
            login_prepared: false,
            chooser: None,
            _chooser_subscription: None,
            catalog_generation: 0,
            localsend_generation: 0,
            _pump: None,
        };
        app.sync_path_field(window, cx);
        app.start_pump(cx);
        app
    }

    /// The repeating drain.
    ///
    /// It is a task rather than a thread because the work it does is merging
    /// batches into the model, which the window owns. The reading itself is
    /// already on the reader's own thread.
    fn start_pump(&mut self, cx: &mut Context<Self>) {
        self._pump = Some(cx.spawn(async move |this, cx| {
            loop {
                // A failed update means the window is gone, which is the one
                // way this loop ends. The engine and its jobs are untouched by
                // that: they belong to the process, not to this window.
                let Ok(interval) = this.update(cx, |app, cx| {
                    if app.session.pump() {
                        cx.notify();
                    }
                    if app
                        .thumbnails
                        .ready
                        .swap(false, std::sync::atomic::Ordering::AcqRel)
                    {
                        cx.notify();
                    }
                    if app.plugins.take_changed() {
                        cx.notify();
                    }
                    for outcome in app.plugins.take_outcomes() {
                        match outcome {
                            crate::plugin_host::Outcome::Notice(text)
                            | crate::plugin_host::Outcome::Failed(text) => {
                                app.session.notice = Some(Notice::Key(text));
                            }
                            crate::plugin_host::Outcome::Copy(text) => {
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            }
                            crate::plugin_host::Outcome::Go(path) => {
                                if let Ok(location) = files_core::Location::local(path) {
                                    app.session.navigate_to(location);
                                }
                            }
                        }
                        cx.notify();
                    }
                    let finished = app
                        .sharing
                        .last
                        .lock()
                        .ok()
                        .and_then(|mut last| last.take());
                    if let Some((machine, sent)) = finished {
                        let c = crate::i18n::copy(app.locale());
                        let words = if sent {
                            c.taildrop_sent
                        } else {
                            c.taildrop_failed
                        };
                        app.session.notice =
                            Some(Notice::Key(words.replace("{machine}", &machine)));
                        cx.notify();
                    }
                    // A LocalSend device that answered while a menu is open
                    // joins it now.
                    let generation = app.sharing.localsend_generation();
                    if generation != app.localsend_generation {
                        app.localsend_generation = generation;
                        if app.context_menu.is_some() {
                            cx.notify();
                        }
                    }
                    // A background catalog reload is a redraw for the
                    // Applications location and nothing else.
                    let generation = app.session.catalog_generation();
                    if generation != app.catalog_generation {
                        app.catalog_generation = generation;
                        if matches!(app.session.location(), files_core::Location::Applications) {
                            app.session.reload();
                        }
                        cx.notify();
                    }
                    if app.session.is_listing() || !app.session.jobs.is_empty() {
                        BUSY_INTERVAL
                    } else {
                        IDLE_INTERVAL
                    }
                }) else {
                    break;
                };
                Timer::after(interval).await;
            }
        }));
    }

    pub(crate) fn locale(&self) -> Locale {
        self.session.locale
    }

    pub(crate) fn compact(&self) -> bool {
        self.viewport.width < px(COMPACT_VIEWPORT_WIDTH)
    }

    /// How many tiles fit across the content area at the current width.
    pub(crate) fn columns(&self) -> usize {
        let available = (self.content_width() - 40.0).max(120.0);
        self.session.content.columns(available)
    }

    /// How many rows a page is, for Page Up and Page Down.
    pub(crate) fn page_rows(&self) -> usize {
        visible_rows(
            (f32::from(self.viewport.height) - 200.0).max(120.0),
            self.session.content.row_height(),
        )
    }

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.set_locale(locale);
        // The type names follow the language.
        let language = match locale.resolved() {
            Locale::ZhTw => "zh_TW",
            _ => "en",
        };
        self.type_names = Arc::new(files_platform::MimeDescriptions::from_env(language));
        let c = crate::i18n::copy(locale);
        self.path_input.update(cx, |state, cx| {
            state.set_placeholder(c.path_placeholder, window, cx)
        });
        self.search_input.update(cx, |state, cx| {
            state.set_placeholder(c.search_placeholder, window, cx)
        });
        self.search_short = false;
        cx.notify();
    }

    /// Applies and saves the appearance the person picked.
    pub(crate) fn set_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance = appearance;
        self._follow_desktop = appearance.apply_and_follow(window, cx);
        if let Some(path) = Appearance::default_path("files")
            && let Err(error) = appearance.save(&path)
        {
            self.session.notice =
                Some(Notice::Key(format!("files.prefs.error.unwritable:{error}")));
        }
        cx.notify();
    }

    /// Whether the navigation pane is drawn compact: the person's choice,
    /// or compact below the WinUI breakpoint.
    pub(crate) fn nav_compact(&self) -> bool {
        self.nav_compact
            .unwrap_or(self.window_width < px(COMPACT_VIEWPORT_WIDTH))
    }

    /// The search box's width: narrower in a narrow pane (half of a split
    /// window), so the address box keeps room for the current folder.
    pub(crate) fn search_width(&self) -> f32 {
        let content = f32::from(self.viewport.width) - self.nav_width();
        if content < 640.0 {
            150.0
        } else if self.compact() {
            200.0
        } else {
            280.0
        }
    }

    /// The navigation pane's width, which every width this pane computes
    /// subtracts from its viewport.
    pub(crate) fn nav_width(&self) -> f32 {
        if self.nav_compact() { 48.0 } else { 320.0 }
    }

    /// What every navigation does afterwards: the address box shows the new
    /// place, and an open menu closes.
    pub(crate) fn after_location_change(&mut self, _window: &mut Window) {
        self.editing_path = false;
        self.context_menu = None;
    }

    /// Turns the breadcrumbs into a text field holding the path, selected.
    pub(crate) fn begin_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_path = true;
        self.sync_path_field(window, cx);
        self.path_input
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Ctrl+Alt+N: opens the right pane on this folder, or closes it.
    pub(crate) fn toggle_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.embedded {
            return;
        }
        if self.split.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        let start = StartPlan {
            target: self
                .session
                .location()
                .as_local_path()
                .cloned()
                .map(crate::launch::StartTarget::Folder),
            problem: None,
            ignored: 0,
        };
        let parent = cx.entity().downgrade();
        let right = cx.new(|cx| {
            let mut app = FilesApp::new(window, cx, start);
            app.embedded = true;
            app.parent = Some(parent);
            app
        });
        let focus = right.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
        self.split = Some(right);
        cx.notify();
    }

    /// Shows the filter field and gives it the keyboard.
    pub(crate) fn open_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filtering = true;
        self.filter_input
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Clears the filter and hands the keyboard back to the files.
    pub(crate) fn close_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filtering = false;
        self.session.set_filter("");
        self.filter_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Sends files to a tailnet machine with Taildrop, on a worker thread,
    /// and says when it is done.
    pub(crate) fn taildrop(
        &mut self,
        files: Vec<std::path::PathBuf>,
        machine: crate::share::Machine,
    ) {
        let done = self.sharing.last.clone();
        let name = machine.name.clone();
        std::thread::spawn(move || {
            let sent = crate::share::taildrop(&files, &machine);
            if let Ok(mut last) = done.lock() {
                *last = Some((name, sent));
            }
        });
    }

    /// Copies a Dropbox share link for `path`.
    pub(crate) fn copy_dropbox_link(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match crate::share::dropbox_link(path) {
            Some(link) => cx.write_to_clipboard(ClipboardItem::new_string(link)),
            None => {
                self.session.notice = Some(Notice::Key(
                    crate::i18n::copy(self.locale())
                        .dropbox_link_failed
                        .to_string(),
                ))
            }
        }
    }

    /// `Ctrl+Shift+C`: the selected paths, or the folder's, as text on the
    /// clipboard, where every other application can paste them.
    pub(crate) fn copy_path_text(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.session.path_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Puts the current location back in the path field.
    pub(crate) fn sync_path_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = display_path(self.session.location());
        let current = self.path_input.read(cx).value().to_string();
        if current != text {
            self.path_input.update(cx, |state, cx| {
                state.set_value(text, window, cx);
            });
        }
    }

    pub(crate) fn submit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.path_input.read(cx).value().to_string();
        // `plugin:<id>` opens a plugin's own page, as in folio.
        if let Some(id) = text.trim().strip_prefix("plugin:") {
            if !self.open_plugin_view(id.trim()) {
                self.session.notice = Some(crate::session::Notice::Key(format!(
                    "files.plugin.no_view:{}",
                    id.trim()
                )));
            }
            self.editing_path = false;
            self.sync_path_field(window, cx);
            cx.notify();
            return;
        }
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        self.session
            .submit_path(&text, home.as_deref(), &FilesystemValidator);
        self.editing_path = false;
        self.sync_path_field(window, cx);
        cx.notify();
    }

    /// One keystroke, routed through the pure key table.
    pub(crate) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The right pane's keys are its own: the window's pane, which holds
        // it, must not act on them a second time.
        if self.embedded {
            cx.stop_propagation();
        }
        // The Performance mode confirmation is answered with its own buttons;
        // Escape is Cancel, and nothing else reaches the window behind it.
        if self.show_shortcuts {
            if matches!(event.keystroke.key.as_str(), "escape" | "?" | "f1")
                || event.keystroke.key_char.as_deref() == Some("?")
            {
                self.show_shortcuts = false;
                cx.notify();
            }
            return;
        }
        if self.session.policy_confirmation.is_some() {
            if event.keystroke.key == "escape" {
                self.session.cancel_policy();
                cx.notify();
            }
            return;
        }
        // The format chooser is answered from the keyboard as well as with its
        // buttons, and nothing else reaches the window behind it.
        if matches!(self.session.dialog, Some(PendingDialog::Compress(_))) {
            let modifiers = Modifiers {
                control: event.keystroke.modifiers.control,
                shift: event.keystroke.modifiers.shift,
                alt: event.keystroke.modifiers.alt,
            };
            if let Some(key) = crate::keys::chooser_key_for(&event.keystroke.key, modifiers) {
                self.session.chooser_key(key);
                cx.notify();
            }
            return;
        }
        // The text fields own their own keys while one is focused.
        if self.editing_path
            || self.editing_search
            || self.filtering
            || self.session.dialog.is_some()
        {
            if event.keystroke.key == "escape" {
                self.editing_path = false;
                if self.filtering {
                    self.close_filter(window, cx);
                }
                if self.editing_search {
                    self.editing_search = false;
                    self.session.close_search();
                    self.sync_search_field(window, cx);
                }
                self.session.dialog = None;
                cx.notify();
            } else if self.editing_path && event.keystroke.key == "tab" {
                // Tab completes the folder being typed, as folio's path bar.
                let text = self.path_input.read(cx).value().to_string();
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                if let Some(completed) = crate::pathbar::complete_path(&text, home.as_deref()) {
                    self.path_input
                        .update(cx, |state, cx| state.set_value(completed, window, cx));
                }
                cx.stop_propagation();
                cx.notify();
            } else if self.editing_search {
                // Every other keystroke goes to the field, and the query is
                // taken from it afterwards. Restarting the run is cheap: it
                // allocates a run and nothing else, and the scan is spread
                // over the frames that follow.
                let text = self.search_input.read(cx).value().to_string();
                if text != self.session.search.text {
                    self.session.set_search_text(text);
                    cx.notify();
                }
            }
            return;
        }
        // A plugin's page takes the arrows, Enter and Escape.
        if self.plugin_view.is_some()
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && self.plugin_page_key(event.keystroke.key.as_str())
        {
            cx.notify();
            return;
        }
        let modifiers = Modifiers {
            control: event.keystroke.modifiers.control,
            shift: event.keystroke.modifiers.shift,
            alt: event.keystroke.modifiers.alt,
        };
        let Some(command) = command_for(
            &event.keystroke.key,
            modifiers,
            event.keystroke.key_char.as_deref(),
            self.session.focus,
        ) else {
            return;
        };
        if command == crate::keys::Command::FocusPathField {
            self.begin_path_edit(window, cx);
            return;
        }
        match command {
            crate::keys::Command::FilterFolder => {
                self.open_filter(window, cx);
                return;
            }
            crate::keys::Command::SplitView => {
                self.toggle_split(window, cx);
                return;
            }
            crate::keys::Command::CloseTab if self.embedded => {
                // Ctrl+W in the right pane closes the pane, as in folio.
                if let Some(parent) = self.parent.clone() {
                    let _ = parent.update(cx, |app, cx| {
                        app.split = None;
                        window.focus(&app.focus_handle, cx);
                        cx.notify();
                    });
                }
                return;
            }
            crate::keys::Command::NewWindow => {
                open_window_at(self.session.location().as_local_path().cloned(), cx);
                return;
            }
            crate::keys::Command::CopyPath => {
                self.copy_path_text(cx);
                return;
            }
            crate::keys::Command::ShowShortcuts => {
                self.show_shortcuts = !self.show_shortcuts;
                cx.notify();
                return;
            }
            crate::keys::Command::ContextMenu => {
                let at = window.mouse_position();
                let target = if self.session.selected_entries().is_empty() {
                    crate::context_menu::MenuTarget::Background
                } else {
                    crate::context_menu::MenuTarget::Selection
                };
                self.open_content_menu(target, at, window, cx);
                return;
            }
            _ => {}
        }
        if command == crate::keys::Command::FocusSearch {
            self.editing_search = true;
            self.search_input.update(cx, |state, cx| {
                state.focus(window, cx);
            });
            cx.notify();
            return;
        }
        let columns = self.columns();
        let rows = self.page_rows();
        self.session.dispatch(command, columns, rows);
        self.prepare_dialog(window, cx);
        self.sync_path_field(window, cx);
        self.sync_chooser(window, cx);
        cx.notify();
    }

    /// Puts the session's query back in the field, for the paths that change it
    /// without typing.
    pub(crate) fn sync_search_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.search.text.clone();
        let current = self.search_input.read(cx).value().to_string();
        if current != text {
            self.search_input.update(cx, |state, cx| {
                state.set_value(text, window, cx);
            });
        }
    }

    /// Opens or closes the embedded Better App Chooser.
    ///
    /// The chooser is Better App Chooser's own component, fed the file's
    /// resolved MIME type. It launches the selection and, for Always Use,
    /// writes the association through `app-chooser-core` with its rollback
    /// record. The window neither launches nor writes: it opens the chooser and
    /// closes it when it reports back.
    pub(crate) fn sync_chooser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.session.chooser.clone() {
            Some(request) if self.chooser.is_none() => {
                let target = app_chooser_gui::ChooserTarget::new(
                    request.display_name.clone(),
                    request.target(),
                    request.mime.clone(),
                );
                let locale = match self.session.locale {
                    Locale::ZhTw => app_chooser_gui::Locale::ZhTw,
                    Locale::EnUs => app_chooser_gui::Locale::EnUs,
                    Locale::System => app_chooser_gui::Locale::System,
                };
                let entity = cx.new(|cx| {
                    app_chooser_gui::AppChooser::new(
                        target,
                        app_chooser_gui::ChooserMode::OpenWith,
                        locale,
                        window,
                        cx,
                    )
                });
                self._chooser_subscription = Some(cx.subscribe(
                    &entity,
                    |app, _entity, event: &app_chooser_gui::ChooserEvent, cx| {
                        let cancelled = matches!(event, app_chooser_gui::ChooserEvent::Cancelled);
                        app.session.close_chooser(cancelled);
                        app.chooser = None;
                        app._chooser_subscription = None;
                        cx.notify();
                    },
                ));
                self.chooser = Some(entity);
            }
            None => {
                self.chooser = None;
                self._chooser_subscription = None;
            }
            _ => {}
        }
    }

    /// Fills the dialog's text field with a sensible starting value.
    pub(crate) fn prepare_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match &self.session.dialog {
            Some(PendingDialog::Rename(path)) => Some(path.file_name()),
            Some(PendingDialog::RenameBookmark(index)) => self
                .session
                .bookmarks
                .get(*index)
                .map(|bookmark| bookmark.display_name()),
            Some(PendingDialog::NewFolder) | Some(PendingDialog::NewFile) => Some(String::new()),
            Some(PendingDialog::Format { device_label, .. }) => Some(
                device_label
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
                    .take(11)
                    .collect(),
            ),
            Some(PendingDialog::ServerLogin { .. }) => {
                self.password_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
                Some(std::env::var("USER").unwrap_or_default())
            }
            _ => None,
        };
        if let Some(initial) = initial {
            self.dialog_input.update(cx, |state, cx| {
                state.set_value(initial, window, cx);
            });
        }
    }

    /// Answers the open text dialog with what was typed.
    pub(crate) fn submit_dialog(&mut self, cx: &mut Context<Self>) {
        // The dialog's text is read, never written, here.
        let text = self.dialog_input.read(cx).value().to_string();
        let Some(dialog) = self.session.dialog.take() else {
            return;
        };
        match dialog {
            PendingDialog::NewFolder => self.session.create_folder(&text),
            PendingDialog::NewFile => self.session.create_file(&text),
            PendingDialog::Rename(path) => self.session.rename(&path, &text),
            PendingDialog::RenameBookmark(index) => self.session.set_bookmark_label(index, &text),
            PendingDialog::ConfirmDelete { targets } => {
                // Put it back and answer it through the one entry point that
                // constructs a confirmation.
                self.session.dialog = Some(PendingDialog::ConfirmDelete { targets });
                self.session.confirm_permanent_delete();
            }
            // Answered by its format buttons, not by a confirm: nothing to
            // submit, so it stays open.
            PendingDialog::Compress(chooser) => {
                self.session.dialog = Some(PendingDialog::Compress(chooser));
            }
            erase @ (PendingDialog::Format { .. } | PendingDialog::WriteImage { .. }) => {
                self.session.dialog = Some(erase);
                self.session.confirm_erase(&text);
            }
            PendingDialog::ServerLogin { address } => {
                // Read once, handed to the mount, and cleared from the field.
                let password = self.password_input.read(cx).value().to_string();
                self.session.mount_server(
                    address,
                    Some(crate::network::Credentials {
                        user: text,
                        password,
                    }),
                );
                self.clear_password = true;
            }
        }
        cx.notify();
    }

    pub(crate) fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.session.dialog = None;
        cx.notify();
    }
}

/// Loads the catalog once, then keeps it current.
///
/// One thread for both, because the reload a change triggers is the same
/// blocking read as the first load. `next_change` collapses everything that
/// arrives inside its settle window into one reload, so installing a package
/// that writes forty files reloads once.
fn start_catalog_thread(catalog: CatalogHandle) {
    let started = std::thread::Builder::new()
        .name("files-catalog".to_string())
        .spawn(move || {
            catalog.reload_from_env();
            let directories = app_catalog_platform::ApplicationDirectories::from_env();
            let Ok(watcher) = app_catalog_platform::CatalogWatcher::new(&directories) else {
                // No watcher: the catalog is still correct, it just will not
                // notice an install until the window is reopened. That is a
                // degraded feature, not a broken one, so the thread ends here
                // rather than polling.
                return;
            };
            loop {
                if watcher
                    .next_change(Duration::from_secs(3_600), Duration::from_millis(250))
                    .is_some()
                {
                    catalog.reload_from_env();
                }
            }
        });
    if let Err(error) = started {
        eprintln!("better-files: the application catalog could not be loaded: {error}");
    }
}

impl Focusable for FilesApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn files_window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1_280.0), px(820.0)), cx)),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        ..better_ui::window_chrome::window_options("io.betteros.Files")
    }
}

/// Ctrl+N: another Better Files window on `folder` (home when `None`), in
/// this process, so it shares the one job engine.
pub(crate) fn open_window_at(folder: Option<files_core::LocalPath>, cx: &mut App) {
    let start = StartPlan {
        target: folder.map(crate::launch::StartTarget::Folder),
        problem: None,
        ignored: 0,
    };
    let options = files_window_options(cx);
    cx.spawn(async move |cx| {
        let _ = cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| FilesApp::new(window, cx, start));
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        });
    })
    .detach();
}

/// Opens the window at the location the command line resolved to.
pub fn run(start: StartPlan) {
    let app = gpui_platform::application().with_assets(crate::icons::FilesAssets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = files_window_options(cx);

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| FilesApp::new(window, cx, start));
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })
            .expect("failed to open Better Files window");
        })
        .detach();

        // "Show in folder" from other applications: each request opens a
        // window, in this process.
        let (requests, received) = std::sync::mpsc::channel();
        crate::filemanager1::serve(requests);
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                while let Ok(target) = received.try_recv() {
                    let start = StartPlan {
                        target: Some(target),
                        problem: None,
                        ignored: 0,
                    };
                    cx.update(|cx| {
                        let options = files_window_options(cx);
                        cx.spawn(async move |cx| {
                            let _ = cx.open_window(options, |window, cx| {
                                let view = cx.new(|cx| FilesApp::new(window, cx, start));
                                cx.new(|cx| gpui_component::Root::new(view, window, cx))
                            });
                        })
                        .detach();
                    });
                }
            }
        })
        .detach();
    });
}

/// The tailnet and Dropbox, read on a thread every minute so the sidebar
/// never waits for `tailscale` or `dropbox`.
/// The Dropbox folder and its sync word, when there is one.
type DropboxState = Option<(std::path::PathBuf, Option<String>)>;

pub(crate) struct Sharing {
    machines: std::sync::Arc<std::sync::Mutex<Vec<crate::share::Machine>>>,
    dropbox: std::sync::Arc<std::sync::Mutex<DropboxState>>,
    /// The last Taildrop or LocalSend: to whom, and whether it went.
    pub(crate) last: std::sync::Arc<std::sync::Mutex<Option<(String, bool)>>>,
    /// LocalSend devices found the last time a Share menu was opened.
    localsend: std::sync::Arc<crate::localsend::KnownPeers>,
    fingerprint: String,
}

impl Sharing {
    fn start() -> Self {
        let sharing = Self {
            machines: Default::default(),
            dropbox: Default::default(),
            last: Default::default(),
            localsend: Default::default(),
            fingerprint: crate::localsend::fingerprint(),
        };
        let machines = sharing.machines.clone();
        let dropbox = sharing.dropbox.clone();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let _ = std::thread::Builder::new()
            .name("files-sharing".to_string())
            .spawn(move || {
                loop {
                    let found = crate::share::tailnet_machines();
                    if let Ok(mut machines) = machines.lock() {
                        *machines = found;
                    }
                    let folder = home.as_deref().and_then(crate::share::dropbox_folder);
                    let state = folder.map(|folder| {
                        let status = crate::share::dropbox_status(&folder);
                        (folder, status)
                    });
                    if let Ok(mut dropbox) = dropbox.lock() {
                        *dropbox = state;
                    }
                    std::thread::sleep(Duration::from_secs(60));
                }
            });
        sharing
    }

    pub(crate) fn machines(&self) -> Vec<crate::share::Machine> {
        self.machines
            .lock()
            .map(|machines| machines.clone())
            .unwrap_or_default()
    }

    /// LocalSend devices on the network, as last found.
    pub(crate) fn localsend_peers(&self) -> Vec<crate::localsend::Peer> {
        self.localsend.peers()
    }

    /// Changes whenever the LocalSend devices do.
    pub(crate) fn localsend_generation(&self) -> u64 {
        self.localsend.generation()
    }

    /// Looks for LocalSend devices for a second and a half, off the window's
    /// thread. Only when a menu that could send is opened.
    pub(crate) fn discover_localsend(&self) {
        let found = self.localsend.clone();
        let fingerprint = self.fingerprint.clone();
        let _ = std::thread::Builder::new()
            .name("files-localsend".to_string())
            .spawn(move || {
                let answered = crate::localsend::discover(
                    &crate::localsend::alias(),
                    &fingerprint,
                    Duration::from_millis(1_500),
                    |peer| found.found(peer),
                );
                found.finished(answered);
            });
    }

    /// Sends files to a LocalSend device, off the window's thread.
    pub(crate) fn localsend(&self, files: Vec<std::path::PathBuf>, peer: crate::localsend::Peer) {
        let done = self.last.clone();
        let own = crate::localsend::own_info(&crate::localsend::alias(), &self.fingerprint, false);
        std::thread::spawn(move || {
            let sent = crate::localsend::send(&peer, own, &files).is_ok();
            if let Ok(mut last) = done.lock() {
                *last = Some((peer.alias.clone(), sent));
            }
        });
    }

    pub(crate) fn dropbox(&self) -> Option<(std::path::PathBuf, Option<String>)> {
        self.dropbox.lock().ok().and_then(|dropbox| dropbox.clone())
    }
}

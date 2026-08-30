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

use gpui::*;
use gpui_component::{Theme, ThemeMode, input::InputState};
use smol::Timer;

use crate::i18n::Locale;
use crate::keys::{Modifiers, command_for};
use crate::layout::{COMPACT_VIEWPORT_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, visible_rows};
use crate::prefs::PreferenceStore;
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

pub struct FilesApp {
    pub(crate) session: FilesSession,
    pub(crate) theme: ThemeMode,
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
    _pump: Option<Task<()>>,
}

impl FilesApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preference_store = PreferenceStore::from_env();
        let loaded = preference_store.load();
        let preferences = loaded.preferences;
        let directories = files_platform::UserDirectories::from_env();
        let reader = Arc::new(FilesReader::from_env());
        let start = directories
            .home()
            .cloned()
            .unwrap_or_else(|| files_core::Location::Local(files_core::LocalPath::root()));

        let mut session = FilesSession::new(SessionSetup {
            start,
            preferences,
            preference_store,
            bookmark_store: crate::bookmarks::BookmarkStore::from_env(),
            directories,
            mounts: files_platform::MountTable::from_env(),
            reader,
            engine: crate::shared_engine(),
        });
        if let Some(problem) = loaded.problem {
            session.notice = Some(Notice::Key(problem));
        }

        let locale = session.locale;
        let path_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::i18n::copy(locale).path_placeholder)
        });
        let dialog_input = cx.new(|cx| InputState::new(window, cx));

        // Better OS is dark-first. `gpui_component::init` installs the light
        // theme, so the choice is applied once the window exists.
        Theme::change(ThemeMode::Dark, Some(window), cx);

        let mut app = Self {
            session,
            theme: ThemeMode::Dark,
            path_input,
            dialog_input,
            focus_handle: cx.focus_handle(),
            editing_path: false,
            apply_to_remaining: false,
            viewport: size(px(1_200.0), px(800.0)),
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
        let sidebar = if self.compact() {
            64.0
        } else {
            crate::layout::SIDEBAR_WIDTH
        };
        let available = (f32::from(self.viewport.width) - sidebar - 40.0).max(120.0);
        self.session.content.columns(available)
    }

    /// How many rows a page is, for Page Up and Page Down.
    pub(crate) fn page_rows(&self) -> usize {
        visible_rows(
            (f32::from(self.viewport.height) - 200.0).max(120.0),
            self.session.content.row_height(),
        )
    }

    pub(crate) fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.session.set_locale(locale);
        cx.notify();
    }

    pub(crate) fn set_theme(
        &mut self,
        mode: ThemeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme = mode;
        Theme::change(mode, Some(window), cx);
        cx.notify();
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
        // The text fields own their own keys while one is focused.
        if self.editing_path || self.session.dialog.is_some() {
            if event.keystroke.key == "escape" {
                self.editing_path = false;
                self.session.dialog = None;
                cx.notify();
            }
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
            self.editing_path = true;
            self.path_input.update(cx, |state, cx| {
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
        cx.notify();
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
        }
        cx.notify();
    }

    pub(crate) fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.session.dialog = None;
        cx.notify();
    }
}

impl Focusable for FilesApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Opens the window.
pub fn run() {
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1_280.0), px(820.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| FilesApp::new(window, cx));
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })
            .expect("failed to open Better Files window");
        })
        .detach();
    });
}

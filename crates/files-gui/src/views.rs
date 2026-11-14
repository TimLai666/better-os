//! The content area, in both view modes, both virtualized.
//!
//! Both modes are one `uniform_list`: it asks for a range of items and draws
//! only those, so the element count per frame is a screenful whether the
//! directory holds twenty entries or a hundred thousand. The list mode's item
//! is one row; the grid mode's item is one row of tiles, which is why the grid
//! is virtualized in the same sense the list is rather than being a wrapping
//! flex that builds every tile.
//!
//! Rows are drawn the WinUI way (`ListViewItem`): a subtle fill under the
//! pointer and while selected, and a 3 by 16 accent bar at a selected row's
//! leading edge, never an outline. Each entry shows the icon theme's own icon
//! for its type, found once per type and kept.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use better_ui::fluent::{self, BODY, CAPTION, Interaction};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName, InteractiveElementExt as _};

use crate::app::FilesApp;
use crate::content::{
    FallbackIcon, ListColumn, SelectionInput, empty_state, header_click, icon_candidates,
    rendered_row_named, status_line, unlistable_reason,
};
use crate::context_menu::MenuTarget;
use crate::i18n::copy;
use crate::keys::Focus;
use crate::prefs::ViewMode;

/// A rubber band in progress.
pub struct Band {
    pub start: (f32, f32),
    pub now: (f32, f32),
    pub base: Vec<files_core::EntryId>,
}

/// What a drag from the content area carries.
///
/// A location, not a path: dropping it on the navigation pane writes a
/// bookmark for the location, and a location is what a bookmark is made of.
#[derive(Clone, Debug)]
pub struct DraggedEntry {
    pub location: files_core::Location,
    pub label: String,
    /// Only a folder can be pinned (the GTK bookmarks file is a list of
    /// folders); a file is dragged elsewhere but never dropped on Favourites.
    pub is_folder: bool,
}

/// The little card that follows the pointer during a drag.
pub struct DragPreview {
    pub(crate) label: String,
}

/// A favourite being dragged to a new place in its section (ticket 121):
/// its position, and its name for the card under the pointer.
#[derive(Clone, Debug)]
pub struct FavouriteDrag {
    pub index: usize,
    pub label: String,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        fluent::menu::panel_frame(div(), p)
            .px(px(12.0))
            .py(px(6.0))
            .child(fluent::label(self.label.clone(), BODY, p.text_primary))
    }
}

/// Real thumbnails for images, from the shared freedesktop cache, made on a
/// worker thread when the cache has none, so scrolling never waits on a
/// decode.
pub struct Thumbnails {
    cache: Option<files_preview::thumbnails::ThumbnailCache>,
    /// What is known per file: a thumbnail path, or `None` for a file that
    /// has none (failed, or being made).
    known: std::sync::Arc<std::sync::Mutex<HashMap<PathBuf, Option<PathBuf>>>>,
    queue: Option<std::sync::mpsc::Sender<(PathBuf, String)>>,
    /// Set by the worker when it made one, so the window redraws.
    pub(crate) ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Thumbnails {
    pub fn from_env() -> Self {
        let thumbnailers = files_preview::thumbnails::load_thumbnailers(
            &files_platform::xdg::data_dirs_from_env(),
        );
        Self::with(
            files_preview::thumbnails::ThumbnailCache::from_env(),
            thumbnailers,
        )
    }

    #[cfg(test)]
    pub fn with_cache(cache: Option<files_preview::thumbnails::ThumbnailCache>) -> Self {
        Self::with(cache, Vec::new())
    }

    /// A cache, and the installed thumbnailers for the types Better Files
    /// cannot decode itself (videos, PDFs, fonts…).
    pub fn with(
        cache: Option<files_preview::thumbnails::ThumbnailCache>,
        thumbnailers: Vec<files_preview::thumbnails::Thumbnailer>,
    ) -> Self {
        use std::sync::{Arc, Mutex, atomic::AtomicBool, mpsc};
        let known: Arc<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = Arc::default();
        let ready = Arc::new(AtomicBool::new(false));
        let queue = cache.clone().and_then(|cache| {
            let (sender, receiver) = mpsc::channel::<(PathBuf, String)>();
            let known = Arc::clone(&known);
            let ready = Arc::clone(&ready);
            std::thread::Builder::new()
                .name("files-thumbnails".to_string())
                .spawn(move || {
                    for (file, mime) in receiver {
                        let size = files_preview::thumbnails::ThumbSize::Large;
                        // The four image types decoded here; anything else
                        // goes to an installed thumbnailer, which is given
                        // ten seconds.
                        let decodable = matches!(
                            mime.as_str(),
                            "image/png" | "image/jpeg" | "image/gif" | "image/bmp"
                        );
                        let made = if decodable {
                            cache.make(&file, size)
                        } else {
                            cache.make_with(&file, &mime, size, &thumbnailers)
                        };
                        if let Ok(mut known) = known.lock() {
                            known.insert(file, made.clone());
                        }
                        if made.is_some() {
                            ready.store(true, std::sync::atomic::Ordering::Release);
                        }
                    }
                })
                .ok()
                .map(|_| sender)
        });
        Self {
            cache,
            known,
            queue,
            ready,
        }
    }

    /// The thumbnail of an image file, when there is one now. Asks the worker
    /// to make one the first time a file has none.
    pub fn for_file(&self, file: &std::path::Path, mime: &str) -> Option<PathBuf> {
        use files_preview::thumbnails::{Lookup, ThumbSize};
        let cache = self.cache.as_ref()?;
        if let Some(known) = self.known.lock().ok()?.get(file) {
            return known.clone();
        }
        let answer = match cache.lookup(file, ThumbSize::Large) {
            Lookup::Found(path) => Some(path),
            Lookup::Failed => None,
            Lookup::Missing => {
                if let Some(queue) = &self.queue {
                    let _ = queue.send((file.to_path_buf(), mime.to_string()));
                }
                None
            }
        };
        if let Ok(mut known) = self.known.lock() {
            known.insert(file.to_path_buf(), answer.clone());
        }
        answer
    }
}

/// Every icon lookup (ticket 121, after review): themed names for files and
/// folders and applications' own `Icon=` values, all resolved on one thread
/// of their own and cached, hits and misses alike. The window draws a
/// fallback until an answer arrives, and never reads an icon directory while
/// drawing. The thread also notices the desktop's icon theme changing (it
/// asks every ten seconds, with or without the frame service) and starts
/// over in the new one.
pub struct IconCache {
    known: std::sync::Arc<std::sync::Mutex<IconAnswers>>,
    asked: RefCell<std::collections::HashSet<IconQuestion>>,
    queue: Option<std::sync::mpsc::Sender<IconQuestion>>,
    /// Set when an answer arrived, so the window draws again.
    pub ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Bumped when the theme changed and every answer was dropped.
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    seen_generation: std::cell::Cell<u64>,
}

/// What one lookup asks.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum IconQuestion {
    /// The first of these (context, name) pairs the theme has.
    Themed(Vec<(&'static str, String)>),
    /// An application's `Icon=` name.
    Application(String),
    /// An application's `Icon=` path, kept as bytes: a path that is not
    /// UTF-8 is still that path.
    ApplicationPath(PathBuf),
}

type IconAnswers = HashMap<IconQuestion, Option<PathBuf>>;

/// How often the resolving thread asks which icon theme the desktop uses.
pub const ICON_THEME_CHECK: std::time::Duration = std::time::Duration::from_secs(10);

/// The search a window's icons are looked up in: the desktop's own theme
/// (and what it inherits), then Adwaita and Yaru, then `hicolor`.
fn session_search() -> better_ui::icons::IconSearch {
    let mut search = better_ui::icons::IconSearch::from_environment();
    for fallback in ["Adwaita", "Yaru"] {
        if !search.themes.iter().any(|theme| theme == fallback) {
            search.themes.push(fallback.to_string());
        }
    }
    search
}

impl IconCache {
    pub fn from_env() -> Self {
        Self::start(session_search, ICON_THEME_CHECK)
    }

    /// Starts the resolving thread. `search` builds the search (again, when
    /// its themes change); `check` is how often that is asked.
    pub fn start(
        search: impl Fn() -> better_ui::icons::IconSearch + Send + 'static,
        check: std::time::Duration,
    ) -> Self {
        use std::sync::{Arc, Mutex, atomic::AtomicBool, atomic::AtomicU64, mpsc};
        let known: Arc<Mutex<IconAnswers>> = Arc::default();
        let ready = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let (sender, receiver) = mpsc::channel::<IconQuestion>();
        let queue = {
            let (known, ready, generation) = (
                Arc::clone(&known),
                Arc::clone(&ready),
                Arc::clone(&generation),
            );
            std::thread::Builder::new()
                .name("files-icons".to_string())
                .spawn(move || {
                    let mut current = search();
                    let mut checked = std::time::Instant::now();
                    loop {
                        let question = match receiver.recv_timeout(check) {
                            Ok(question) => Some(question),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        };
                        if checked.elapsed() >= check {
                            checked = std::time::Instant::now();
                            let fresh = search();
                            if fresh.themes != current.themes {
                                current = fresh;
                                if let Ok(mut known) = known.lock() {
                                    known.clear();
                                }
                                generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                                ready.store(true, std::sync::atomic::Ordering::Release);
                            }
                        }
                        let Some(question) = question else { continue };
                        let answer = match &question {
                            IconQuestion::Themed(candidates) => candidates
                                .iter()
                                .find_map(|(context, name)| current.resolve_in(context, name)),
                            IconQuestion::Application(icon) => {
                                application_icon_file(&current, icon)
                            }
                            IconQuestion::ApplicationPath(path) => {
                                better_ui::icons::acceptable_icon_file(path).then(|| path.clone())
                            }
                        };
                        if let Ok(mut known) = known.lock() {
                            known.insert(question, answer);
                        }
                        ready.store(true, std::sync::atomic::Ordering::Release);
                    }
                })
                .ok()
                .map(|_| sender)
        };
        Self {
            known,
            asked: RefCell::default(),
            queue,
            ready,
            generation,
            seen_generation: std::cell::Cell::new(0),
        }
    }

    /// The answer when it is known; asks for it otherwise.
    fn answer(&self, question: IconQuestion) -> Option<PathBuf> {
        let generation = self.generation.load(std::sync::atomic::Ordering::Acquire);
        if generation != self.seen_generation.get() {
            // The theme changed: everything is asked again.
            self.seen_generation.set(generation);
            self.asked.borrow_mut().clear();
        }
        if let Some(found) = self
            .known
            .lock()
            .ok()
            .and_then(|known| known.get(&question).cloned())
        {
            return found;
        }
        if self.asked.borrow_mut().insert(question.clone())
            && let Some(queue) = &self.queue
        {
            let _ = queue.send(question);
        }
        None
    }

    /// The first candidate the theme has, once looked up.
    pub fn first(&self, candidates: &[(&'static str, String)]) -> Option<PathBuf> {
        if candidates.is_empty() {
            return None;
        }
        self.answer(IconQuestion::Themed(candidates.to_vec()))
    }

    /// An application's own icon, once resolved.
    pub fn application(&self, icon: &app_catalog_core::IconReference) -> Option<PathBuf> {
        self.answer(match icon {
            app_catalog_core::IconReference::Name(name) => IconQuestion::Application(name.clone()),
            app_catalog_core::IconReference::Path(path) => {
                IconQuestion::ApplicationPath(path.clone())
            }
        })
    }

    /// Whether an answer arrived since the last call.
    pub fn take_ready(&self) -> bool {
        self.ready.swap(false, std::sync::atomic::Ordering::AcqRel)
    }
}

/// The file an application's `Icon=` names, by the shared lookup. Reads
/// the disk: only the resolving thread and tests call it.
pub fn application_icon_file(search: &better_ui::icons::IconSearch, icon: &str) -> Option<PathBuf> {
    search.resolve(icon)
}

/// The first of these (context, name) pairs the icon theme has.
pub(crate) type ThemeLookup<'a> = dyn Fn(&[(&'static str, String)]) -> Option<PathBuf> + 'a;

/// A folder's icon in the content view: the theme's own icon for what it
/// is when the theme has one; otherwise the theme's plain folder, with the
/// glyph's mark on it (the developer folder's `</>`).
pub(crate) fn folder_row_icon(
    glyph: crate::icons::FolderGlyph,
    first: &ThemeLookup<'_>,
) -> RowIcon {
    let mut names = glyph.theme_names();
    let plain = names.pop();
    if let Some(own) = first(&names) {
        return RowIcon {
            file: Some(own),
            emblem: None,
        };
    }
    RowIcon {
        file: plain.and_then(|plain| first(&[plain])),
        emblem: glyph.emblem(),
    }
}

/// The icon a content row draws: a file, and a mark on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowIcon {
    pub file: Option<PathBuf>,
    pub emblem: Option<&'static str>,
}

impl FilesApp {
    /// What a row in the content view draws: an application's own icon; a
    /// folder's icon by what it is (ticket 120: its themed icon, or the plain
    /// folder with an emblem); a file's by its type.
    pub(crate) fn row_icon(
        &self,
        entry: &files_core::Entry,
        icon_names: &dyn Fn(&str) -> Vec<String>,
    ) -> RowIcon {
        if let files_core::EntryBody::Application(facts) = &entry.body {
            // Its own icon; until that is found, or when it cannot be, the
            // theme's generic application icon.
            let own = facts
                .icon
                .as_ref()
                .and_then(|icon| self.icons.application(icon));
            return RowIcon {
                file: own.or_else(|| self.icons.first(&icon_candidates(entry, icon_names))),
                emblem: None,
            };
        }
        if entry.kind == files_core::EntryKind::Directory
            && let Some(path) = entry.as_local_path()
        {
            let glyph = self
                .session
                .folder_glyph(&files_core::Location::Local(path.clone()));
            return folder_row_icon(glyph, &|names| self.icons.first(names));
        }
        RowIcon {
            file: self.icons.first(&icon_candidates(entry, icon_names)),
            emblem: None,
        }
    }
}

impl FilesApp {
    /// The whole content area: the header, the virtualized body, and the
    /// status line.
    pub(crate) fn content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let column = self.content_column(cx);
        if self.session.preferences.view_mode != ViewMode::Columns {
            return column;
        }
        // The columns view: the folders above, this folder, and what the
        // selected folder holds.
        let p = fluent::palette(cx);
        let show_hidden = self.session.preferences.show_hidden;
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let here = self
            .session
            .location()
            .as_local_path()
            .map(|path| path.as_path().to_path_buf());
        let mut row = div().flex().flex_1().min_w_0().min_h_0();
        if let Some(here) = &here {
            for folder in crate::columns::ancestor_columns(here, home.as_deref()) {
                let along = here
                    .ancestors()
                    .find(|path| path.parent() == Some(folder.as_path()))
                    .map(std::path::Path::to_path_buf);
                row = row.child(self.side_column(&folder, along.as_deref(), show_hidden, cx));
            }
        }
        row = row.child(
            div()
                .w(px(300.0))
                .flex_none()
                .flex()
                .border_r_1()
                .border_color(p.divider)
                .child(column),
        );
        let opened = self
            .session
            .focused_entry()
            .filter(|entry| entry.kind == files_core::EntryKind::Directory)
            .and_then(|entry| entry.as_local_path())
            .map(|path| path.as_path().to_path_buf());
        if let Some(opened) = opened {
            row = row.child(self.side_column(&opened, None, show_hidden, cx));
        }
        div()
            .id("columns-view")
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_x_scroll()
            .child(row)
            .into_any_element()
    }

    /// One side column: a folder's names, the one along the way to this
    /// folder selected. A click on a folder opens it; on a file, opens its
    /// folder with the file selected.
    fn side_column(
        &self,
        folder: &std::path::Path,
        along: Option<&std::path::Path>,
        show_hidden: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = fluent::palette(cx);
        let entries = crate::columns::column_entries(folder, show_hidden);
        div()
            .id(SharedString::from(format!("column-{}", folder.display())))
            .w(px(240.0))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .py(px(4.0))
            .border_r_1()
            .border_color(p.divider)
            .children(entries.into_iter().map(|entry| {
                let selected = along == Some(entry.path.as_path());
                let hover = fluent::list::row_fill(selected, Interaction::PointerOver, p);
                let path = entry.path.clone();
                let is_folder = entry.is_folder;
                let folder_glyph = files_core::Location::local(&entry.path)
                    .map(|location| self.session.folder_glyph(&location))
                    .unwrap_or(crate::icons::FolderGlyph::Folder);
                div()
                    .id(SharedString::from(format!(
                        "column-entry-{}",
                        entry.path.display()
                    )))
                    .relative()
                    .mx(px(4.0))
                    .h(px(32.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(fluent::radius::SM)
                    .bg(fluent::list::row_fill(selected, Interaction::Rest, p))
                    .hover(move |style| style.bg(hover))
                    .when(selected, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(8.0))
                                .child(fluent::list::selection_bar(p, false)),
                        )
                    })
                    .child(
                        if is_folder {
                            Icon::empty().path(folder_glyph.svg())
                        } else {
                            Icon::new(IconName::File)
                        }
                        .size(px(16.0))
                        .text_color(p.text_secondary),
                    )
                    .child(
                        fluent::label(entry.name, BODY, p.text_primary)
                            .flex_1()
                            .truncate(),
                    )
                    .when(is_folder, |row| {
                        row.child(
                            Icon::new(IconName::ChevronRight)
                                .size(px(12.0))
                                .text_color(p.text_secondary),
                        )
                    })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        let (folder, select) = if is_folder {
                            (path.clone(), None)
                        } else {
                            (
                                path.parent()
                                    .map(std::path::Path::to_path_buf)
                                    .unwrap_or_default(),
                                path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned()),
                            )
                        };
                        if let Ok(location) = files_core::Location::local(folder) {
                            app.session.navigate_to(location);
                            if let Some(name) = select {
                                app.session.select_by_name(&name);
                            }
                            app.after_location_change(window);
                            cx.notify();
                        }
                    }))
            }))
            .into_any_element()
    }

    fn content_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let mode = self.session.preferences.view_mode;
        let columns = self.columns();
        let model = self.session.pane().model();
        let searching = self.session.search.is_active();
        let total = self.session.row_count();
        let status = if searching {
            self.session.search.status_line(c)
        } else {
            status_line(model, c)
        };
        let empty = if searching {
            self.session.search.empty_state(c).map(str::to_string)
        } else if !model.name_filter().is_empty() && model.visible_len() == 0 {
            Some(c.no_filter_matches.to_string())
        } else {
            empty_state(model, c).map(str::to_string)
        };
        let unlistable = unlistable_reason(self.session.location(), c).map(str::to_string);
        let message = unlistable.clone().or(empty);

        let rows = match mode {
            ViewMode::List | ViewMode::Columns => total,
            ViewMode::Grid => total.div_ceil(columns.max(1)),
        };

        let filter_bar =
            (self.filtering || !self.session.filter().is_empty()).then(|| self.filter_bar(cx));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .children(filter_bar)
            .when(mode == ViewMode::List && total > 0, |column| {
                column.child(self.list_header(cx))
            })
            .child(
                div()
                    .id("content-body")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    // A press on empty space starts a rubber band; a row or
                    // tile stops the press before it gets here.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, event: &MouseDownEvent, _window, cx| {
                            app.session.focus = Focus::Content;
                            app.context_menu = None;
                            app.begin_band(event.position, event.modifiers.control);
                            if !event.modifiers.control {
                                let columns = app.columns();
                                app.session.apply_selection(SelectionInput::Clear, columns);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _window, cx| {
                        if app.band.is_some() && event.pressed_button == Some(MouseButton::Left) {
                            app.move_band(event.position);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|app, _: &MouseUpEvent, _window, cx| {
                            if app.band.take().is_some() {
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|app, _: &MouseUpEvent, _window, cx| {
                            if app.band.take().is_some() {
                                cx.notify();
                            }
                        }),
                    )
                    // Right-click on empty space: the folder's own menu. A
                    // row stops the event before it gets here.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|app, event: &MouseDownEvent, window, cx| {
                            app.open_content_menu(
                                MenuTarget::Background,
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                    .when_some(message, |element, message| {
                        // One sentence, centred; the status line below says
                        // the count, so the two never repeat each other.
                        element.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(fluent::label(message, BODY, p.text_secondary)),
                        )
                    })
                    .when(unlistable.is_none() && total > 0, |element| {
                        element.child(self.virtual_body(rows, columns, mode, cx))
                    })
                    .children(self.band_overlay(cx)),
            )
            .child(
                div()
                    .w_full()
                    .h(px(28.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(p.divider)
                    .child(fluent::label(status, CAPTION, p.text_secondary)),
            )
            .into_any_element()
    }

    /// The filter field, how many names it lets through, and Clear.
    fn filter_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let status = self
            .session
            .filter_status()
            .map(|(shown, total)| format!("{} {shown} {} {total}", c.filter_showing, c.filter_of));
        div()
            .w_full()
            .px(px(16.0))
            .py(px(8.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b_1()
            .border_color(p.divider)
            .child(div().w(px(280.0)).child(fluent::text_box::TextBox::search(
                "filter-field",
                &self.filter_input,
            )))
            .children(status.map(|status| fluent::label(status, CAPTION, p.text_secondary)))
            .child(div().flex_1())
            .child(
                fluent::button::Button::icon_button(
                    "filter-clear",
                    IconName::Close,
                    c.filter_clear,
                )
                .on_click({
                    let this = cx.entity().downgrade();
                    move |window, cx| {
                        let _ = this.update(cx, |app, cx| app.close_filter(window, cx));
                    }
                }),
            )
            .into_any_element()
    }

    /// The clickable column headers of the detailed list.
    fn list_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let order = self.session.preferences.order();
        let columns = self.list_columns();
        div()
            .w_full()
            .h(px(32.0))
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .border_b_1()
            .border_color(p.divider)
            .children(columns.into_iter().map(|column| {
                let sorted = order.key == column.sort_key();
                let hover = p.subtle_fill;
                sized(
                    div().id(SharedString::from(format!("column-{}", column.key()))),
                    column,
                )
                .h_full()
                .flex()
                .items_center()
                .gap(px(4.0))
                .rounded(fluent::radius::SM)
                .hover(move |style| style.bg(hover))
                .child(fluent::label(column.header(c), CAPTION, p.text_secondary).truncate())
                .when(sorted, |cell| {
                    cell.child(
                        Icon::new(match order.direction {
                            files_core::SortDirection::Ascending => IconName::ChevronUp,
                            files_core::SortDirection::Descending => IconName::ChevronDown,
                        })
                        .size(px(12.0))
                        .text_color(p.text_secondary),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let next = header_click(this.session.preferences.order(), column);
                    this.session.set_order(next);
                    cx.notify();
                }))
            }))
            .into_any_element()
    }

    /// The virtualized body.
    fn virtual_body(
        &self,
        rows: usize,
        columns: usize,
        mode: ViewMode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let locale = self.locale();
        let scale = self.session.preferences.scale;
        let list_columns = std::rc::Rc::new(self.list_columns());
        uniform_list(
            "files-content",
            rows,
            move |range: std::ops::Range<usize>, _window, cx: &mut App| {
                let c = copy(locale);
                let app = entity.read(cx);
                let model = app.session.pane().model();
                let cursor = app.session.content.cursor();
                let names = app.type_names.clone();
                let describe = |mime: &str| names.describe(mime);
                let icon_names = |mime: &str| names.icon_names(mime);
                let mut out = Vec::with_capacity(range.len());
                for row in range {
                    match mode {
                        ViewMode::List | ViewMode::Columns => {
                            let Some(entry) = app.session.entry_at(row) else {
                                out.push(div().into_any_element());
                                continue;
                            };
                            let rendered = rendered_row_named(
                                entry,
                                model.selection().contains(&entry.id()),
                                cursor == Some(row),
                                c,
                                &describe,
                            );
                            let icon = app.row_icon(entry, &icon_names);
                            let location = entry
                                .as_local_path()
                                .cloned()
                                .map(files_core::Location::Local);
                            out.push(list_row(
                                &entity,
                                row,
                                rendered,
                                icon,
                                location,
                                (scale, &list_columns),
                                cx,
                            ));
                        }
                        ViewMode::Grid => {
                            let start = row * columns;
                            let mut tiles = Vec::new();
                            for offset in 0..columns {
                                let index = start + offset;
                                let Some(entry) = app.session.entry_at(index) else {
                                    break;
                                };
                                let rendered = rendered_row_named(
                                    entry,
                                    model.selection().contains(&entry.id()),
                                    cursor == Some(index),
                                    c,
                                    &describe,
                                );
                                let thumbnail = entry.mime.as_ref().and_then(|mime| {
                                    let path = entry.as_local_path()?;
                                    app.thumbnails.for_file(path.as_path(), mime.as_str())
                                });
                                let icon = match thumbnail {
                                    Some(file) => RowIcon {
                                        file: Some(file),
                                        emblem: None,
                                    },
                                    None => app.row_icon(entry, &icon_names),
                                };
                                let location = entry
                                    .as_local_path()
                                    .cloned()
                                    .map(files_core::Location::Local);
                                tiles.push(grid_tile(
                                    &entity, index, rendered, icon, location, scale, cx,
                                ));
                            }
                            out.push(
                                div()
                                    .w_full()
                                    .flex()
                                    .gap(px(crate::content::GRID_GAP))
                                    .px(px(16.0))
                                    .pt(px(crate::content::GRID_GAP))
                                    .children(tiles)
                                    .into_any_element(),
                            );
                        }
                    }
                }
                out
            },
        )
        .track_scroll(&self.content_scroll)
        .h_full()
        .track_scroll(&self.content_scroll)
        .into_any_element()
    }

    /// The pointer in the content's own coordinates: the list's top-left is
    /// (0, 0) however far it is scrolled.
    fn content_point(&self, position: Point<Pixels>) -> (f32, f32) {
        let handle = self.content_scroll.0.borrow();
        let bounds = handle.base_handle.bounds();
        let offset = handle.base_handle.offset();
        (
            f32::from(position.x - bounds.origin.x - offset.x),
            f32::from(position.y - bounds.origin.y - offset.y),
        )
    }

    fn band_layout(&self) -> crate::content::BandLayout {
        let scale = self.session.preferences.scale;
        match self.session.preferences.view_mode {
            ViewMode::Grid => crate::content::BandLayout::Grid {
                columns: self.columns(),
                tile: scale.tile_size(),
                gap: crate::content::GRID_GAP,
                pad: 16.0,
            },
            ViewMode::List | ViewMode::Columns => crate::content::BandLayout::List {
                row: scale.row_height(),
            },
        }
    }

    pub(crate) fn begin_band(&mut self, position: Point<Pixels>, extend: bool) {
        let start = self.content_point(position);
        let base = if extend {
            self.session
                .selected_entries()
                .iter()
                .map(|entry| entry.id())
                .collect()
        } else {
            Vec::new()
        };
        self.band = Some(Band {
            start,
            now: start,
            base,
        });
    }

    pub(crate) fn move_band(&mut self, position: Point<Pixels>) {
        let now = self.content_point(position);
        let layout = self.band_layout();
        let count = self.session.row_count();
        let Some(band) = self.band.as_mut() else {
            return;
        };
        band.now = now;
        let hits = crate::content::band_hits(band.start, band.now, layout, count);
        let base = band.base.clone();
        self.session.select_rows(&hits, &base);
    }

    /// The band, drawn in the list's viewport.
    fn band_overlay(&self, cx: &App) -> Option<AnyElement> {
        let band = self.band.as_ref()?;
        if (band.start.0 - band.now.0).abs() < 3.0 && (band.start.1 - band.now.1).abs() < 3.0 {
            return None;
        }
        let p = fluent::palette(cx);
        let offset = self.content_scroll.0.borrow().base_handle.offset();
        let left = band.start.0.min(band.now.0) + f32::from(offset.x);
        let top = band.start.1.min(band.now.1) + f32::from(offset.y);
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px((band.start.0 - band.now.0).abs()))
                .h(px((band.start.1 - band.now.1).abs()))
                .border_1()
                .border_color(p.accent)
                .bg(fluent::with_alpha(p.accent, 0.15))
                .into_any_element(),
        )
    }
}

/// How wide a column is drawn: the name takes what the others leave.
fn sized<E: Styled>(element: E, column: ListColumn) -> E {
    if column == ListColumn::Name {
        element
            .flex_1()
            .min_w(px(crate::content::MIN_NAME_WIDTH - 40.0))
    } else {
        element.w(px(column.width())).flex_shrink_0()
    }
}

impl FilesApp {
    /// The list columns that fit beside the navigation pane and side panel.
    pub(crate) fn list_columns(&self) -> Vec<ListColumn> {
        // A column of the columns view is names only.
        if self.session.preferences.view_mode == ViewMode::Columns {
            return vec![ListColumn::Name];
        }
        crate::content::visible_columns(self.content_width() - 48.0)
    }

    /// The content area's width: the pane less the navigation pane and
    /// every side panel that is drawn.
    pub(crate) fn content_width(&self) -> f32 {
        self.pane_layout().content
    }
}

fn entry_icon(icon: RowIcon, fallback: FallbackIcon, size: f32, p: &fluent::Palette) -> AnyElement {
    let color = p.text_secondary;
    let fallback_icon = move || {
        Icon::new(match fallback {
            FallbackIcon::Folder => IconName::Folder,
            FallbackIcon::File => IconName::File,
            FallbackIcon::Application => IconName::LayoutDashboard,
        })
        .size(px(size))
        .text_color(color)
        .into_any_element()
    };
    let picture = match icon.file {
        Some(path) => img(path)
            .size(px(size))
            .flex_none()
            .object_fit(ObjectFit::Contain)
            .with_fallback(fallback_icon)
            .into_any_element(),
        None => fallback_icon(),
    };
    let Some(emblem) = icon.emblem else {
        return picture;
    };
    // The mark sits on the icon's lower right on a disc of the dialog fill,
    // half the icon's size, so it reads at every scale.
    // Never smaller than 14px, so the brackets read on a 16px list icon.
    let mark = (size * 0.5).max(14.0);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(picture)
        .child(
            div()
                .absolute()
                .right(px(-mark * 0.15))
                .bottom(px(-mark * 0.1))
                .size(px(mark))
                .rounded_full()
                // Opaque, so the mark reads over the folder in both
                // appearances.
                .bg(p.dialog)
                .border_1()
                .border_color(p.stroke)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::empty()
                        .path(emblem)
                        .size(px(mark * 0.7))
                        .text_color(p.text_primary),
                ),
        )
        .into_any_element()
}

/// One row of the detailed list.
fn list_row(
    entity: &Entity<FilesApp>,
    index: usize,
    row: crate::content::RenderedRow,
    icon: RowIcon,
    location: Option<files_core::Location>,
    (scale, columns): (crate::prefs::ItemScale, &[ListColumn]),
    cx: &App,
) -> AnyElement {
    let p = fluent::palette(cx);
    let selected = row.selected;
    let hover = fluent::list::row_fill(selected, Interaction::PointerOver, p);
    let pressed = fluent::list::row_fill(selected, Interaction::Pressed, p);
    let secondary = if row.hidden {
        p.text_tertiary
    } else {
        p.text_secondary
    };
    let primary = if row.hidden {
        p.text_secondary
    } else {
        p.text_primary
    };
    let icon_size = if scale == crate::prefs::ItemScale::Large {
        24.0
    } else {
        16.0
    };
    let mut element = div()
        .id(SharedString::from(format!("row-{index}")))
        .relative()
        .w_full()
        .h(px(scale.row_height()))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(fluent::radius::SM)
        .bg(fluent::list::row_fill(selected, Interaction::Rest, p))
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(pressed))
        .when(selected, |element| {
            element.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px((scale.row_height() - 16.0) / 2.0))
                    .child(fluent::list::selection_bar(p, false)),
            )
        })
        .when(row.focused && !selected, |element| {
            element.border_1().border_color(p.stroke)
        })
        .child(
            sized(div(), ListColumn::Name)
                .flex()
                .gap(px(12.0))
                .items_center()
                .overflow_hidden()
                .child(entry_icon(icon, row.fallback, icon_size, p))
                .child(fluent::label(row.name.clone(), BODY, primary).truncate()),
        )
        .children(
            columns
                .iter()
                .filter(|c| **c != ListColumn::Name)
                .map(|column| {
                    let text = match column {
                        ListColumn::Size => row.size.clone(),
                        ListColumn::Modified => row.modified.clone(),
                        ListColumn::Type => row.type_label.clone(),
                        _ => row.extension.clone(),
                    };
                    cell(text, *column, secondary)
                }),
        );

    let is_folder = row.kind == files_core::EntryKind::Directory;
    element = attach_row_handlers(
        element,
        entity,
        index,
        location,
        (row.name, is_folder),
        row.selected,
    );
    // The row is inset 4px from the list's sides, as a WinUI list row is,
    // and as wide as the list, so its cells line up under the headers.
    div().w_full().px(px(4.0)).child(element).into_any_element()
}

/// One tile of the icon grid.
fn grid_tile(
    entity: &Entity<FilesApp>,
    index: usize,
    row: crate::content::RenderedRow,
    icon: RowIcon,
    location: Option<files_core::Location>,
    scale: crate::prefs::ItemScale,
    cx: &App,
) -> AnyElement {
    let p = fluent::palette(cx);
    let tile = scale.tile_size();
    let selected = row.selected;
    let hover = fluent::list::row_fill(selected, Interaction::PointerOver, p);
    let pressed = fluent::list::row_fill(selected, Interaction::Pressed, p);
    let primary = if row.hidden {
        p.text_secondary
    } else {
        p.text_primary
    };
    let mut element = div()
        .id(SharedString::from(format!("tile-{index}")))
        .w(px(tile))
        .h(px(tile))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .p(px(8.0))
        .rounded(fluent::radius::MD)
        .bg(fluent::list::row_fill(selected, Interaction::Rest, p))
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(pressed))
        // A selected tile is filled and edged in the accent, as WinUI's
        // `GridViewItem` is; a tile the keyboard is on but has not selected
        // gets the neutral edge.
        .when(selected, |element| {
            element.border_2().border_color(p.accent)
        })
        .when(row.focused && !selected, |element| {
            element.border_1().border_color(p.stroke)
        })
        .child(entry_icon(icon, row.fallback, scale.icon_size(), p))
        .child(
            fluent::label(row.name.clone(), CAPTION, primary)
                .w_full()
                .text_center()
                .truncate(),
        )
        // Size and date under the name, except on the smallest tiles, which
        // have no room for a second line.
        .when(scale != crate::prefs::ItemScale::Small, |tile| {
            let caption = crate::content::tile_caption(&row);
            tile.gap(px(4.0)).child(
                fluent::label(caption, CAPTION, p.text_secondary)
                    .w_full()
                    .text_center()
                    .truncate(),
            )
        });

    let is_folder = row.kind == files_core::EntryKind::Directory;
    element = attach_row_handlers(
        element,
        entity,
        index,
        location,
        (row.name, is_folder),
        row.selected,
    );
    element.into_any_element()
}

fn cell(text: String, column: ListColumn, color: Rgba) -> Div {
    sized(fluent::label(text, BODY, color), column).truncate()
}

/// Click, double-click, the context menu, and drag, shared by both view
/// modes.
fn attach_row_handlers(
    element: Stateful<Div>,
    entity: &Entity<FilesApp>,
    index: usize,
    location: Option<files_core::Location>,
    (label, is_folder): (String, bool),
    selected: bool,
) -> Stateful<Div> {
    let clicked = entity.clone();
    let opened = entity.clone();
    let menu = entity.clone();
    let element = element
        // The press is the row's: it does not start a rubber band.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, _window, cx| {
            let input = if event.modifiers().control || event.modifiers().alt {
                SelectionInput::ToggleClick(index)
            } else if event.modifiers().shift {
                SelectionInput::RangeClick(index)
            } else {
                SelectionInput::Click(index)
            };
            clicked.update(cx, |app, cx| {
                app.session.focus = Focus::Content;
                app.context_menu = None;
                let columns = app.columns();
                // A running search draws its hits, so the row number on screen
                // is translated to the index the selection works in.
                if let Some(model_row) = app.session.model_row(index) {
                    app.session.apply_selection(input.at(model_row), columns);
                }
                cx.notify();
            });
        })
        .on_double_click(move |_event, window, cx| {
            opened.update(cx, |app, cx| {
                app.session.open_row(index);
                app.after_location_change(window);
                cx.notify();
            });
        })
        // Right-click opens the menu for the selection, selecting the row
        // first when it was not part of it, as every file manager does.
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            cx.stop_propagation();
            menu.update(cx, |app, cx| {
                app.session.focus = Focus::Content;
                if !selected {
                    let columns = app.columns();
                    if let Some(model_row) = app.session.model_row(index) {
                        app.session
                            .apply_selection(SelectionInput::Click(model_row), columns);
                    }
                }
                app.open_content_menu(MenuTarget::Selection, event.position, window, cx);
            });
        });

    match location {
        Some(location) => element.on_drag(
            DraggedEntry {
                location,
                label: label.clone(),
                is_folder,
            },
            move |dragged, _offset, _window, cx| {
                let label = dragged.label.clone();
                cx.new(|_| DragPreview { label })
            },
        ),
        None => element,
    }
}

//! The overlay itself.
//!
//! One GPUI view: a search row fixed near the top and the application area
//! below it, in one window for the whole interaction. It renders
//! [`crate::model::OverlayModel`] and routes input back into it. It decides
//! nothing — not what matches, not what ranks, not what a failure means.
//!
//! Two things happen off the render thread. Reading every application
//! directory and building the index is a background task, so the window opens
//! and the search row takes focus before the list exists. Watching those
//! directories is a second background task that blocks on the kernel's
//! notifications rather than re-reading on a timer; when one arrives the index
//! is rebuilt and swapped in under the query the user already typed.

use std::sync::Arc;
use std::time::Duration;

use app_catalog_core::DesktopId;
use app_catalog_platform::SessionEnvironment;
use better_ui::TileStyle;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{
    Input, InputEvent, InputState, MoveDown, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveUp,
};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, Icon, IconName, *};
use launcher_platform::catalog::{LauncherSnapshot, MetadataWatch, load_snapshot};
use launcher_platform::{CatalogLauncher, SessionCapabilities};

use crate::i18n::{Locale, copy};
use crate::model::{Activation, KeyAction, LoadState, Notice, OverlayModel, key_action};
use crate::{TILE_WIDTH, grid_columns};

/// How long a watch waits before re-arming. Long on purpose: the wait is
/// blocked on the kernel's event channel, so a short timeout would be a wake-up
/// that learns nothing. Nothing happens on this task while the directories are
/// idle.
const WATCH_TIMEOUT: Duration = Duration::from_secs(3600);

/// What the overlay tells whoever opened it.
#[derive(Clone, Debug)]
pub enum OverlayEvent {
    /// Escape, or a launch that succeeded. The overlay has finished.
    Closed,
}

impl EventEmitter<OverlayEvent> for LauncherOverlay {}

pub struct LauncherOverlay {
    locale: Locale,
    model: OverlayModel,
    starter: Option<CatalogLauncher>,
    capabilities: SessionCapabilities,
    search: Entity<InputState>,
    focus: FocusHandle,
    _load: Option<Task<()>>,
    _watch: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl LauncherOverlay {
    pub fn new(locale: Locale, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search_placeholder));
        let search_for_callback = search.clone();
        let subscription = cx.subscribe_in(
            &search,
            window,
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = search_for_callback.read(cx).value().to_string();
                    this.model.set_query(value);
                    cx.notify();
                }
            },
        );

        let mut overlay = Self {
            locale,
            model: OverlayModel::new(),
            starter: None,
            capabilities: SessionCapabilities::from_env(),
            search,
            focus: cx.focus_handle(),
            _load: None,
            _watch: None,
            _subscriptions: vec![subscription],
        };
        // Issue #2's first requirement about focus: the search row has it
        // before anything has been read, so someone can start typing into a
        // list that is still loading.
        overlay.search.focus_handle(cx).focus(window, cx);
        overlay.start_load(cx);
        overlay.start_watch(cx);
        overlay
    }

    /// What the current session can offer, for the diagnostics a later ticket
    /// will show and for the degradation rule this build already relies on.
    pub fn capabilities(&self) -> &SessionCapabilities {
        &self.capabilities
    }

    pub fn model(&self) -> &OverlayModel {
        &self.model
    }

    /// Reads the application directories and builds the index, off the render
    /// thread, then swaps the result in.
    fn start_load(&mut self, cx: &mut Context<Self>) {
        let entry_locale = self.locale.entry_locale();
        let work = cx.background_spawn(async move {
            let session = SessionEnvironment::from_env();
            load_snapshot(&session, entry_locale)
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let snapshot = work.await;
            this.update(cx, |this, cx| {
                this.adopt(snapshot, cx);
            })
            .ok();
        }));
    }

    fn adopt(&mut self, snapshot: LauncherSnapshot, cx: &mut Context<Self>) {
        self.starter = Some(CatalogLauncher::new(
            Arc::clone(&snapshot.catalog),
            self.locale.entry_locale(),
        ));
        self.model.apply_snapshot(snapshot);
        crate::startup::mark(
            crate::startup::STAGE_LIBRARY_READY,
            &format!("applications={}", self.model.rows().len()),
        );
        cx.notify();
    }

    /// Waits for the application directories to change, then re-reads them.
    ///
    /// The wait blocks on the watcher's channel, so an idle desktop costs
    /// nothing here. A watcher that could not start is not an error the user
    /// needs to see: the list is simply the one read at open, which is what
    /// every launcher did before inotify existed.
    fn start_watch(&mut self, cx: &mut Context<Self>) {
        let work = cx.background_spawn(async move {
            let session = SessionEnvironment::from_env();
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

    fn launch_selected(&mut self, cx: &mut Context<Self>) {
        let Some(starter) = self.starter.clone() else {
            return;
        };
        match self.model.activate(&starter) {
            Activation::Launched(_) => cx.emit(OverlayEvent::Closed),
            // Both remaining outcomes keep the overlay open. A failed launch
            // that closed the window would be indistinguishable from a
            // successful one.
            Activation::Failed(_) | Activation::NothingSelected => {}
        }
        cx.notify();
    }

    fn select_and_launch(&mut self, desktop_id: DesktopId, cx: &mut Context<Self>) {
        self.model.select_by_id(&desktop_id);
        self.launch_selected(cx);
    }

    /// Handles the keys the overlay owns that reach it as key presses.
    ///
    /// A key the search row binds to one of its own actions never gets here:
    /// GPUI runs a key's binding before any key listener, capture phase
    /// included. Those keys arrive through [`Self::take_key`] from the
    /// captured action instead. Escape and Enter do get here, because the
    /// search row's handlers for them pass the event on.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.take_key(event.keystroke.key.as_str(), window, cx);
    }

    /// Does what [`key_action`] says a key means, tested in `model.rs`, and
    /// stops it there. A key it leaves alone goes on to the search row, which
    /// is why typing keeps working while the arrow keys move through the grid.
    fn take_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let movement = match key_action(key) {
            Some(KeyAction::Close) => {
                cx.emit(OverlayEvent::Closed);
                cx.stop_propagation();
                return;
            }
            Some(KeyAction::Launch) => {
                self.launch_selected(cx);
                cx.stop_propagation();
                return;
            }
            Some(KeyAction::Move(movement)) => movement,
            None => return,
        };
        self.model.set_columns(grid_columns(
            f32::from(window.viewport_size().width),
            window.scale_factor(),
        ));
        self.model.move_selection(movement);
        cx.stop_propagation();
        cx.notify();
    }

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

    fn tile(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let application = &self.model.rows()[index];
        let selected = self.model.selected_index() == Some(index);
        let desktop_id = application.desktop_id.clone();
        // A letter drawn from the application's own name, in Better OS's own
        // colors. No third-party icon theme asset is copied into this build.
        let glyph = application
            .display_name
            .chars()
            .next()
            .map(|character| character.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        let detail = application
            .generic_name
            .clone()
            .unwrap_or_else(|| application.desktop_id.as_str().to_string());

        div()
            .id(SharedString::from(
                application.desktop_id.as_str().to_string(),
            ))
            .w(px(TILE_WIDTH - 12.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_and_launch(desktop_id.clone(), cx);
            }))
            .child(better_ui::application_tile(
                glyph,
                application.display_name.clone(),
                detail,
                Vec::new(),
                self.tile_style(selected, cx),
            ))
            .into_any_element()
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        if self.model.load_state() == LoadState::Loading {
            return better_ui::state_message(
                c.loading_title,
                c.loading_detail,
                cx.theme().foreground,
                cx.theme().muted_foreground,
            )
            .into_any_element();
        }
        if self.model.is_empty_result() {
            return better_ui::state_message(
                c.no_matches_title,
                c.no_matches_detail,
                cx.theme().foreground,
                cx.theme().muted_foreground,
            )
            .into_any_element();
        }
        if self.model.is_empty_library() {
            return better_ui::state_message(
                c.empty_library_title,
                c.empty_library_detail,
                cx.theme().foreground,
                cx.theme().muted_foreground,
            )
            .into_any_element();
        }

        // One flat grid in the index's deterministic order. Category grouping
        // is one of Issue #2's deferred decisions, so this build shows the
        // order rather than inventing a presentation for it.
        div()
            .flex()
            .flex_wrap()
            .w_full()
            .min_w_0()
            .gap_3()
            .children(
                (0..self.model.rows().len())
                    .map(|index| self.tile(index, cx))
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }

    fn status(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let text = match self.model.load_state() {
            LoadState::Loading => c.loading_title.to_string(),
            LoadState::Refreshing => c.refreshing.to_string(),
            LoadState::Ready => {
                let unit = if self.model.is_browsing() {
                    c.library_count
                } else {
                    c.result_count
                };
                format!("{} {unit}", self.model.rows().len())
            }
        };
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .into_any_element()
    }

    fn hints(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .flex_wrap()
            .justify_between()
            .child(self.status(cx))
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.hint_navigate)
                    .child(c.hint_launch)
                    .child(c.hint_close),
            )
            .into_any_element()
    }
}

impl Focusable for LauncherOverlay {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LauncherOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The keyboard and the layout have to agree on how wide a row is, or
        // Down moves somewhere the eye did not follow.
        self.model.set_columns(grid_columns(
            f32::from(window.viewport_size().width),
            window.scale_factor(),
        ));
        let c = copy(self.locale);
        let notice = self.model.notice().cloned();

        v_flex()
            .track_focus(&self.focus)
            .key_context("BetterLauncher")
            .capture_key_down(cx.listener(Self::on_key))
            // The search row binds the plain navigation keys to its own cursor
            // actions, and a binding runs before any key listener, so Left,
            // Right, Home, and End never reached `on_key`. Each action is
            // taken on its way down to the search row and handed over as the
            // key that `gpui_component` binds it to. Up and Down are unbound
            // in a one-line field today and are taken the same way so they do
            // not depend on that. Shift and Ctrl with these keys are other
            // actions and stay with the search row for selecting and jumping
            // words.
            .capture_action(
                cx.listener(|this, _: &MoveLeft, window, cx| this.take_key("left", window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveRight, window, cx| this.take_key("right", window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveUp, window, cx| this.take_key("up", window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveDown, window, cx| this.take_key("down", window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveHome, window, cx| this.take_key("home", window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveEnd, window, cx| this.take_key("end", window, cx)),
            )
            .size_full()
            .min_w_0()
            .gap_4()
            .p_6()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Input::new(&self.search)
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).small()),
            )
            .when_some(notice, |view, notice| {
                let Notice::LaunchFailed(key) = notice;
                view.child(better_ui::notice(
                    format!("{} {key}", c.launch_failed),
                    cx.theme().danger_foreground,
                    cx.theme().danger,
                    cx.theme().radius,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(self.body(cx)),
            )
            .child(self.hints(cx))
    }
}

#[cfg(test)]
mod tests {
    //! The key path itself, through GPUI's real dispatch.
    //!
    //! `key_action` deciding what a key means is not enough: a key only does
    //! that if it reaches the overlay. The search row's own key bindings run
    //! before any key listener, so these tests open the overlay in a headless
    //! window, press keys the way the platform would, and read the selection
    //! and the query back.

    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AnyWindowHandle, AppContext as _, AsyncApp, Bounds, Entity, Keystroke, WindowBounds,
        WindowOptions, point, px, size,
    };
    use gpui_component::Root;

    use super::LauncherOverlay;
    use crate::i18n::Locale;
    use crate::model::tests::library;

    /// What the test saw after each step, read out once the application has
    /// quit so a failed assertion cannot leave a run loop behind.
    #[derive(Debug, Default)]
    struct Observed {
        selections: Vec<(String, Option<usize>)>,
        query: String,
        rows: Vec<String>,
    }

    /// Opens the overlay two tiles wide over the four-application library,
    /// presses each keystroke in turn, and records the selection after each.
    fn press(keys: &'static [&'static str], typed: &'static [&'static str]) -> Observed {
        let observed = Rc::new(RefCell::new(Observed::default()));
        let out = observed.clone();
        gpui_platform::headless().run(move |cx| {
            gpui_component::init(cx);
            // Wide enough for two tiles and no more, so a row has an end.
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(400.0), px(600.0)),
                ))),
                ..WindowOptions::default()
            };
            let slot: Rc<RefCell<Option<Entity<LauncherOverlay>>>> = Rc::default();
            let slot_in_window = slot.clone();
            let window = cx
                .open_window(options, move |window, cx| {
                    let overlay = cx.new(|cx| LauncherOverlay::new(Locale::EnUs, window, cx));
                    *slot_in_window.borrow_mut() = Some(overlay.clone());
                    cx.new(|cx| Root::new(overlay, window, cx))
                })
                .expect("a headless window opens");
            let overlay = slot.borrow_mut().take().expect("the overlay was built");

            cx.spawn(async move |cx| {
                // The library replaces whatever the overlay's own background
                // read finds, and nothing below yields, so it cannot come back.
                cx.update(|cx| overlay.update(cx, |overlay, cx| overlay.adopt(library(), cx)));
                // Through the untyped handle: a typed one would hold the root
                // view while the dispatch redraws it.
                let window: AnyWindowHandle = window.into();
                let step = |key: &str, cx: &mut AsyncApp| {
                    cx.update_window(window, |_, window, cx| {
                        window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                    })
                    .expect("the window is still open");
                };
                for key in keys {
                    step(key, cx);
                    let selected = cx.update(|cx| overlay.read(cx).model().selected_index());
                    out.borrow_mut()
                        .selections
                        .push((key.to_string(), selected));
                }
                for key in typed {
                    step(key, cx);
                }
                cx.update(|cx| {
                    let overlay = overlay.read(cx);
                    let mut out = out.borrow_mut();
                    out.query = overlay.search.read(cx).value().to_string();
                    out.rows = overlay
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
    fn every_navigation_key_reaches_the_grid_through_the_focused_search_row() {
        // Archive Manager  Browser
        // Calculator       Text Editor
        let observed = press(
            &[
                "right", "right", "down", "left", "left", "up", "end", "home",
            ],
            &[],
        );
        let expected = [
            ("right", Some(1)),
            ("right", Some(1)),
            ("down", Some(3)),
            ("left", Some(2)),
            ("left", Some(2)),
            ("up", Some(0)),
            ("end", Some(3)),
            ("home", Some(0)),
        ]
        .map(|(key, index)| (key.to_string(), index));
        assert_eq!(observed.selections, expected);
    }

    #[test]
    fn the_search_row_still_takes_typing_and_modified_arrows_after_the_grid_moves() {
        // Left and Right belong to the grid; Shift+Left still selects text in
        // the search row, so typing over the selection replaces it.
        let observed = press(&[], &["c", "a", "l", "left", "right", "shift-left", "x"]);
        assert_eq!(observed.query, "cax");

        // The trade: a plain Left moves the grid, not the text cursor, so a
        // letter typed after it still lands at the end of the query.
        let observed = press(&[], &["c", "a", "l", "left", "x"]);
        assert_eq!(observed.query, "calx");

        let observed = press(&[], &["c", "a", "l", "backspace", "l"]);
        assert_eq!(observed.query, "cal");
        assert_eq!(observed.rows, vec!["Calculator"]);
    }
}

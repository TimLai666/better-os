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
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, Icon, IconName, *};
use launcher_platform::catalog::{LauncherSnapshot, MetadataWatch, load_snapshot};
use launcher_platform::{CatalogLauncher, SessionCapabilities};

use crate::i18n::{Locale, copy};
use crate::model::{Activation, LoadState, Move, Notice, OverlayModel};
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

    /// Handles the keys the overlay owns, before the search row sees them.
    ///
    /// Only navigation, launching, and closing are taken. Everything else
    /// falls through to the search row, which is why typing keeps working while
    /// the arrow keys move through the grid.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let movement = match event.keystroke.key.as_str() {
            "escape" => {
                cx.emit(OverlayEvent::Closed);
                cx.stop_propagation();
                return;
            }
            "enter" => {
                self.launch_selected(cx);
                cx.stop_propagation();
                return;
            }
            "right" => Move::Next,
            "left" => Move::Previous,
            "down" => Move::NextRow,
            "up" => Move::PreviousRow,
            "home" => Move::First,
            "end" => Move::Last,
            _ => return,
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

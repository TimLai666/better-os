//! Drawing the Graph tab: the filter bar, the find field, and the
//! virtualized list of commit rows with their lanes and labels.
//!
//! Nothing here decides anything; [`crate::graph`] and the session do. The
//! list is a `uniform_list`, so only the rows on screen are built, whatever
//! the history's length. Building them is also where the list notices it is
//! near the end of what is loaded and asks for the next page, and where it
//! remembers which commits were at the top so a refresh can put them back.

use std::ops::Range;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{Icon, IconName};

use better_ui::fluent::button::{Button, ButtonKind};
use better_ui::fluent::chip::Chip;
use better_ui::fluent::info_bar::{InfoBar, Severity};
use better_ui::fluent::lanes::{LANE_WIDTH, LaneCell, LaneLine, LineKind, NodeStyle, lane_hue};
use better_ui::fluent::list::{row_fill, selection_bar};
use better_ui::fluent::menu::{MenuEntry, open_menu};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toggle::{CheckBox, CheckState};
use better_ui::fluent::{
    self, BODY, CAPTION, CODE, Interaction, Palette, label, radius, spacing, with_alpha,
};
use git_core::{EdgeKind, RefKind};
use git_platform::{GraphQuery, GraphScope};

use crate::app::{BRANCH_ICON, ContextTarget, GitApp};
use crate::graph::{Direction, GraphCopy, graph_copy};
use crate::i18n::{Copy, fill};
use crate::session::Repo;

/// A graph row: one line, as a list row in `DESIGN.md`.
pub(crate) const GRAPH_ROW: Pixels = px(36.0);
/// The tag glyph, drawn like [`BRANCH_ICON`] in Lucide's 24px grid, because
/// gpui-component's icon set has none.
pub(crate) const TAG_ICON: &str = "better-git/tag.svg";
pub(crate) const TAG_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12.6 2.6A2 2 0 0 0 11.2 2H4a2 2 0 0 0-2 2v7.2a2 2 0 0 0 .6 1.4l8.7 8.7a2.4 2.4 0 0 0 3.4 0l6.6-6.6a2.4 2.4 0 0 0 0-3.4z"/><circle cx="7.5" cy="7.5" r="1.5"/></svg>"#;
/// The author and date columns' widths.
const AUTHOR_WIDTH: Pixels = px(132.0);
const DATE_WIDTH: Pixels = px(72.0);
const SHA_WIDTH: Pixels = px(64.0);
/// The tallest the drop strip of branches grows while a commit is dragged.
const MAX_STRIP_HEIGHT: f32 = 152.0;
/// The least room the subject keeps when lanes and labels are wide.
const SUBJECT_MIN: Pixels = px(120.0);
/// How much of the window's width the Graph tab's list takes.
pub(crate) const GRAPH_SHARE: f32 = 0.58;
/// How many rows at the top of the list are remembered for a refresh.
const REMEMBERED_ROWS: usize = 8;

/// The Graph tab's window-side state: the list's scroll position, the find
/// field, and what was at the top before the last refresh.
pub(crate) struct GraphUi {
    pub scroll: UniformListScrollHandle,
    pub search: Entity<InputState>,
    /// The commits at the top of the list the last time it was drawn.
    top: Vec<String>,
    /// The replacement the list was last placed for.
    placed: u64,
    /// How many lanes the person dragged the graph column to.
    pub lanes: Option<u16>,
}

impl GraphUi {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<GitApp>,
        g: &'static GraphCopy,
    ) -> (Self, Subscription) {
        let placeholder = g.find_placeholder.to_string();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |this: &mut GitApp, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.session.set_graph_search(&text);
                    cx.notify();
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.session.graph_find(if *shift {
                        Direction::Previous
                    } else {
                        Direction::Next
                    });
                    this.drive(window, cx);
                }
                _ => {}
            },
        );
        (
            GraphUi {
                scroll: UniformListScrollHandle::new(),
                search,
                top: Vec::new(),
                placed: 0,
                lanes: None,
            },
            subscription,
        )
    }
}

/// The graph column's right edge, being dragged.
#[derive(Clone)]
pub(crate) struct LaneDivider;

impl Render for LaneDivider {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Which columns a row has room for in this window.
struct RowLayout {
    author: bool,
    date: bool,
    /// The most lanes the graph column may take and still leave the
    /// subject its least room (one column is kept for the overflow column).
    lane_room: u16,
    /// What is left for the lanes and the labels together, in pixels.
    free: f32,
}

impl RowLayout {
    fn for_window(window: &Window) -> Self {
        let pane = f32::from(window.viewport_size().width * GRAPH_SHARE);
        // A narrow window drops the author, then the date, before the
        // subject has no room left.
        let author = pane >= 720.0;
        let date = pane >= 560.0;
        let gap = f32::from(spacing::SM);
        let mut fixed = LANES_LEFT
            + f32::from(spacing::MD)
            + 4.0
            + f32::from(SUBJECT_MIN)
            + f32::from(SHA_WIDTH)
            + 3.0 * gap;
        if author {
            fixed += f32::from(AUTHOR_WIDTH) + gap;
        }
        if date {
            fixed += f32::from(DATE_WIDTH) + gap;
        }
        let free = (pane - fixed).max(0.0);
        let lane_room = ((free / f32::from(LANE_WIDTH)).floor() - 1.0).max(1.0) as u16;
        Self {
            author,
            date,
            lane_room,
            free,
        }
    }

    /// Room left for the labels beside a graph column of `lanes` (and the
    /// overflow column when some are hidden).
    fn label_room(&self, lanes: u16, overflow: bool) -> f32 {
        let graph = f32::from(LANE_WIDTH) * (lanes + u16::from(overflow)) as f32;
        (self.free - graph - f32::from(spacing::SM)).max(0.0)
    }
}

/// How wide a label chip is: its text measured by the window's own text
/// system at Caption size (Semibold for the emphasized chip), the 12px icon
/// or check mark and its 4px gap, 6px padding either side, the 1px stroke
/// either side, and 2px to spare for rounding.
fn chip_width(window: &Window, name: &str, emphasized: bool) -> f32 {
    let mut font = window.text_style().font();
    if emphasized {
        font.weight = FontWeight::SEMIBOLD;
    }
    let run = TextRun {
        len: name.len(),
        font,
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text = window
        .text_system()
        .shape_line(name.to_string().into(), CAPTION_SIZE, &[run], None)
        .width;
    f32::from(text) + 12.0 + 4.0 + 12.0 + 2.0 + 2.0
}

/// Caption text's size, which chips use.
const CAPTION_SIZE: Pixels = px(12.0);

/// Builds a branch's context menu (the branch list's), for the "+N" menu.
pub(crate) type BranchMenu<'a> =
    dyn Fn(&GitApp, String, &mut Context<GitApp>) -> Vec<MenuEntry> + 'a;

/// The key context gpui-component's text fields set while they have the
/// keyboard.
const TEXT_INPUT_CONTEXT: &str = "Input";

/// The group every graph row's face reacts to the pointer through.
const GRAPH_ROW_GROUP: &str = "graph-row";

/// From a row's left edge to the graph column: the wrapper's inset and the
/// row's own padding.
const LANES_LEFT: f32 = 4.0 + 8.0;

/// A commit being dragged from the graph.
#[derive(Clone)]
pub(crate) struct DraggedGraphCommit {
    pub sha: String,
    pub summary: SharedString,
}

impl Render for DraggedGraphCommit {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        fluent::menu::panel_frame(div(), p)
            .px(spacing::MD)
            .py(spacing::SM)
            .child(label(self.summary.clone(), BODY, p.text_primary))
    }
}

fn caption(text: impl Into<SharedString>, p: &Palette) -> Div {
    label(text, CAPTION, p.text_secondary)
}

/// How long ago, as History's rows say it: `3d`.
fn age(now: i64, then: i64) -> String {
    let (n, unit) = crate::history::age(now, then);
    format!("{n}{unit}")
}

impl GitApp {
    /// Whether Ctrl+F opens the graph's find: the Graph tab is shown, and
    /// the keyboard is with the window itself or already in the find field,
    /// never in another text field.
    pub(crate) fn graph_find_allowed(&self, window: &Window, cx: &App) -> bool {
        let shown = self
            .session
            .repo
            .as_ref()
            .is_some_and(|r| r.tab == crate::session::Tab::Graph)
            && self.session.page == crate::session::Page::Main
            && self.session.dialog.is_none();
        // Anywhere in the window will do (the list, the filter, a check
        // box, the diff) except a text field, whose own Ctrl+F it is; the
        // find field itself is the one text field that may.
        let in_text_field = window
            .context_stack()
            .iter()
            .any(|context| context.contains(TEXT_INPUT_CONTEXT));
        let in_find = self
            .graph
            .search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let free = !in_text_field || in_find;
        shown && free
    }

    /// Opens the find field and puts the cursor in it (Ctrl+F).
    pub(crate) fn open_graph_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session.open_graph_search();
        self.graph
            .search
            .update(cx, |state, cx| state.focus(window, cx));
    }

    /// The left side of the Graph tab, under the tabs.
    pub(crate) fn graph_pane(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let g = graph_copy(self.locale);
        let graph = &repo.graph;
        // After a refresh, put the commits that were at the top back there.
        if graph.replaced != self.graph.placed {
            let place = graph.place_of(&self.graph.top);
            self.graph
                .scroll
                .scroll_to_item_strict(place, ScrollStrategy::Top);
            let replaced = graph.replaced;
            cx.defer_in(window, move |this, _, _| this.graph.placed = replaced);
        }
        let query = graph.query;
        let scope_items = vec![
            ComboItem::new(g.scope_all),
            ComboItem::new(g.scope_local),
            ComboItem::new(g.scope_current),
        ];
        let scope_index = match query.scope {
            GraphScope::All => 0,
            GraphScope::Local => 1,
            GraphScope::Current => 2,
        };
        let filters = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing::SM)
            .px(spacing::MD)
            .pb(spacing::SM)
            .child(
                ComboBox::new("graph-scope", scope_items, Some(scope_index))
                    .min_width(px(160.0))
                    .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                        let scope = match i {
                            1 => GraphScope::Local,
                            2 => GraphScope::Current,
                            _ => GraphScope::All,
                        };
                        this.session.set_graph_query(GraphQuery { scope, ..query });
                    })),
            )
            .child(
                CheckBox::new("graph-remotes", CheckState::from_bool(query.remotes))
                    .label(g.remotes)
                    .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                        this.session.set_graph_query(GraphQuery {
                            remotes: s == CheckState::Checked,
                            ..query
                        });
                    })),
            )
            .child(
                CheckBox::new("graph-tags", CheckState::from_bool(query.tags))
                    .label(g.tags)
                    .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                        this.session.set_graph_query(GraphQuery {
                            tags: s == CheckState::Checked,
                            ..query
                        });
                    })),
            )
            .child(div().flex_1())
            .when(graph.loaded, |d| {
                d.child(caption(
                    fill(
                        if graph.end { g.count } else { g.count_more },
                        &[("n", &graph.commits.len().to_string())],
                    ),
                    p,
                ))
            })
            .child(
                Button::icon_button("graph-find", IconName::Search, g.find)
                    .kind(ButtonKind::Subtle)
                    .tooltip(format!("{} (Ctrl+F)", g.shortcut_find))
                    .on_click(Self::act(cx, |this, window, cx| {
                        this.open_graph_find(window, cx)
                    })),
            );
        let find = graph.search_open.then(|| {
            div()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .px(spacing::MD)
                .pb(spacing::SM)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.session.close_graph_search();
                        this.focus_handle.focus(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(
                    div().flex_1().min_w_0().child(
                        TextBox::search("graph-search", &self.graph.search)
                            .on_search(Self::act(cx, |this, _, _| {
                                this.session.graph_find(Direction::Next)
                            })),
                    ),
                )
                .child(
                    Button::icon_button(
                        "graph-find-previous",
                        IconName::ChevronUp,
                        g.find_previous,
                    )
                    .kind(ButtonKind::Subtle)
                    .on_click(Self::act(cx, |this, _, _| {
                        this.session.graph_find(Direction::Previous)
                    })),
                )
                .child(
                    Button::icon_button("graph-find-next", IconName::ChevronDown, g.find_next)
                        .kind(ButtonKind::Subtle)
                        .on_click(Self::act(cx, |this, _, _| {
                            this.session.graph_find(Direction::Next)
                        })),
                )
                .child(
                    // A word, set apart: the field's own clear button is
                    // the cross.
                    div().ml(spacing::SM).child(
                        Button::standard("graph-find-close", g.find_done)
                            .tooltip(g.find_close)
                            .on_click(Self::act(cx, |this, window, cx| {
                                this.session.close_graph_search();
                                this.focus_handle.focus(window, cx);
                            })),
                    ),
                )
        });
        let find_state = if graph.search_pending.is_some() {
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .px(spacing::MD)
                    .pb(spacing::SM)
                    .child(
                        ProgressRing::new("graph-ring-small", Progress::Indeterminate)
                            .size(px(16.0)),
                    )
                    .child(caption(g.searching, p)),
            )
        } else if graph.not_found && graph.search_open {
            Some(div().px(spacing::MD).pb(spacing::SM).child(label(
                g.not_found,
                CAPTION,
                p.critical,
            )))
        } else {
            None
        };
        let refusal = graph.refusal.map(|refusal| {
            div().px(spacing::MD).pb(spacing::SM).child(
                InfoBar::new("graph-refusal", Severity::Warning, g.refused_title)
                    .message(g.refusal(refusal))
                    .locale(self.words())
                    .on_close(Self::act(cx, |this, _, _| {
                        this.session.dismiss_graph_refusal()
                    })),
            )
        });
        let room = RowLayout::for_window(window).lane_room;
        let lanes = crate::graph::shown_lanes(graph.columns, self.graph.lanes).min(room);
        let body = if !graph.loaded {
            v_center(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(ProgressRing::new("graph-ring", Progress::Indeterminate))
                    .child(label(g.loading, BODY, p.text_secondary)),
            )
            .into_any_element()
        } else if graph.commits.is_empty() {
            v_center(label(g.empty, BODY, p.text_secondary)).into_any_element()
        } else {
            let columns = graph.columns;
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .on_drag_move::<LaneDivider>(cx.listener(
                    move |this, event: &DragMoveEvent<LaneDivider>, _, cx| {
                        let offset =
                            f32::from(event.event.position.x - event.bounds.origin.x) - LANES_LEFT;
                        this.graph.lanes = Some(crate::graph::lanes_at(
                            offset,
                            f32::from(LANE_WIDTH),
                            columns.min(room),
                        ));
                        cx.notify();
                    },
                ))
                .child(
                    uniform_list(
                        "graph-list",
                        graph.commits.len(),
                        cx.processor(move |this, range: Range<usize>, window, cx| {
                            this.graph_rows(range, p, window, cx)
                        }),
                    )
                    .track_scroll(&self.graph.scroll)
                    .flex_1()
                    .w_full(),
                )
                // The graph column's edge: drag it to show more or fewer
                // lanes.
                .when(columns > 1, |d| {
                    d.child(
                        div()
                            .id("graph-lane-divider")
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(LANES_LEFT) + LANE_WIDTH * lanes as f32 - px(3.0))
                            .w(px(6.0))
                            .cursor(CursorStyle::ResizeColumn)
                            .hover(move |s| s.bg(with_alpha(p.text_secondary, 0.2)))
                            .tooltip(better_ui::fluent::dialog::tooltip_builder(
                                g.lanes_hint.into(),
                            ))
                            .on_drag(LaneDivider, |divider, _, _, cx| cx.new(|_| divider.clone())),
                    )
                })
                .into_any_element()
        };
        let dragging = cx.has_active_drag();
        let drop_branches = dragging.then(|| self.graph_branch_strip(repo, p, c, cx));
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(filters)
            .children(find)
            .children(find_state)
            .children(refusal)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(p.divider)
                    .child(body),
            )
            .when(graph.loading && graph.loaded, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::SM)
                        .px(spacing::MD)
                        .py(spacing::XS)
                        .border_t_1()
                        .border_color(p.divider)
                        .child(
                            ProgressRing::new("graph-ring-small", Progress::Indeterminate)
                                .size(px(16.0)),
                        )
                        .child(caption(g.loading_more, p)),
                )
            })
            .when(!graph.commits.is_empty(), |d| {
                d.child(
                    caption(g.drag_hint, p)
                        .p(spacing::MD)
                        .border_t_1()
                        .border_color(p.divider),
                )
            })
            .children(drop_branches.map(|strip| {
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .bg(p.flyout)
                    .border_t_1()
                    .border_color(p.flyout_stroke)
                    .child(strip)
            }))
    }

    /// The rows on screen. Also: ask for the next page near the end, scroll
    /// to a commit the find picked, and remember the top rows.
    fn graph_rows(
        &mut self,
        range: Range<usize>,
        p: &'static Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(repo) = self.session.repo.as_mut() else {
            return Vec::new();
        };
        if let Some(index) = repo.graph.scroll_to.take() {
            self.graph
                .scroll
                .scroll_to_item(index, ScrollStrategy::Center);
            cx.notify();
        }
        let repo = self.session.repo.as_ref().expect("checked above");
        let graph = &repo.graph;
        // A list scrolled to its top stays at the top, so new commits show;
        // one scrolled down keeps its commits in view.
        if graph.replaced == self.graph.placed {
            self.graph.top = if range.start == 0 {
                Vec::new()
            } else {
                graph.commits[range.start.min(graph.commits.len())..]
                    .iter()
                    .take(REMEMBERED_ROWS)
                    .map(|c| c.sha.clone())
                    .collect()
            };
        }
        if graph.wants_more(range.end) {
            cx.defer_in(window, |this, window, cx| {
                this.session.load_more_graph();
                this.drive(window, cx);
            });
        }
        let now = crate::github::now();
        let layout = RowLayout::for_window(window);
        let (show_author, show_date) = (layout.author, layout.date);
        let head = repo.status.head_oid.clone();
        let g_head = graph_copy(self.locale).detached;
        let selected = graph.selected_index();
        let columns = graph.columns;
        let lanes = crate::graph::shown_lanes(columns, self.graph.lanes).min(layout.lane_room);
        // Rows past what is stored are left out by `rows`; the list never
        // asks for them.
        let laid_out = graph.rows.rows(range.clone());
        let mut rows = Vec::with_capacity(range.len());
        for (index, row) in range.zip(laid_out) {
            let Some(commit) = graph.commits.get(index) else {
                continue;
            };
            let lines: Vec<LaneLine> = row
                .edges
                .iter()
                .map(|e| LaneLine {
                    kind: match e.kind {
                        EdgeKind::Pass => LineKind::Pass,
                        EdgeKind::In => LineKind::In,
                        EdgeKind::Out => LineKind::Out,
                    },
                    column: e.column,
                    lane: e.lane,
                })
                .collect();
            let is_head = head.as_deref() == Some(commit.sha.as_str());
            let node = if is_head {
                NodeStyle::Head
            } else if commit.is_merge() {
                NodeStyle::Merge
            } else {
                NodeStyle::Commit
            };
            let is_selected = selected == Some(index);
            let rewritable = self.session.graph_can_rewrite(&commit.sha);
            let sha = commit.sha.clone();
            let summary: SharedString = commit.summary.clone().into();
            // Labels give way before the subject does: they are cut off at
            // their right, and the subject keeps room for a few words.
            let mut chips = div().flex().flex_none().items_center().gap(px(4.0));
            let labels = graph.labels(index);
            let widths: Vec<f32> = labels
                .iter()
                .map(|l| match l.kind {
                    RefKind::Head => chip_width(window, g_head, true),
                    _ => chip_width(window, &l.name, l.current),
                })
                .collect();
            let fit = crate::graph::fit_labels(
                &widths,
                layout.label_room(lanes, columns > lanes),
                chip_width(window, "+99", false),
                4.0,
            );
            let hidden: Vec<git_core::RefLabel> = labels[fit..].to_vec();
            for (i, l) in labels.into_iter().take(fit).enumerate() {
                let hue = lane_hue(row.lane);
                let chip = match l.kind {
                    RefKind::Local => Chip::new(l.name.clone())
                        .icon(Icon::empty().path(BRANCH_ICON))
                        .hue(hue)
                        .emphasized(l.current),
                    RefKind::Remote => Chip::new(l.name.clone()).icon(IconName::Globe).hue(hue),
                    RefKind::Tag => Chip::new(l.name.clone()).icon(Icon::empty().path(TAG_ICON)),
                    RefKind::Head => Chip::new(g_head).hue(hue).emphasized(true),
                };
                let mut holder = div().id(("graph-label", index * 64 + i)).child(chip);
                if matches!(l.kind, RefKind::Local | RefKind::Remote) {
                    let name = l.name.clone();
                    holder = holder.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.context =
                                Some((ContextTarget::Branch(name.clone()), event.position));
                            open_menu(&this.context_menu, None, window, cx);
                            cx.stop_propagation();
                        }),
                    );
                }
                if l.kind == RefKind::Local && !l.current {
                    let onto = l.name.clone();
                    holder = holder
                        .rounded(radius::SM)
                        .drag_over::<DraggedGraphCommit>(move |s, _, _, _| {
                            s.bg(with_alpha(p.accent, 0.25))
                        })
                        .on_drop(cx.listener(
                            move |this, dragged: &DraggedGraphCommit, window, cx| {
                                this.session
                                    .cherry_pick_sha(dragged.sha.clone(), Some(onto.clone()));
                                this.drive(window, cx);
                            },
                        ));
                }
                chips = chips.child(holder);
            }
            if !hidden.is_empty() {
                // The labels that did not fit: how many, and on a click a
                // menu of them, each with its own menu under it.
                let count = hidden.len();
                let open = move |this: &mut GitApp,
                                 event: &MouseDownEvent,
                                 window: &mut Window,
                                 cx: &mut Context<GitApp>| {
                    this.context =
                        Some((ContextTarget::GraphLabels(hidden.clone()), event.position));
                    open_menu(&this.context_menu, None, window, cx);
                    cx.stop_propagation();
                };
                let open = std::rc::Rc::new(open);
                let right = open.clone();
                chips = chips.child(
                    div()
                        .id(("graph-more-labels", index))
                        .cursor_pointer()
                        .child(Chip::new(format!("+{count}")))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                open(this, event, window, cx)
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event, window, cx| {
                                right(this, event, window, cx)
                            }),
                        ),
                );
            }
            let hover = row_fill(is_selected, Interaction::PointerOver, p);
            let pressed = row_fill(is_selected, Interaction::Pressed, p);
            // What the row looks like: inset 4px with rounded corners, as
            // every list row is drawn.
            let face = div()
                .id(("graph-row-face", index))
                .relative()
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(spacing::SM)
                .size_full()
                .pl(spacing::SM)
                .pr(spacing::MD)
                .rounded(radius::SM)
                .bg(row_fill(is_selected, Interaction::Rest, p))
                .group_hover(GRAPH_ROW_GROUP, move |s| s.bg(hover))
                .group_active(GRAPH_ROW_GROUP, move |s| s.bg(pressed))
                .when(is_selected, |r| {
                    r.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px((f32::from(GRAPH_ROW) - 16.0) / 2.0))
                            .child(selection_bar(p, false)),
                    )
                })
                .when(rewritable, |r| {
                    r.group_drag_over::<DraggedGraphCommit>(GRAPH_ROW_GROUP, move |s| {
                        s.border_1().border_color(p.accent)
                    })
                })
                .child(
                    LaneCell::new(row.column, row.lane, lines, columns)
                        .visible(lanes)
                        .node(node),
                )
                .child(chips)
                .child(
                    label(commit.summary.clone(), BODY, p.text_primary)
                        .flex_1()
                        .min_w(SUBJECT_MIN)
                        .truncate(),
                )
                .when(show_author, |d| {
                    d.child(
                        caption(commit.author_name.clone(), p)
                            .flex_none()
                            .w(AUTHOR_WIDTH)
                            .truncate(),
                    )
                })
                .when(show_date, |d| {
                    d.child(
                        caption(age(now, commit.author_time), p)
                            .flex_none()
                            .w(DATE_WIDTH)
                            .truncate(),
                    )
                })
                .child(
                    fluent::text(div(), CODE)
                        .flex_none()
                        .w(SHA_WIDTH)
                        .text_color(p.text_secondary)
                        .child(commit.short_sha().to_string()),
                );
            let (onto, before) = (sha.clone(), sha.clone());
            // Where the row reacts: all of it, the inset margin included, so
            // no click between rows is lost.
            let item = div()
                .id(("graph-row", index))
                .group(GRAPH_ROW_GROUP)
                .relative()
                .w_full()
                .h(GRAPH_ROW)
                .px(px(4.0))
                .child(face)
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if event.is_keyboard() {
                        return;
                    }
                    this.session.select_graph_commit(index);
                    this.drive(window, cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.context = Some((ContextTarget::GraphCommit(index), event.position));
                        open_menu(&this.context_menu, None, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_drag(
                    DraggedGraphCommit {
                        sha: sha.clone(),
                        summary: summary.clone(),
                    },
                    |dragged, _, _, cx| cx.new(|_| dragged.clone()),
                )
                // Dropping on a row squashes; the session says why when it
                // cannot.
                .on_drop(
                    cx.listener(move |this, dragged: &DraggedGraphCommit, window, cx| {
                        this.session.graph_squash(&dragged.sha, &onto);
                        this.drive(window, cx);
                    }),
                )
                // The top edge: put the dragged commit just above this one.
                .child(
                    div()
                        .id(("graph-reorder", index))
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(8.0))
                        .rounded(px(2.0))
                        .when(rewritable, |d| {
                            d.drag_over::<DraggedGraphCommit>(move |s, _, _, _| s.bg(p.accent))
                        })
                        .on_drop(cx.listener(
                            move |this, dragged: &DraggedGraphCommit, window, cx| {
                                this.session.graph_reorder(&dragged.sha, &before);
                                this.drive(window, cx);
                            },
                        )),
                );
            rows.push(item.into_any_element());
        }
        rows
    }

    /// While a commit is dragged: the local branches it can be dropped on to
    /// cherry-pick it there, over the list's foot as History shows them.
    fn graph_branch_strip(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().flex().flex_col().gap(px(2.0));
        for (i, branch) in repo
            .info
            .branches
            .iter()
            .filter(|b| !b.remote && !b.current)
            .enumerate()
        {
            let name = branch.name.clone();
            list = list.child(
                div()
                    .id(("graph-drop-branch", i))
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .h(px(28.0))
                    .px(spacing::MD)
                    .rounded(radius::SM)
                    .child(
                        Icon::empty()
                            .path(BRANCH_ICON)
                            .size(px(12.0))
                            .text_color(p.text_secondary),
                    )
                    .child(label(branch.name.clone(), BODY, p.text_primary))
                    .drag_over::<DraggedGraphCommit>(move |s, _, _, _| {
                        s.bg(with_alpha(p.accent, 0.25))
                    })
                    .on_drop(
                        cx.listener(move |this, dragged: &DraggedGraphCommit, window, cx| {
                            this.session
                                .cherry_pick_sha(dragged.sha.clone(), Some(name.clone()));
                            this.drive(window, cx);
                        }),
                    ),
            );
        }
        // At most a third of the list's height, scrolled within, so many
        // branches never cover the rows a commit can be dropped on.
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(spacing::SM)
            .child(caption(c.cherry_pick_to, p))
            .child(
                div()
                    .id("graph-drop-branches")
                    .max_h(px(MAX_STRIP_HEIGHT))
                    .overflow_y_scroll()
                    .child(list),
            )
    }
}

impl GitApp {
    /// The "+N" chip's menu: each label that did not fit, with its own menu
    /// under it (`branch` gives a branch's, from the branch list), and a
    /// tag's delete and copy.
    pub(crate) fn graph_label_entries(
        &self,
        labels: &[git_core::RefLabel],
        c: &'static Copy,
        cx: &mut Context<Self>,
        branch: &BranchMenu<'_>,
    ) -> Vec<MenuEntry> {
        let mut entries = Vec::new();
        for label in labels {
            let inner = match label.kind {
                RefKind::Local | RefKind::Remote => branch(self, label.name.clone(), cx),
                RefKind::Tag => {
                    let (delete, copy) = (label.name.clone(), label.name.clone());
                    vec![
                        better_ui::fluent::menu::MenuItem::new(fill(
                            c.delete_tag,
                            &[("tag", &label.name)],
                        ))
                        .on_select(Self::act(cx, move |this, _, _| {
                            this.session.dialog = Some(crate::dialogs::Dialog::DeleteTag {
                                name: delete.clone(),
                            })
                        }))
                        .entry(),
                        better_ui::fluent::menu::MenuItem::new(fill(
                            c.copy_tag,
                            &[("tag", &label.name)],
                        ))
                        .on_select(Self::act(cx, move |this, _, cx| {
                            this.copy_text(copy.clone(), cx)
                        }))
                        .entry(),
                    ]
                }
                RefKind::Head => continue,
            };
            if !inner.is_empty() {
                entries.push(MenuEntry::submenu(label.name.clone(), inner));
            }
        }
        entries
    }

    /// A graph row's context menu: History's commit menu, with reset and
    /// revert only for HEAD and its ancestors.
    pub(crate) fn graph_entries(
        &self,
        index: usize,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        let Some(repo) = self.session.repo.as_ref() else {
            return Vec::new();
        };
        let Some(commit) = repo.graph.commits.get(index).cloned() else {
            return Vec::new();
        };
        let head = repo.status.head_oid.clone().unwrap_or_default();
        let menu = crate::commit_menu::CommitMenu {
            head: head == commit.sha && !repo.status.unborn,
            ancestor: repo.graph.is_ancestor_of(&commit.sha, &head),
            tags: git_core::ref_labels(&commit.refs)
                .into_iter()
                .filter(|l| l.kind == RefKind::Tag)
                .map(|l| l.name)
                .collect(),
        };
        self.commit_entries(&commit, menu, c, cx)
    }
}

fn v_center(child: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .p(spacing::LG)
        .child(child)
}

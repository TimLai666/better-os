//! Drawing the session in `better_ui::fluent`'s controls, laid out the way
//! GitHub Desktop is: a toolbar with the repository, the branch, and the
//! sync button; a 320px pane with Changes and History; and the diff.
//!
//! Nothing here decides anything. Every state shown is read from the
//! session, and every click calls the session or one of [`GitApp`]'s
//! methods.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use git_core::{Commit, CommitFile, ConflictKind, DiffLineKind, FileDiff, Operation};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _};

use better_ui::fluent::button::{Button, ButtonKind, MenuSegment};
use better_ui::fluent::dialog::{
    ContentDialog, DefaultButton, DialogButtons, DialogResult, Flyout,
};
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::list::{row_fill, selection_bar};
use better_ui::fluent::menu::{ContextMenu, MenuEntry, MenuFlyout, MenuItem, open_menu};
use better_ui::fluent::navigation::{NavItem, NavigationView};
use better_ui::fluent::progress::{Progress, ProgressRing};
use better_ui::fluent::selection::{ComboBox, ComboItem};
use better_ui::fluent::settings_card::SettingsCard;
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::toggle::{CheckBox, CheckState, RadioButtons, RadioOption, ToggleSwitch};
use better_ui::fluent::{
    self, BODY, BODY_STRONG, CAPTION, CODE, Interaction, Palette, SUBTITLE, TITLE,
    chrome::TitleBar, label, radius, spacing, with_alpha,
};

use crate::app::{BRANCH_ICON, ContextTarget, Flyout as Open, GitApp};
use crate::changes::{Badge, Row as ChangeRow};
use crate::dialogs::{BranchPurpose, CloneTab, Dialog};
use crate::github::SignIn;
use crate::i18n::{Copy, Locale, fill};
use crate::keys::{self, Action};
use crate::pane::PaneKind;
use crate::remote::SyncAction;
use crate::session::{DiffContent, DiffKey, Page, Problem, Repo, Tab};
use crate::{Appearance, publish};
use better_ui::pane_edge::PANE_EDGE_HIT_WIDTH;
use better_ui::window_chrome;

const TOOLBAR_HEIGHT: Pixels = px(56.0);
const ROW_HEIGHT: Pixels = px(36.0);
const TALL_ROW: Pixels = px(48.0);
const DIFF_ROW: Pixels = px(20.0);
const NUMBER_WIDTH: Pixels = px(44.0);
const GUTTER_WIDTH: Pixels = px(20.0);
const FOLDER_CHARS: usize = 32;
const TINT: f32 = 0.14;

/// A commit being dragged in the history list.
#[derive(Clone)]
pub(crate) struct DraggedCommit {
    pub index: usize,
    pub summary: SharedString,
}

impl Render for DraggedCommit {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        fluent::menu::panel_frame(div(), p)
            .px(spacing::MD)
            .py(spacing::SM)
            .child(label(self.summary.clone(), BODY, p.text_primary))
    }
}

fn t(element: Div, style: fluent::TypeStyle, color: Rgba) -> Div {
    fluent::text(element, style).text_color(color)
}

fn caption(text: impl Into<SharedString>, p: &Palette) -> Div {
    label(text, CAPTION, p.text_secondary)
}

fn badge(kind: Badge, p: &Palette) -> Div {
    let color = match kind {
        Badge::Added => p.success,
        Badge::Deleted | Badge::Conflicted => p.critical,
        Badge::Renamed => p.accent,
        Badge::Modified => p.caution,
    };
    div()
        .flex_none()
        .size(px(16.0))
        .rounded(radius::SM)
        .border_1()
        .border_color(color)
        .flex()
        .items_center()
        .justify_center()
        .child(label(kind.letter(), CAPTION, color).font_weight(FontWeight::BOLD))
}

/// A list row: the subtle fill, the accent bar when selected. The row is
/// the click target edge to edge and down to the next row; the fill is drawn
/// inset inside it (4px from the sides, 1px from the next row), so the
/// pixels between two rows still belong to one of them (`hit_area`).
pub(crate) fn row(
    id: impl Into<ElementId>,
    selected: bool,
    height: Pixels,
    p: &Palette,
) -> Stateful<Div> {
    let inset = better_ui::fluent::hit_area::RowInset { x: 4.0, y: 1.0 };
    let hover = row_fill(selected, Interaction::PointerOver, p);
    let pressed = row_fill(selected, Interaction::Pressed, p);
    let group = better_ui::fluent::hit_area::ROW_GROUP;
    div()
        .id(id)
        .group(group)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::SM)
        .h(height + px(2.0 * inset.y))
        .px(spacing::MD + px(inset.x))
        .child(
            div()
                .id("look")
                .absolute()
                .top(px(inset.y))
                .bottom(px(inset.y))
                .left(px(inset.x))
                .right(px(inset.x))
                .rounded(radius::SM)
                .bg(row_fill(selected, Interaction::Rest, p))
                .group_hover(group, move |s| s.bg(hover))
                .group_active(group, move |s| s.bg(pressed)),
        )
        .when(selected, |r| {
            r.child(
                div()
                    .absolute()
                    .left(px(inset.x))
                    .top(px((f32::from(height) - 16.0) / 2.0 + inset.y))
                    .child(selection_bar(p, false)),
            )
        })
}

fn truncate(text: &str, max: usize) -> String {
    crate::changes::truncate_leading(text, max)
}

fn image(bytes: &[u8]) -> Option<Arc<Image>> {
    let format = match git_core::sniff_image(bytes)? {
        git_core::ImageKind::Png => ImageFormat::Png,
        git_core::ImageKind::Jpeg => ImageFormat::Jpeg,
        git_core::ImageKind::Gif => ImageFormat::Gif,
        git_core::ImageKind::Webp => ImageFormat::Webp,
        git_core::ImageKind::Bmp => ImageFormat::Bmp,
        git_core::ImageKind::Svg => ImageFormat::Svg,
        git_core::ImageKind::Ico => ImageFormat::Ico,
        git_core::ImageKind::Tiff => ImageFormat::Tiff,
    };
    Some(Arc::new(Image::from_bytes(format, bytes.to_vec())))
}

/// The value the list pane's edge drag carries, and the (empty) thing drawn
/// under the pointer while it moves: the pane itself follows the pointer.
pub(crate) struct EdgeDrag;

impl Render for EdgeDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// What a click on a diff gutter does.
type Toggle = Box<dyn Fn(&mut Window, &mut App)>;

/// One visual row of a diff line: a byte range of the line as drawn (tabs
/// as spaces), and whether it is the line's first row, which carries the
/// numbers and the mark (ticket 123).
#[derive(Clone, Copy)]
struct Part {
    from: usize,
    to: usize,
    first: bool,
    /// On a line's last row when the line reached the row cap: how many
    /// characters are not drawn, said at the row's end.
    hidden: usize,
}

/// One row of the flattened diff.
#[derive(Clone, Copy)]
enum DiffRow {
    Hunk(usize),
    Line(usize, usize, Part),
    /// A side by side row: the pair of lines, and the part of each drawn on
    /// this row. A side whose line has no row left here is drawn blank in
    /// its line's color, under the line's last row.
    Split(usize, git_core::SplitRow, Option<Part>, Option<Part>),
}

/// A diff laid out as rows, kept between frames: expanding tabs and
/// wrapping a long diff is the costly part of drawing it, so it is done when
/// the diff, the wrap width or the mode changes, not on every frame.
pub(crate) struct DiffLayout {
    key: (u64, bool, Option<(usize, usize)>),
    rows: Arc<Vec<DiffRow>>,
    texts: Arc<Vec<Vec<String>>>,
    hunks: Arc<Vec<git_core::Hunk>>,
}

/// How many text columns a diff row has room for: `None` when lines do not
/// wrap. `unified` and `split` are the columns of a unified row and of one
/// half of a side by side row.
#[derive(Clone, Copy, PartialEq)]
struct WrapAt {
    unified: usize,
    split: usize,
}

fn parts(text: &str, columns: Option<usize>) -> Vec<Part> {
    let (ranges, hidden) = match columns {
        Some(columns) => {
            let rows = crate::wrap::rows(text, columns);
            (rows.ranges, rows.hidden)
        }
        None => (std::iter::once(0..text.len()).collect(), 0),
    };
    let count = ranges.len();
    ranges
        .into_iter()
        .enumerate()
        .map(|(i, r)| Part {
            from: r.start,
            to: r.end,
            first: i == 0,
            hidden: if i + 1 == count { hidden } else { 0 },
        })
        .collect()
}

fn flatten(
    hunks: &[git_core::Hunk],
    texts: &[Vec<String>],
    split: bool,
    wrap: Option<WrapAt>,
) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for (h, hunk) in hunks.iter().enumerate() {
        rows.push(DiffRow::Hunk(h));
        if split {
            let columns = wrap.map(|w| w.split);
            for pair in git_core::split_rows(hunk) {
                let left = pair.left.map(|l| parts(&texts[h][l], columns));
                let right = pair.right.map(|l| parts(&texts[h][l], columns));
                let height = left
                    .as_ref()
                    .map_or(1, Vec::len)
                    .max(right.as_ref().map_or(1, Vec::len));
                for i in 0..height {
                    rows.push(DiffRow::Split(
                        h,
                        pair,
                        left.as_ref().and_then(|p| p.get(i).copied()),
                        right.as_ref().and_then(|p| p.get(i).copied()),
                    ));
                }
            }
        } else {
            let columns = wrap.map(|w| w.unified);
            for (l, text) in texts[h].iter().enumerate() {
                rows.extend(
                    parts(text, columns)
                        .into_iter()
                        .map(|part| DiffRow::Line(h, l, part)),
                );
            }
        }
    }
    rows
}

impl Render for GitApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keyboard shortcuts need focus somewhere in the window; when a
        // dialog or flyout that held it closes, it comes back here.
        if window.focused(cx).is_none() {
            self.focus_handle.focus(window, cx);
        }
        // A dropdown that closed may leave focus on one of its own controls
        // (its search box, a row's More button or its menu), which is no
        // longer drawn, and keys would reach nothing. A dialog it opened
        // takes focus itself.
        // Whatever held focus while it was open was inside it (the flyout
        // keeps focus in itself); focus still there goes back to the window.
        let focused = window.focused(cx);
        if self.flyout.is_some() {
            if let Some(focused) = focused
                && !self.flyout_focus.contains(&focused)
            {
                self.flyout_focus.push(focused);
            }
        } else if !self.flyout_focus.is_empty() {
            let inside = focused.is_some_and(|f| self.flyout_focus.contains(&f));
            self.flyout_focus.clear();
            if inside && self.session.dialog.is_none() {
                self.focus_handle.focus(window, cx);
            }
        }
        let p = fluent::palette(cx);
        let c = self.copy();
        // The desktop's name for the window (the overview, Alt+Tab).
        self.names = self.session.open_names();
        let title = crate::session::window_title(self.names.as_ref(), c.app_name);
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
        let content = match self.session.page {
            Page::Settings => self.settings_page(p, c, cx).into_any_element(),
            Page::Main => v_stack()
                .flex_1()
                .min_h_0()
                .child(self.toolbar(p, c, window, cx))
                .children(self.bars(p, c, cx))
                .child(match &self.session.repo {
                    Some(_) => self.repository_view(p, c, window, cx).into_any_element(),
                    None => self.welcome(p, c, cx).into_any_element(),
                })
                .into_any_element(),
        };
        let dialog = self.dialog(p, c, window, cx);
        let context = self.context_menu(c, cx);
        self.sync_notice_toast(cx);
        div()
            .id("better-git")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // The window frame paints the window background, with the
            // desktop's top corners (ticket 108).
            .text_color(p.text_primary)
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            .child(
                TitleBar::new(c.app_name)
                    .icon(Icon::empty().path(BRANCH_ICON).small().text_color(p.accent))
                    .middle(self.menu_bar(c, cx)),
            )
            .child(content)
            .children(dialog)
            .children(context)
            .children(self.update_restart.toast_layer(self.words(), cx))
    }
}

fn v_stack() -> Div {
    div().flex().flex_col()
}

fn h_stack() -> Div {
    div().flex().flex_row().items_center()
}

impl GitApp {
    // --- toolbar ------------------------------------------------------------

    /// One of the toolbar's drop-down cells. It starts at the width its
    /// text asks for (`shortcuts::item_width`) and gives way down to
    /// `min_width` when the window is narrow, its value truncated with an
    /// ellipsis and given whole in the tooltip.
    #[allow(clippy::too_many_arguments)]
    fn toolbar_item(
        &self,
        id: &'static str,
        title: &str,
        value: String,
        tooltip: String,
        icon: Icon,
        open: bool,
        quiet: bool,
        captions: bool,
        p: &'static Palette,
    ) -> Stateful<Div> {
        let hover = p.subtle_fill;
        let basis = crate::shortcuts::item_width(title, &value);
        // With its caption it never gets narrower than the caption: only
        // the value line ends in an ellipsis.
        let min_width = if captions {
            crate::shortcuts::item_min_width(title)
        } else {
            crate::shortcuts::ITEM_MIN_BARE
        };
        div()
            .id(id)
            .relative()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .h_full()
            .flex_basis(px(basis))
            .flex_shrink(1.0)
            .min_w(px(min_width))
            .max_w(px(300.0))
            .px(spacing::LG)
            .border_r_1()
            .border_color(p.divider)
            .when(open, |d| d.bg(p.subtle_fill))
            .hover(move |s| s.bg(hover))
            .when(!open, |d| {
                d.tooltip(better_ui::fluent::dialog::tooltip_builder(tooltip.into()))
            })
            .child(icon.size(px(16.0)).text_color(p.text_secondary))
            .child(
                v_stack()
                    .flex_1()
                    .min_w_0()
                    .when(captions, |d| {
                        d.child(caption(title.to_string(), p).whitespace_nowrap())
                    })
                    .child(if quiet {
                        label(value, BODY, p.text_secondary).truncate()
                    } else {
                        label(value, BODY_STRONG, p.text_primary).truncate()
                    }),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(12.0))
                    .text_color(p.text_secondary),
            )
    }

    fn toolbar(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let repo = self.session.repo.as_ref();
        let names = self.names.clone();
        let width = f32::from(window.viewport_size().width);
        let checks = repo.is_some_and(|r| r.github.checks.is_some());
        let captions = crate::shortcuts::captions_fit(
            width,
            [c.current_repository, c.current_worktree(), c.current_branch],
            self.shortcut_labels().len(),
            checks,
        );
        let repo_name = names
            .as_ref()
            .map(|n| n.repository.clone())
            .unwrap_or_else(|| c.no_repository.to_string());
        let mut bar = h_stack()
            .flex_none()
            .h(TOOLBAR_HEIGHT)
            .w_full()
            .border_b_1()
            .border_color(p.divider)
            .bg(p.layer);

        let repo_label = repo_name.clone();
        let repos = self
            .toolbar_item(
                "toolbar-repository",
                c.current_repository,
                repo_name.clone(),
                repo.map(|r| r.root.display().to_string())
                    .unwrap_or(repo_name),
                Icon::new(IconName::Folder),
                self.flyout == Some(Open::Repositories),
                false,
                captions,
                p,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_flyout(Open::Repositories, window, cx);
                this.drive(window, cx);
            }))
            .when(self.flyout == Some(Open::Repositories), |d| {
                d.child(self.repository_flyout(p, c, cx))
            });
        bar = bar.child(repos);

        let worktree_label = names
            .as_ref()
            .map(|n| n.worktree.clone())
            .unwrap_or_default();
        if let (Some(repo), Some(names)) = (repo, names.as_ref()) {
            // Quieter, and narrower, while the main worktree is the only
            // one: still there, so adding one is a click away.
            let worktrees = self
                .toolbar_item(
                    "toolbar-worktree",
                    c.current_worktree(),
                    names.worktree.clone(),
                    repo.root.display().to_string(),
                    Icon::empty().path(crate::app::WORKTREE_ICON),
                    self.flyout == Some(Open::Worktrees),
                    names.only_main,
                    captions,
                    p,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_flyout(Open::Worktrees, window, cx);
                    cx.notify();
                }))
                .when(self.flyout == Some(Open::Worktrees), |d| {
                    d.child(self.worktree_flyout(p, c, cx))
                });
            bar = bar.child(worktrees);

            let branch = repo
                .branch()
                .map(str::to_string)
                .unwrap_or_else(|| c.detached.to_string());
            let branches = self
                .toolbar_item(
                    "toolbar-branch",
                    c.current_branch,
                    branch.clone(),
                    branch,
                    Icon::empty().path(BRANCH_ICON),
                    self.flyout == Some(Open::Branches),
                    false,
                    captions,
                    p,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_flyout(Open::Branches, window, cx);
                    cx.notify();
                }))
                .when(self.flyout == Some(Open::Branches), |d| {
                    d.child(self.branch_flyout(p, c, window, cx))
                });
            bar = bar.child(branches);
        }

        bar = bar.child(div().flex_1());
        // Fetch, pull, push and publish sit at the right, before the
        // shortcut buttons (owner, 2026-11-15).
        if let Some(repo) = repo {
            bar = bar.child(self.sync_item(repo, p, c, cx));
        }
        let branch_name = repo
            .map(|r| {
                r.branch()
                    .map(str::to_string)
                    .unwrap_or_else(|| c.detached.to_string())
            })
            .unwrap_or_default();
        let labels: Vec<String> = self.shortcut_labels().into_iter().map(|(_, l)| l).collect();
        let compact = crate::shortcuts::compact(
            f32::from(window.viewport_size().width),
            [
                (c.current_repository, repo_label.as_str()),
                (c.current_worktree(), worktree_label.as_str()),
                (c.current_branch, branch_name.as_str()),
            ],
            &labels,
            checks,
        );
        bar = bar.child(self.shortcut_buttons(compact, p, cx));
        if let Some(repo) = repo {
            bar = bar.children(self.checks_item(repo, captions, p, c, cx));
        }
        bar.child(
            div().flex_none().px(spacing::SM).child(
                Button::icon_button("settings-button", IconName::Settings, c.settings)
                    .on_click(Self::act(cx, |this, _, _| this.session.open_settings())),
            ),
        )
    }

    /// GitHub Desktop's menu bar, in the titlebar: File, View, Repository,
    /// Branch, each a menu flyout under its button.
    fn menu_bar(&self, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let mut bar = h_stack().gap(px(2.0)).pl(spacing::MD);
        for (index, (title, entries)) in self.app_menu_entries(c, cx).into_iter().enumerate() {
            let state = self.app_menu[index].clone();
            let open = state.read(cx).is_open();
            let opener = state.clone();
            bar = bar.child(
                div()
                    .relative()
                    .child(
                        Button::subtle(("menu-bar", index), title)
                            .on_click(move |window, cx| open_menu(&opener, None, window, cx)),
                    )
                    .when(open, |d| {
                        d.child(MenuFlyout::new(("menu-flyout", index), entries).state(state))
                    }),
            );
        }
        bar
    }

    fn sync_item(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let action = crate::remote::sync_action(
            &repo.status,
            !repo.info.remotes.is_empty(),
            repo.branch().is_none(),
        );
        let busy = self.session.is_network_busy();
        let now = crate::github::now();
        let fetched = repo
            .last_fetch
            .map(|t| fill(c.last_fetched, &[("when", &c.ago(now - t))]))
            .unwrap_or_else(|| c.never_fetched.to_string());
        let (title, icon, counts) = match action {
            SyncAction::PublishRepository => {
                (c.publish_repository, IconName::ArrowUp, String::new())
            }
            SyncAction::PublishBranch => (c.publish_branch, IconName::ArrowUp, String::new()),
            SyncAction::Pull { behind, ahead } => (
                c.pull_origin,
                IconName::ArrowDown,
                if ahead > 0 {
                    format!("↓{behind} ↑{ahead}")
                } else {
                    format!("↓{behind}")
                },
            ),
            SyncAction::Push { ahead } => (c.push_origin, IconName::ArrowUp, format!("↑{ahead}")),
            SyncAction::Fetch | SyncAction::None => (c.fetch_origin, IconName::Redo, String::new()),
        };
        // Fetch reads as a refresh of what the remote has, not as Redo: the
        // shared two-arrow glyph, as GitHub Desktop's fetch is.
        let icon = if matches!(action, SyncAction::Fetch | SyncAction::None) {
            Icon::empty().path(better_ui::fluent::glyphs::REFRESH)
        } else {
            Icon::new(icon)
        };
        let title = match (busy, repo.busy) {
            (true, Some(crate::session::OpKind::Pull)) => c.pulling,
            (true, Some(crate::session::OpKind::Push | crate::session::OpKind::ForcePush)) => {
                c.pushing
            }
            (true, _) => c.fetching,
            _ => title,
        };
        let hover = p.subtle_fill;
        let main = div()
            .id("toolbar-sync-main")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::MD)
            .h_full()
            .pl(spacing::LG)
            .pr(spacing::SM)
            .hover(move |s| s.bg(hover))
            .child(if busy {
                ProgressRing::new("sync-ring", Progress::Indeterminate)
                    .size(px(16.0))
                    .into_any_element()
            } else {
                icon.size(px(16.0))
                    .text_color(p.text_secondary)
                    .into_any_element()
            })
            .child(
                v_stack()
                    .flex_1()
                    .min_w_0()
                    .child(label(title, BODY_STRONG, p.text_primary).truncate())
                    .child(caption(fetched, p).truncate()),
            )
            .when(!counts.is_empty() && !busy, |d| {
                d.child(label(counts, BODY_STRONG, p.accent))
            })
            .when(busy, |d| {
                d.child(
                    Button::subtle("sync-cancel", c.cancel)
                        .tooltip(c.cancel_network)
                        .on_click(Self::act(cx, |this, _, _| this.session.cancel_network())),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if !this.session.is_network_busy() {
                    this.sync_button(true);
                    this.drive(window, cx);
                }
            }));
        // The arrow: every action that applies now, as GitHub Desktop's
        // split button offers (ticket 123).
        let state = self.sync_menu.clone();
        let open = state.read(cx).is_open();
        div()
            .id("toolbar-sync")
            .relative()
            .flex()
            .items_center()
            .h_full()
            // Room for "Fetch origin" whole; narrower windows shrink it to
            // here, the lines ending in an ellipsis.
            .flex_shrink(1.0)
            .min_w(px(crate::shortcuts::SYNC_MIN))
            .border_r_1()
            .border_color(p.divider)
            .child(main)
            // The divider and everything after it open the menu, the
            // cell's full height, and Escape gives focus back (ticket 130).
            .child(
                MenuSegment::new("sync-more", state)
                    .entries(if open {
                        self.sync_menu_entries(repo, c, cx)
                    } else {
                        Vec::new()
                    })
                    .tooltip(c.sync_more)
                    .disabled(busy),
            )
    }

    /// The rows of the sync button's arrow menu: what each does and how many
    /// commits it moves; one that cannot be chosen now says why.
    fn sync_menu_entries(
        &self,
        repo: &Repo,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        use crate::remote::MenuAction;
        // Where Fetch, Pull and Push go: the push target's remote (whose
        // name may hold a `/`), and the upstream, or the branch it would
        // get when published.
        let target = repo.push_target();
        let remote = target
            .as_ref()
            .map(|(remote, _, _)| remote.clone())
            .or_else(|| repo.push_remote())
            .unwrap_or_else(|| "origin".to_string());
        let upstream = match (&repo.status.upstream, &target) {
            (Some(upstream), _) => upstream.clone(),
            (None, Some((remote, name, _))) => format!("{remote}/{name}"),
            (None, None) => remote.clone(),
        };
        let busy = self.session.is_network_busy();
        let rows = crate::remote::sync_menu(
            &repo.status,
            !repo.info.remotes.is_empty(),
            repo.branch().is_none(),
        );
        let mut entries = Vec::new();
        for row in rows {
            if matches!(row.action, MenuAction::ForcePush { .. }) {
                entries.push(MenuEntry::Separator);
            }
            let icon = match row.action {
                MenuAction::Pull { .. } => Some(IconName::ArrowDown),
                MenuAction::Push { .. }
                | MenuAction::ForcePush { .. }
                | MenuAction::PublishBranch
                | MenuAction::PublishRepository => Some(IconName::ArrowUp),
                MenuAction::Fetch => None,
            };
            let mut item = MenuItem::new(crate::remote::menu_label(&row, &upstream, &remote, c))
                .disabled(!row.enabled() || busy);
            item = match icon {
                Some(icon) => item.icon(icon),
                None => item.icon_path(better_ui::fluent::glyphs::REFRESH),
            };
            let action = row.action;
            entries.push(
                item.on_select(Self::act(cx, move |this, _, _| match action {
                    MenuAction::Fetch => this.session.fetch(false, crate::github::now()),
                    MenuAction::Pull { .. } => this.session.pull(),
                    MenuAction::Push { .. } | MenuAction::PublishBranch => this.session.push(false),
                    // Always asked, whatever the setting: from this menu it
                    // is one click away from the plain push.
                    MenuAction::ForcePush { .. } => this.session.ask_force_push(),
                    MenuAction::PublishRepository => this.sync_button(true),
                }))
                .entry(),
            );
        }
        entries
    }

    /// The pull request's checks. In a narrow window (`roomy` false) the
    /// re-run button is an icon, and the item clips rather than reaching
    /// over its neighbours.
    fn checks_item(
        &self,
        repo: &Repo,
        roomy: bool,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let checks = repo.github.checks.as_ref()?;
        let overall = checks.overall()?;
        let (icon, color, text) = match overall {
            github_api::CheckState::Success => (IconName::CircleCheck, p.success, c.checks_passed),
            github_api::CheckState::Failure => (IconName::CircleX, p.critical, c.checks_failed),
            github_api::CheckState::Pending => (IconName::Loader, p.caution, c.checks_pending),
            github_api::CheckState::Neutral => (IconName::Info, p.text_secondary, c.checks),
        };
        let mut item = h_stack()
            .flex_shrink(1.0)
            .min_w_0()
            .overflow_hidden()
            .gap(spacing::SM)
            .px(spacing::MD)
            .child(Icon::new(icon).size(px(16.0)).text_color(color))
            .child(label(text, BODY, p.text_primary).min_w_0().truncate());
        if overall == github_api::CheckState::Failure {
            let rerun = if roomy {
                Button::standard("rerun-checks", c.rerun_checks)
            } else {
                Button::icon_button("rerun-checks", IconName::Redo, c.rerun_checks)
            };
            item =
                item.child(rerun.on_click(Self::act(cx, |this, _, _| this.session.rerun_checks())));
        }
        if let Some(pull) = self.session.current_pull() {
            let url = pull.html_url.clone();
            item = item.child(
                Button::subtle("view-pull", format!("#{}", pull.number))
                    .icon(IconName::ExternalLink)
                    .tooltip(c.view_pull)
                    .on_click(Self::act(cx, move |this, _, _| this.open_url(&url))),
            );
        }
        Some(item.into_any_element())
    }

    fn menu_item(
        &self,
        label_text: &str,
        action: Action,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> MenuEntry {
        MenuItem::new(label_text.to_string())
            .shortcut(keys::label(action))
            .disabled(!enabled)
            .on_select(Self::act(cx, move |this, window, cx| {
                this.run_action(action, window, cx)
            }))
            .entry()
    }

    fn app_menu_entries(
        &self,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<(&'static str, Vec<MenuEntry>)> {
        let repo = self.session.repo.as_ref();
        let has = repo.is_some();
        let on_branch = repo.is_some_and(|r| r.branch().is_some());
        let hosted = repo.is_some_and(|r| r.hosted.is_some());
        let signed = self.session.github.account.is_some();
        let default = repo
            .and_then(Repo::default_branch)
            .unwrap_or_else(|| "main".into());
        vec![
            (
                c.menu_file,
                vec![
                    self.menu_item(c.new_repository, Action::NewRepository, true, cx),
                    self.menu_item(c.add_local, Action::AddLocalRepository, true, cx),
                    self.menu_item(c.clone_repository, Action::CloneRepository, true, cx),
                    MenuEntry::Separator,
                    self.menu_item(c.settings, Action::Settings, true, cx),
                ],
            ),
            (
                c.menu_view,
                vec![
                    self.menu_item(c.show_changes, Action::ShowChanges, has, cx),
                    self.menu_item(c.show_history, Action::ShowHistory, has, cx),
                    self.menu_item(
                        crate::graph::graph_copy(self.locale).tab,
                        Action::ShowGraph,
                        has,
                        cx,
                    ),
                    self.menu_item(c.repository_list, Action::RepositoryList, true, cx),
                    self.menu_item(c.branch_list, Action::BranchList, has, cx),
                    self.menu_item(c.worktrees, Action::WorktreeList, has, cx),
                    self.menu_item(c.go_to_summary, Action::GoToSummary, has, cx),
                    self.menu_item(
                        c.show_stash,
                        Action::ToggleStash,
                        self.session.branch_stash().is_some(),
                        cx,
                    ),
                    self.menu_item(c.toggle_filter, Action::ToggleFilter, has, cx),
                    MenuItem::new(c.wrap_lines)
                        .checked(self.session.settings.wrap_lines)
                        .on_select(Self::act(cx, |this, _, _| {
                            let wrap = this.session.settings.wrap_lines;
                            this.session.set_wrap_lines(!wrap);
                        }))
                        .entry(),
                    self.menu_item(c.refresh, Action::Refresh, has, cx),
                ],
            ),
            (
                c.menu_repository,
                vec![
                    self.menu_item(c.push_origin, Action::Push, on_branch, cx),
                    self.menu_item(c.pull_origin, Action::Pull, on_branch, cx),
                    self.menu_item(c.fetch_origin, Action::Fetch, has, cx),
                    MenuItem::new(c.amend_commit)
                        .disabled(!repo.is_some_and(|r| !r.status.unborn))
                        .on_select(Self::act(cx, |this, _, _| this.session.amend_last()))
                        .entry(),
                    self.menu_item(c.remove_repository, Action::RemoveRepository, has, cx),
                    MenuEntry::Separator,
                    self.menu_item(c.view_on_github, Action::ViewOnGitHub, hosted, cx),
                    self.menu_item(c.open_in_terminal, Action::OpenInTerminal, has, cx),
                    self.menu_item(c.show_in_files, Action::ShowInFiles, has, cx),
                    self.menu_item(c.open_in_editor, Action::OpenInEditor, has, cx),
                    self.menu_item(c.create_issue, Action::CreateIssue, hosted && signed, cx),
                    self.menu_item(c.new_worktree, Action::NewWorktree, has, cx),
                    MenuItem::new(c.repository_settings)
                        .disabled(!has)
                        .on_select(Self::act(cx, |this, _, _| {
                            this.session.dialog = Some(Dialog::Remotes { editing: None })
                        }))
                        .entry(),
                ],
            ),
            (
                c.menu_branch,
                vec![
                    self.menu_item(c.new_branch, Action::NewBranch, has, cx),
                    self.menu_item(c.rename_branch, Action::RenameBranch, on_branch, cx),
                    self.menu_item(c.delete_branch, Action::DeleteBranch, on_branch, cx),
                    MenuEntry::Separator,
                    self.menu_item(
                        c.discard_all,
                        Action::DiscardAll,
                        repo.is_some_and(Repo::has_changes),
                        cx,
                    ),
                    self.menu_item(
                        c.stash_all,
                        Action::StashAll,
                        repo.is_some_and(Repo::has_changes),
                        cx,
                    ),
                    MenuEntry::Separator,
                    self.menu_item(
                        &fill(c.update_from, &[("branch", &default)]),
                        Action::UpdateFromDefault,
                        on_branch,
                        cx,
                    ),
                    self.menu_item(c.compare_to_branch, Action::CompareToBranch, has, cx),
                    self.menu_item(
                        c.merge_into_current,
                        Action::MergeIntoCurrent,
                        on_branch,
                        cx,
                    ),
                    self.menu_item(c.squash_merge, Action::SquashAndMerge, on_branch, cx),
                    self.menu_item(c.rebase_current, Action::RebaseCurrent, on_branch, cx),
                    MenuEntry::Separator,
                    self.menu_item(
                        c.compare_on_github,
                        Action::CompareOnGitHub,
                        hosted && on_branch,
                        cx,
                    ),
                    self.menu_item(
                        c.view_branch_on_github,
                        Action::ViewBranchOnGitHub,
                        hosted && on_branch,
                        cx,
                    ),
                    self.menu_item(
                        if self.session.current_pull().is_some() {
                            c.view_pull
                        } else {
                            c.create_pull
                        },
                        Action::PullRequest,
                        hosted && on_branch && signed,
                        cx,
                    ),
                ],
            ),
        ]
    }

    // --- flyouts --------------------------------------------------------------

    fn repository_flyout(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.inputs.repo_search.read(cx).value().to_string();
        let rows = self.session.repository_groups(&query);
        let mut list = v_stack()
            .max_h(px(420.0))
            .id("repo-list")
            .overflow_y_scroll();
        for (index, r) in rows.into_iter().enumerate() {
            let path = r.path.clone();
            let open_path = r.opens.clone();
            let menu_path = r.path.clone();
            let mut item = row(("repo", index), r.current, TALL_ROW, p)
                .children(self.highlight_ring(index, p))
                .child(
                    Icon::new(if r.missing {
                        IconName::TriangleAlert
                    } else {
                        IconName::Folder
                    })
                    .size(px(16.0))
                    .text_color(if r.missing {
                        p.caution
                    } else {
                        p.text_secondary
                    }),
                )
                .child(
                    v_stack()
                        .flex_1()
                        .min_w_0()
                        .child(
                            label(
                                r.name.clone(),
                                BODY_STRONG,
                                if r.missing {
                                    p.text_disabled
                                } else {
                                    p.text_primary
                                },
                            )
                            .truncate(),
                        )
                        .child(
                            caption(
                                if r.missing {
                                    c.missing.to_string()
                                } else {
                                    truncate(&r.location, 40)
                                },
                                p,
                            )
                            .truncate(),
                        ),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.context =
                            Some((ContextTarget::Repository(menu_path.clone()), event.position));
                        open_menu(&this.context_menu, None, window, cx);
                        cx.stop_propagation();
                    }),
                );
            if r.missing {
                let from = path.clone();
                item = item
                    .child(
                        Button::standard(("locate", index), c.locate).on_click(Self::act(
                            cx,
                            move |this, window, cx| {
                                let from = from.clone();
                                this.choose_folder(window, cx, move |app, to, _, _| {
                                    app.session.relocate(&from, to)
                                });
                            },
                        )),
                    )
                    .child(
                        Button::subtle(("remove-missing", index), c.remove)
                            .on_click(Self::act(cx, move |this, _, _| {
                                this.session.ask_remove_repository(path.clone())
                            })),
                    );
            } else if let Some(open_path) = open_path {
                item = item.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if event.is_keyboard() {
                        return;
                    }
                    this.flyout = None;
                    this.session.open(open_path.clone());
                    this.drive(window, cx);
                }));
            }
            list = list.child(item);
        }
        Flyout::new("repository-flyout")
            .initial_focus(self.inputs.repo_search.focus_handle(cx))
            .on_dismiss(Self::act(cx, |this, _, _| this.flyout = None))
            .child(
                self.flyout_keys(v_stack(), cx)
                    .w(px(400.0))
                    .gap(spacing::SM)
                    .child(
                        h_stack()
                            .gap(spacing::SM)
                            .child(
                                div().flex_1().child(TextBox::search(
                                    "repo-search",
                                    &self.inputs.repo_search,
                                )),
                            )
                            .child(
                                fluent::button::SplitButton::new("repo-add-button", c.add)
                                    .entries(vec![
                                        MenuItem::new(c.add_local)
                                            .on_select(Self::act(cx, |this, w, cx| {
                                                this.flyout = None;
                                                this.run_action(Action::AddLocalRepository, w, cx)
                                            }))
                                            .entry(),
                                        MenuItem::new(c.clone_repository)
                                            .on_select(Self::act(cx, |this, w, cx| {
                                                this.flyout = None;
                                                this.run_action(Action::CloneRepository, w, cx)
                                            }))
                                            .entry(),
                                        MenuItem::new(c.new_repository)
                                            .on_select(Self::act(cx, |this, w, cx| {
                                                this.flyout = None;
                                                this.run_action(Action::NewRepository, w, cx)
                                            }))
                                            .entry(),
                                    ])
                                    .on_click(Self::act(cx, |this, w, cx| {
                                        this.flyout = None;
                                        this.run_action(Action::AddLocalRepository, w, cx)
                                    })),
                            ),
                    )
                    .child(caption(c.all_repositories, p))
                    .child(list),
            )
            .into_any_element()
    }

    fn branch_flyout(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(repo) = self.session.repo.as_ref() else {
            return div().into_any_element();
        };
        let tabs = SelectorBar::new(
            "branch-tabs",
            vec![
                SelectorItem::new(c.branches),
                SelectorItem::new(format!("{} ({})", c.pull_requests, repo.github.pulls.len())),
            ],
            Some(self.branch_tab),
        )
        .on_select(Self::act_with(cx, |this, index: usize, _, _| {
            this.branch_tab = index
        }));
        let body = if self.branch_tab == 0 {
            let query = self.inputs.branch_search.read(cx).value().to_string();
            let default = repo.default_branch();
            let rows = crate::branches::rows(
                &repo.info.branches,
                default.as_deref(),
                &repo.recent_branches,
                &query,
            );
            let mut list = v_stack()
                .max_h(px(420.0))
                .id("branch-list")
                .overflow_y_scroll();
            let mut last_group = None;
            for (index, b) in rows.into_iter().enumerate() {
                if last_group != Some(b.group) {
                    last_group = Some(b.group);
                    list = list.child(
                        caption(
                            match b.group {
                                crate::branches::Group::Default => c.default_branch,
                                crate::branches::Group::Recent => c.recent_branches,
                                crate::branches::Group::Other => c.other_branches,
                            },
                            p,
                        )
                        .px(spacing::MD)
                        .pt(spacing::SM),
                    );
                }
                let name = b.name.clone();
                let menu_name = b.name.clone();
                let drop_name = b.name.clone();
                list = list.child(
                    row(("branch", index), b.current, ROW_HEIGHT, p)
                        .children(self.highlight_ring(index, p))
                        .child(
                            Icon::empty()
                                .path(BRANCH_ICON)
                                .size(px(14.0))
                                .text_color(p.text_secondary),
                        )
                        .child(
                            label(
                                b.name.clone(),
                                if b.current { BODY_STRONG } else { BODY },
                                p.text_primary,
                            )
                            .flex_1()
                            .truncate(),
                        )
                        .when(b.remote_only, |r| r.child(caption("remote", p)))
                        .children(
                            crate::worktree_links::checked_out_elsewhere(
                                &b.name,
                                &repo.info.worktrees,
                                &repo.root,
                            )
                            .map(|path| {
                                caption(c.open_in_its_worktree(), p)
                                    .id(("branch-worktree", index))
                                    .text_color(p.accent_text)
                                    .tooltip(better_ui::fluent::dialog::tooltip_builder(
                                        fill(
                                            c.checked_out_in(),
                                            &[("path", &path.display().to_string())],
                                        )
                                        .into(),
                                    ))
                            }),
                        )
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            if event.is_keyboard() {
                                return;
                            }
                            this.flyout = None;
                            this.session.switch_branch(&name);
                            this.drive(window, cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                this.context = Some((
                                    ContextTarget::Branch(menu_name.clone()),
                                    event.position,
                                ));
                                open_menu(&this.context_menu, None, window, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .drag_over::<DraggedCommit>(move |s, _, _, _| {
                            s.bg(with_alpha(p.accent, 0.2))
                        })
                        .on_drop(
                            cx.listener(move |this, dragged: &DraggedCommit, window, cx| {
                                this.flyout = None;
                                this.session
                                    .cherry_pick(dragged.index, Some(drop_name.clone()));
                                this.drive(window, cx);
                            }),
                        ),
                );
            }
            self.flyout_keys(v_stack(), cx)
                .gap(spacing::SM)
                .child(
                    h_stack()
                        .gap(spacing::SM)
                        .child(
                            div().flex_1().child(TextBox::search(
                                "branch-search",
                                &self.inputs.branch_search,
                            )),
                        )
                        .child(
                            Button::standard("new-branch", c.new_branch)
                                .icon(IconName::Plus)
                                .on_click(Self::act(cx, |this, w, cx| {
                                    this.flyout = None;
                                    this.run_action(Action::NewBranch, w, cx)
                                })),
                        ),
                )
                .child(list)
                .into_any_element()
        } else if self.session.github.account.is_none() {
            caption(c.sign_in_for_pulls, p)
                .p(spacing::MD)
                .into_any_element()
        } else if repo.github.pulls.is_empty() {
            caption(c.no_pulls, p).p(spacing::MD).into_any_element()
        } else {
            let mut list = v_stack()
                .max_h(px(420.0))
                .id("pull-list")
                .overflow_y_scroll();
            for (index, pull) in repo.github.pulls.iter().enumerate() {
                let url = pull.html_url.clone();
                let head = pull.head.name.clone();
                let from_fork = pull.head.repo.as_ref().map(|r| r.full_name.clone())
                    != repo.hosted.as_ref().map(|h| h.full_name());
                let remote_branch = format!(
                    "{}/{}",
                    repo.push_remote().unwrap_or_else(|| "origin".into()),
                    head
                );
                list = list.child(
                    row(
                        ("pull", index),
                        self.session
                            .current_pull()
                            .is_some_and(|c| c.number == pull.number),
                        TALL_ROW,
                        p,
                    )
                    .child(
                        v_stack()
                            .flex_1()
                            .min_w_0()
                            .child(
                                label(pull.title.clone(), BODY_STRONG, p.text_primary).truncate(),
                            )
                            .child(
                                caption(
                                    format!("#{} · {} · {}", pull.number, pull.user.login, head),
                                    p,
                                )
                                .truncate(),
                            ),
                    )
                    .child(
                        Button::standard(("checkout-pull", index), c.checkout_pull)
                            .disabled(from_fork)
                            .on_click(Self::act(cx, move |this, _, _| {
                                this.flyout = None;
                                this.session.switch_branch(&remote_branch);
                            })),
                    )
                    .child(
                        Button::icon_button(
                            ("view-pull", index),
                            IconName::ExternalLink,
                            c.view_pull_short,
                        )
                        .kind(ButtonKind::Subtle)
                        .on_click(Self::act(cx, move |this, _, _| this.open_url(&url))),
                    ),
                );
            }
            list.into_any_element()
        };
        let mut flyout = Flyout::new("branch-flyout");
        if self.branch_tab == 0 {
            flyout = flyout.initial_focus(self.inputs.branch_search.focus_handle(cx));
        }
        flyout
            .on_dismiss(Self::act(cx, |this, _, _| this.flyout = None))
            .child(
                v_stack()
                    .w(px(400.0))
                    .gap(spacing::SM)
                    .child(tabs)
                    .child(body),
            )
            .into_any_element()
    }

    // --- bars ------------------------------------------------------------------

    fn bars(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut bars = Vec::new();
        let words = self.words();
        if let Some(problem) = &self.session.problem {
            let (title, message, details) = c.problem(problem);
            let mut report = ErrorReport::new(
                title,
                if details.is_empty() {
                    message.clone()
                } else {
                    details
                },
            );
            if !message.is_empty() {
                report = report.advice(message);
            }
            let mut bar = ErrorBar::new("problem", report)
                .locale(words)
                .on_close(Self::act(cx, |this, _, _| this.session.dismiss_problem()));
            if let Some(action) = self.next_step_button(problem, c, cx) {
                bar = bar.next_step(action);
            }
            bars.push(bar.into_any_element());
        }
        if let Some(repo) = &self.session.repo {
            if repo.branch().is_none() && !repo.status.unborn {
                bars.push(
                    InfoBar::new("detached", Severity::Warning, c.detached)
                        .message(c.detached_warning)
                        .locale(words)
                        .action(Button::standard("detached-branch", c.new_branch).on_click(
                            Self::act(cx, |this, w, cx| this.run_action(Action::NewBranch, w, cx)),
                        ))
                        .into_any_element(),
                );
            }
            let _ = p;
        }
        bars
    }

    fn next_step_button(
        &self,
        problem: &Problem,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Problem::Git { kind, error } = problem else {
            if let Problem::GitHub(github_api::ApiError::Unauthorized) = problem {
                return Some(
                    Button::standard("next-sign-in", c.next_sign_in)
                        .on_click(Self::act(cx, |this, _, _| {
                            this.session.start_sign_in(github_api::Host::github())
                        }))
                        .into_any_element(),
                );
            }
            return None;
        };
        let message = error.message()?;
        let push = matches!(
            kind,
            crate::session::OpKind::Push | crate::session::OpKind::ForcePush
        );
        if !kind.is_network()
            && !matches!(
                kind,
                crate::session::OpKind::Switch | crate::session::OpKind::Merge
            )
        {
            return None;
        }
        let repo = self.session.repo.as_ref()?;
        let step = crate::remote::next_step(
            git_core::classify_refusal(message),
            push,
            self.session.github.account.is_some(),
            repo.hosted.is_some(),
        );
        let (text, run): (&str, fn(&mut GitApp)) = match step {
            crate::remote::NextStep::Pull => (c.next_pull, |a| a.session.pull()),
            crate::remote::NextStep::ForcePush => {
                (c.next_force_push, |a| a.session.ask_force_push())
            }
            crate::remote::NextStep::SignIn => (c.next_sign_in, |a| {
                a.session.start_sign_in(github_api::Host::github())
            }),
            crate::remote::NextStep::Fork => {
                (c.next_fork, |a| a.session.dialog = Some(Dialog::Fork))
            }
            crate::remote::NextStep::StashAndRetry => (c.next_stash, |a| a.session.stash_all()),
            crate::remote::NextStep::PublishBranch => (c.next_publish, |a| a.session.push(false)),
            crate::remote::NextStep::ResolveConflicts | crate::remote::NextStep::None => {
                return None;
            }
        };
        Some(
            Button::standard("next-step", text)
                .on_click(Self::act(cx, move |this, _, _| {
                    this.session.dismiss_problem();
                    run(this)
                }))
                .into_any_element(),
        )
    }

    // --- welcome ---------------------------------------------------------------

    fn welcome(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let mut actions = v_stack().gap(spacing::SM).w(px(360.0));
        for (id, text, icon, action) in [
            (
                "welcome-clone",
                c.clone_repository,
                IconName::Globe,
                Action::CloneRepository,
            ),
            (
                "welcome-new",
                c.new_repository,
                IconName::Plus,
                Action::NewRepository,
            ),
            (
                "welcome-add",
                c.add_local,
                IconName::FolderOpen,
                Action::AddLocalRepository,
            ),
        ] {
            actions = actions.child(
                SettingsCard::new(id, text)
                    .icon(icon)
                    .description(keys::label(action))
                    .on_click(Self::act(cx, move |this, w, cx| {
                        this.run_action(action, w, cx)
                    })),
            );
        }
        if self.session.github.account.is_none() {
            actions = actions.child(
                SettingsCard::new("welcome-sign-in", c.sign_in)
                    .icon(IconName::User)
                    .description(if self.session.sign_in_available() {
                        c.welcome_signed_out
                    } else {
                        c.sign_in_unavailable
                    })
                    .disabled(!self.session.sign_in_available())
                    .on_click(Self::act(cx, |this, _, _| {
                        this.session.start_sign_in(github_api::Host::github())
                    })),
            );
        }
        let mut recent = v_stack().w(px(360.0));
        for (index, path) in self.session.recent.iter().enumerate() {
            let open = path.clone();
            recent = recent.child(
                row(("welcome-recent", index), false, TALL_ROW, p)
                    .child(
                        Icon::new(IconName::Folder)
                            .size(px(16.0))
                            .text_color(p.text_secondary),
                    )
                    .child(
                        v_stack()
                            .min_w_0()
                            .child(label(
                                crate::repositories::folder_name(path),
                                BODY_STRONG,
                                p.text_primary,
                            ))
                            .child(caption(
                                truncate(
                                    &path
                                        .parent()
                                        .map(|x| x.display().to_string())
                                        .unwrap_or_default(),
                                    44,
                                ),
                                p,
                            )),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        this.session.open(open.clone());
                        this.drive(window, cx);
                    })),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .justify_center()
            .pt(px(64.0))
            .gap(px(48.0))
            .child(
                v_stack()
                    .gap(spacing::LG)
                    .child(label(c.welcome_title, TITLE, p.text_primary))
                    .child(label(c.welcome_body, BODY, p.text_secondary))
                    .child(actions),
            )
            .when(!self.session.recent.is_empty(), |d| {
                d.child(
                    v_stack()
                        .gap(spacing::SM)
                        .child(label(c.recent, SUBTITLE, p.text_primary))
                        .child(recent),
                )
            })
    }

    // --- the repository --------------------------------------------------------

    fn repository_view(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let repo = self.session.repo.as_ref().expect("a repository is open");
        // The graph needs the room: its list takes most of the window, and
        // the commit's details and diff the rest, as History's do. Either
        // width is the person's to drag (ticket 123).
        let graph = repo.tab == Tab::Graph;
        let kind = if graph {
            PaneKind::Graph
        } else {
            PaneKind::List
        };
        let view = self.view_width(window);
        let width = crate::pane::width(kind, self.session.pane_width(kind), view);
        let pane = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(width))
            .h_full()
            .relative()
            // The pane reaches the window's bottom leading corner: its fill
            // and edge follow the corner (ticket 141); what sits at its
            // bottom (the commit box, the history's hint) is padded clear of
            // the curve. The edge's width stays in the layout.
            .border_r_1()
            .border_color(gpui::transparent_black())
            .child(window_chrome::corner_fill(p.layer).border(
                gpui::Edges {
                    right: px(1.0),
                    ..gpui::Edges::default()
                },
                p.divider,
            ));
        let pane_contents = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div().p(spacing::SM).child(
                    SelectorBar::new(
                        "tabs",
                        vec![
                            SelectorItem::new(if repo.status.changes.is_empty() {
                                c.show_changes.to_string()
                            } else {
                                format!("{} {}", c.show_changes, repo.status.changes.len())
                            }),
                            SelectorItem::new(c.show_history),
                            SelectorItem::new(crate::graph::graph_copy(self.locale).tab),
                        ],
                        Some(match repo.tab {
                            Tab::Changes => 0,
                            Tab::History => 1,
                            Tab::Graph => 2,
                        }),
                    )
                    .on_select(Self::act_with(
                        cx,
                        |this, index: usize, _, _| {
                            this.session.set_tab(match index {
                                0 => Tab::Changes,
                                1 => Tab::History,
                                _ => Tab::Graph,
                            })
                        },
                    )),
                ),
            )
            .child(match (repo.tab, repo.in_conflict()) {
                (Tab::Changes, true) => self.conflicts_pane(repo, p, c, cx).into_any_element(),
                (Tab::Changes, false) => self.changes_pane(repo, p, c, cx).into_any_element(),
                (Tab::History, _) => self.history_pane(repo, p, c, cx).into_any_element(),
                (Tab::Graph, _) => self.graph_pane(repo, p, c, window, cx).into_any_element(),
            });
        let pane = pane.child(pane_contents);
        let follow = cx.listener(move |this, event: &DragMoveEvent<EdgeDrag>, window, cx| {
            if this.edge_cancelled {
                return;
            }
            let view = this.view_width(window);
            let width = f32::from(event.event.position.x - event.bounds.left());
            this.session
                .set_pane_width(kind, crate::pane::store(kind, width, view));
            this.dragging_edge = true;
            this.edge_dirty = true;
            cx.notify();
        });
        // The view's real width, measured as it is laid out: the pane's
        // limits are a share of it, not of the window with its shadow.
        let measured = self.view_width.clone();
        let probe = canvas(
            move |bounds, window, _| {
                let width = f32::from(bounds.size.width);
                if (measured.get() - width).abs() >= 1.0 {
                    measured.set(width);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        // The drag ends where the button comes up, in the view or anywhere
        // else in the window: the width is saved once, not per move.
        let end = |this: &mut Self, cx: &mut Context<Self>| this.end_edge_drag(cx);
        h_stack()
            .id("repository-view")
            .relative()
            .items_start()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(probe)
            .child(pane)
            // A diff's lines run its whole width, so while they scroll past
            // the window's bottom trailing corner they stop the corner's
            // radius above its edge; the list ends with as much padding, so
            // its last line scrolls fully into view (ticket 141).
            .child(window_chrome::corner_clip(
                px(0.0),
                self.detail_pane(repo, p, c, window, cx),
            ))
            .on_drag_move(follow)
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| end(this, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| end(this, cx)),
            )
            .child(self.pane_edge(kind, width, window, cx))
    }

    /// The repository view's width: as measured, or the window's before the
    /// first layout.
    pub(crate) fn view_width(&self, window: &Window) -> f32 {
        match self.view_width.get() {
            width if width > 0.0 => width,
            _ => f32::from(window.viewport_size().width),
        }
    }

    /// A drag of the list pane's edge is over: save what it set.
    fn end_edge_drag(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.edge_dirty) {
            self.session.save_settings();
        }
        let was = self.dragging_edge || self.edge_drag_start.is_some();
        self.dragging_edge = false;
        self.edge_drag_start = None;
        self.edge_cancelled = false;
        if was {
            cx.notify();
        }
    }

    /// The grab strip on the list pane's right edge, as Better Files' pane
    /// edges are (ticket 118): wider than the divider it lights, a resize
    /// pointer, a double-click back to the default width, Escape during a
    /// drag puts the width back, and a keyboard stop where Left and Right
    /// move it 16px and Enter puts it back. Every change is saved.
    fn pane_edge(
        &self,
        kind: PaneKind,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = fluent::palette(cx);
        let lit =
            (cx.has_active_drag() && self.dragging_edge) || self.edge_focus.is_focused(window);
        let half = PANE_EDGE_HIT_WIDTH / 2.0;
        div()
            .id("pane-edge")
            .group("pane-edge")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(width - half))
            .w(px(PANE_EDGE_HIT_WIDTH))
            .cursor(CursorStyle::ResizeLeftRight)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(half - 1.0))
                    .w(px(2.0))
                    .when(lit, |line| line.bg(p.accent))
                    .group_hover("pane-edge", |line| line.bg(p.accent)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    // Pressed, it is the keyboard's stop too, so the arrow
                    // keys and Escape reach it.
                    this.edge_focus.focus(window, cx);
                    this.edge_drag_start = Some(this.session.pane_width(kind));
                    this.edge_cancelled = false;
                    if event.click_count >= 2 {
                        this.session
                            .set_pane_width(kind, crate::pane::edge(kind, 0.0).reset());
                        this.edge_dirty = true;
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_drag(EdgeDrag, {
                let this = cx.entity().downgrade();
                move |_, _, _, cx| {
                    let _ = this.update(cx, |app, _| app.dragging_edge = true);
                    cx.new(|_| EdgeDrag)
                }
            })
            .track_focus(&self.edge_focus.clone().tab_index(0).tab_stop(true))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    // During a drag: back to the width it started at.
                    if let Some(start) = this.edge_drag_start.filter(|_| this.dragging_edge) {
                        cx.stop_propagation();
                        this.session.set_pane_width(kind, start);
                        this.edge_dirty = false;
                        this.edge_cancelled = true;
                        cx.notify();
                    }
                    return;
                }
                let view = this.view_width(window);
                let now = crate::pane::width(kind, this.session.pane_width(kind), view);
                let stored = match key {
                    "right" => crate::pane::store(kind, now + 16.0, view),
                    "left" => crate::pane::store(kind, now - 16.0, view),
                    "enter" => crate::pane::edge(kind, view).reset(),
                    _ => return,
                };
                cx.stop_propagation();
                this.session.set_pane_width(kind, stored);
                this.session.save_settings();
                cx.notify();
            }))
            .into_any_element()
    }

    fn changes_pane(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let rows = repo.rows();
        let all = crate::changes::Inclusion::summary(&rows);
        let count = repo.status.changes.len();
        let header = h_stack()
            .flex_none()
            .h(ROW_HEIGHT)
            .px(spacing::MD)
            .gap(spacing::SM)
            .border_b_1()
            .border_color(p.divider)
            .child(
                CheckBox::new("all-files", all)
                    .disabled(count == 0 || self.session.selection_locked())
                    .on_change(Self::act_with(cx, |this, _: CheckState, _, _| {
                        this.session.toggle_all()
                    })),
            )
            .child(
                label(
                    if count == 1 {
                        c.one_change.to_string()
                    } else {
                        fill(c.changes_count, &[("n", &count.to_string())])
                    },
                    BODY_STRONG,
                    p.text_primary,
                )
                .flex_1(),
            )
            .child(
                Button::icon_button("toggle-filter", IconName::Search, c.toggle_filter)
                    .checked(self.show_filter)
                    .on_click(Self::act(cx, |this, _, _| {
                        this.show_filter = !this.show_filter
                    })),
            );
        let mut list = v_stack().py(px(1.0));
        for (index, r) in rows.iter().enumerate() {
            list = list.child(self.change_row(index, r, p, cx));
        }
        let stash = self.session.branch_stash().map(|s| s.name.clone());
        v_stack()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(header)
            .when(self.show_filter, |d| {
                d.child(
                    div()
                        .p(spacing::SM)
                        .child(TextBox::search("filter", &self.inputs.filter)),
                )
            })
            .child(
                div()
                    .id("changes-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if count == 0 {
                        v_stack()
                            .p(spacing::LG)
                            .gap(spacing::SM)
                            .child(label(c.no_changes_title, BODY_STRONG, p.text_primary))
                            .child(caption(c.no_changes_body, p))
                            .into_any_element()
                    } else {
                        list.into_any_element()
                    }),
            )
            .children(stash.map(|name| {
                let selected = repo.stash.is_some();
                row("stash-row", selected, ROW_HEIGHT, p)
                    .border_t_1()
                    .border_color(p.divider)
                    .child(
                        Icon::new(IconName::Inbox)
                            .size(px(16.0))
                            .text_color(p.text_secondary),
                    )
                    .child(label(c.stashed_changes, BODY, p.text_primary).flex_1())
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        this.session.view_stash(name.clone());
                        this.drive(window, cx);
                    }))
            }))
            .child(self.commit_form(repo, p, c, cx))
    }

    fn change_row(
        &self,
        index: usize,
        r: &ChangeRow,
        p: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let locked = self.session.selection_locked();
        let path = r.path.clone();
        let toggle = r.path.clone();
        let menu = r.path.clone();
        let display = match &r.original_path {
            Some(original) => format!("{} → {}", original.to_string_lossy(), r.name),
            None => r.name.clone(),
        };
        row(("change", index), r.selected, ROW_HEIGHT, p)
            .child(
                CheckBox::new(("change-check", index), r.check)
                    .disabled(r.conflicted || locked)
                    .on_change(Self::act_with(cx, move |this, _: CheckState, _, _| {
                        this.session.toggle_file(&toggle)
                    })),
            )
            .child(
                h_stack()
                    .flex_1()
                    .min_w_0()
                    .gap(spacing::SM)
                    // The name gives way before the badge does: a long
                    // rename ("old → new") ends in an ellipsis, never under
                    // the badge.
                    .child(
                        label(display, BODY, p.text_primary)
                            .min_w_0()
                            .max_w(px(if r.original_path.is_some() {
                                280.0
                            } else {
                                200.0
                            }))
                            .truncate(),
                    )
                    .child(
                        caption(truncate(&r.folder, FOLDER_CHARS), p)
                            .min_w_0()
                            .truncate(),
                    ),
            )
            .child(div().flex_none().child(badge(r.badge, p)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.is_keyboard() {
                    return;
                }
                this.session.select_file(&path);
                this.drive(window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.context = Some((ContextTarget::Change(menu.clone()), event.position));
                    open_menu(&this.context_menu, None, window, cx);
                    cx.stop_propagation();
                }),
            )
    }

    fn commit_form(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let form = &repo.form;
        let blocked = self.session.commit_blocked();
        // While a commit runs the fields are read-only: what was typed now
        // would be cleared when the commit lands.
        let committing = repo.busy == Some(crate::session::OpKind::Commit);
        let branch = repo.branch();
        let mut form_view = v_stack()
            .flex_none()
            .gap(spacing::SM)
            .p(spacing::MD)
            .border_t_1()
            .border_color(p.divider)
            .child(TextBox::new("summary", &self.inputs.summary).disabled(committing))
            .child(
                TextBox::new("description", &self.inputs.description)
                    .multi_line(3)
                    .disabled(committing),
            );
        if crate::commit::summary_too_long(&form.summary) {
            form_view = form_view.child(caption(c.summary_long, p).text_color(p.caution));
        }
        if form.show_co_authors {
            form_view =
                form_view.child(TextBox::new("co-authors", &self.inputs.co_authors).multi_line(2));
        }
        if repo.github.protected
            && let Some(branch) = branch
        {
            form_view = form_view.child(
                InfoBar::new(
                    "protected",
                    Severity::Warning,
                    fill(c.protected_warning, &[("branch", branch)]),
                )
                .locale(self.words()),
            );
        }
        if form.reading_last {
            form_view = form_view.child(caption(c.reading_last_commit(), p));
        } else if form.amend {
            form_view = form_view.child(caption(c.amending, p).text_color(p.accent));
        }
        if self.show_commit_options {
            form_view = form_view.child(
                v_stack()
                    .gap(px(4.0))
                    .child(
                        CheckBox::new("opt-co", CheckState::from_bool(form.show_co_authors))
                            .label(c.add_co_authors)
                            .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                                if let Some(r) = this.session.repo.as_mut() {
                                    r.form.show_co_authors = s == CheckState::Checked;
                                }
                            })),
                    )
                    .child(
                        CheckBox::new("opt-amend", CheckState::from_bool(form.amend))
                            .label(c.amend)
                            .disabled(repo.status.unborn)
                            .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                                if s == CheckState::Checked {
                                    this.session.amend_last();
                                } else {
                                    this.session.stop_amending();
                                }
                            })),
                    )
                    .child(
                        CheckBox::new("opt-hooks", CheckState::from_bool(form.no_verify))
                            .label(c.bypass_hooks)
                            .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                                if let Some(r) = this.session.repo.as_mut() {
                                    r.form.no_verify = s == CheckState::Checked;
                                }
                            })),
                    )
                    .child(
                        CheckBox::new("opt-empty", CheckState::from_bool(form.allow_empty))
                            .label(c.allow_empty)
                            .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                                if let Some(r) = this.session.repo.as_mut() {
                                    r.form.allow_empty = s == CheckState::Checked;
                                }
                            })),
                    )
                    .child(
                        CheckBox::new("opt-push", CheckState::from_bool(form.push_after))
                            .label(c.push_after)
                            .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                                if let Some(r) = this.session.repo.as_mut() {
                                    r.form.push_after = s == CheckState::Checked;
                                }
                            })),
                    ),
            );
        }
        let reason = blocked.map(|b| match b {
            crate::commit::Blocked::NoSummary => c.why_summary,
            crate::commit::Blocked::NothingIncluded => c.why_nothing,
            crate::commit::Blocked::Busy => c.why_busy,
            crate::commit::Blocked::OperationInProgress => c.why_operation,
            crate::commit::Blocked::BadCoAuthor => c.why_co_author,
        });
        let commit_label = if form.amend {
            c.amend.to_string()
        } else {
            c.commit_label(branch)
        };
        form_view
            .child(
                h_stack()
                    .gap(spacing::SM)
                    .child(
                        // A long branch name ends in an ellipsis; the
                        // tooltip has it whole.
                        div().flex_1().min_w_0().child(
                            Button::accent("commit", commit_label.clone())
                                .full_width()
                                .disabled(blocked.is_some())
                                .tooltip(format!(
                                    "{commit_label} · {}",
                                    reason.unwrap_or(keys::label(Action::Commit))
                                ))
                                .on_click(Self::act(cx, |this, _, _| this.session.commit())),
                        ),
                    )
                    .child(
                        Button::icon_button("commit-options", IconName::Settings, c.commit_options)
                            .checked(self.show_commit_options)
                            .on_click(Self::act(cx, |this, _, _| {
                                this.show_commit_options = !this.show_commit_options
                            })),
                    ),
            )
            .children(
                reason
                    .filter(|_| !repo.status.changes.is_empty() || form.summary.is_empty())
                    .map(|reason| caption(reason, p)),
            )
    }

    fn conflicts_pane(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let operation = match repo.info.operation {
            Some(Operation::Merge) => c.op_merge,
            Some(Operation::SquashMerge) => c.op_squash_merge,
            Some(Operation::Rebase) => c.op_rebase,
            Some(Operation::CherryPick) => c.op_cherry_pick,
            Some(Operation::Revert) => c.op_revert,
            None => c.op_stash,
        };
        // Each side by its name: in a rebase git's "ours" is the branch
        // being rebased onto and "theirs" the person's own commit, so
        // "ours" and "theirs" would read backwards.
        let (ours_name, theirs_name) = match &repo.info.sides {
            Some(sides) => (sides.ours.clone(), sides.theirs.clone()),
            None => (
                repo.branch().unwrap_or("HEAD").to_string(),
                c.side_stash.to_string(),
            ),
        };
        let rows = crate::conflicts::rows(&repo.status, &repo.info.conflict_markers);
        let can_continue = crate::conflicts::can_continue(&repo.status);
        let mut list = v_stack().gap(spacing::SM).p(spacing::SM);
        for (index, r) in rows.iter().enumerate() {
            let describe = match r.kind {
                ConflictKind::BothModified => fill(
                    c.conflict_markers,
                    &[("n", &r.markers.unwrap_or(0).to_string())],
                ),
                ConflictKind::BothAdded => c.conflict_both_added.to_string(),
                ConflictKind::BothDeleted => c.conflict_both_deleted.to_string(),
                ConflictKind::DeletedByUs | ConflictKind::AddedByThem => fill(
                    c.conflict_deleted_in,
                    &[("deleted", &ours_name), ("changed", &theirs_name)],
                ),
                ConflictKind::DeletedByThem | ConflictKind::AddedByUs => fill(
                    c.conflict_deleted_in,
                    &[("deleted", &theirs_name), ("changed", &ours_name)],
                ),
            };
            let (ours, theirs, mark, edit) = (
                r.path.clone(),
                r.path.clone(),
                r.path.clone(),
                repo.root.join(&r.path),
            );
            let markers_left = crate::conflicts::needs_confirmation(r);
            list = list.child(
                v_stack()
                    .gap(spacing::SM)
                    .p(spacing::MD)
                    .rounded(radius::SM)
                    .bg(p.card)
                    .border_1()
                    .border_color(p.card_stroke)
                    .child(
                        h_stack()
                            .gap(spacing::SM)
                            .child(badge(Badge::Conflicted, p))
                            .child(
                                label(
                                    r.path.to_string_lossy().to_string(),
                                    BODY_STRONG,
                                    p.text_primary,
                                )
                                .truncate(),
                            ),
                    )
                    .child(caption(describe, p))
                    .child(
                        h_stack()
                            .gap(spacing::SM)
                            .flex_wrap()
                            .when(r.kind.has_markers(), |d| {
                                d.child(
                                    Button::standard(("conflict-edit", index), c.open_editor)
                                        .on_click(Self::act(cx, move |this, _, _| {
                                            this.open_in_editor(Some(edit.clone()))
                                        })),
                                )
                            })
                            .child(
                                Button::standard(
                                    ("conflict-ours", index),
                                    fill(c.use_side, &[("side", &ours_name)]),
                                )
                                .on_click(Self::act(cx, move |this, _, _| {
                                    this.session.resolve_with(ours.clone(), true)
                                })),
                            )
                            .child(
                                Button::standard(
                                    ("conflict-theirs", index),
                                    fill(c.use_side, &[("side", &theirs_name)]),
                                )
                                .on_click(Self::act(
                                    cx,
                                    move |this, _, _| {
                                        this.session.resolve_with(theirs.clone(), false)
                                    },
                                )),
                            )
                            .when(r.kind.has_markers(), |d| {
                                d.child(
                                    Button::subtle(("conflict-mark", index), c.mark_resolved)
                                        .on_click(Self::act(cx, move |this, _, _| {
                                            this.session
                                                .ask_mark_resolved(mark.clone(), markers_left)
                                        })),
                                )
                            }),
                    ),
            );
        }
        v_stack()
            .flex_1()
            .min_h_0()
            .child(
                v_stack()
                    .p(spacing::MD)
                    .gap(px(4.0))
                    .border_b_1()
                    .border_color(p.divider)
                    .child(label(
                        fill(c.conflicts_title, &[("operation", operation)]),
                        BODY_STRONG,
                        p.text_primary,
                    ))
                    .child(caption(
                        if rows.is_empty() {
                            c.conflicts_none.to_string()
                        } else {
                            fill(c.conflicts_remaining, &[("n", &rows.len().to_string())])
                        },
                        p,
                    )),
            )
            .child(
                div()
                    .id("conflicts")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                h_stack()
                    .p(spacing::MD)
                    .gap(spacing::SM)
                    .border_t_1()
                    .border_color(p.divider)
                    .child(
                        div().flex_1().min_w_0().child(
                            Button::accent("continue", c.continue_op)
                                .full_width()
                                .disabled(!can_continue || self.session.is_busy())
                                .on_click(Self::act(cx, |this, _, _| {
                                    this.session.continue_operation()
                                })),
                        ),
                    )
                    .child(
                        Button::standard("abort", c.abort_op)
                            .disabled(self.session.is_busy())
                            .on_click(Self::act(cx, |this, _, _| this.session.abort_operation())),
                    ),
            )
    }

    fn history_pane(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = crate::github::now();
        let commits: &[Commit] = match &repo.compare {
            Some(compare) => &compare.ahead,
            None => &repo.history.commits,
        };
        let mut list = v_stack().py(px(1.0));
        for (index, commit) in commits.iter().enumerate() {
            let selected = repo.compare.is_none() && repo.history.selected == Some(index);
            let (n, unit) = crate::history::age(now, commit.author_time);
            let tags = commit.tags().join(", ");
            let rewritable = repo.compare.is_none() && self.session.can_rewrite(index);
            let summary: SharedString = commit.summary.clone().into();
            let drop_zone = div()
                .id(("reorder", index))
                .h(px(6.0))
                .mx(spacing::MD)
                .rounded(px(2.0))
                .drag_over::<DraggedCommit>(move |s, _, _, _| s.bg(p.accent))
                .on_drop(
                    cx.listener(move |this, dragged: &DraggedCommit, window, cx| {
                        this.session.reorder(dragged.index, index);
                        this.drive(window, cx);
                    }),
                );
            let mut item = row(("commit", index), selected, TALL_ROW, p)
                .child(
                    v_stack()
                        .id(("commit-subject", index))
                        .flex_1()
                        .min_w_0()
                        .tooltip(better_ui::fluent::dialog::tooltip_builder(summary.clone()))
                        .child(label(commit.summary.clone(), BODY, p.text_primary).truncate())
                        .child(
                            caption(format!("{} · {}{}", commit.author_name, n, unit), p)
                                .truncate(),
                        ),
                )
                // Tags give way to the subject, up to a third of the row.
                .when(!tags.is_empty(), |r| {
                    r.child(
                        label(tags.clone(), CAPTION, p.accent)
                            .flex_shrink(1.0)
                            .min_w(px(24.0))
                            .max_w(relative(0.34))
                            .truncate(),
                    )
                })
                .when(
                    index
                        < repo
                            .status
                            .ahead_behind
                            .map(|(a, _)| a as usize)
                            .unwrap_or(0)
                        && repo.compare.is_none(),
                    |r| {
                        r.child(
                            Icon::new(IconName::ArrowUp)
                                .size(px(12.0))
                                .text_color(p.accent),
                        )
                    },
                );
            if repo.compare.is_none() {
                item = item
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        this.session.select_commit(index);
                        this.drive(window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.context = Some((ContextTarget::Commit(index), event.position));
                            open_menu(&this.context_menu, None, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_drag(
                        DraggedCommit {
                            index,
                            summary: summary.clone(),
                        },
                        |dragged, _, _, cx| cx.new(|_| dragged.clone()),
                    )
                    .when(rewritable, |r| {
                        r.drag_over::<DraggedCommit>(move |s, _, _, _| {
                            s.border_1().border_color(p.accent)
                        })
                        .on_drop(cx.listener(
                            move |this, dragged: &DraggedCommit, window, cx| {
                                this.session.ask_squash(dragged.index, index);
                                this.drive(window, cx);
                            },
                        ))
                    });
                if rewritable {
                    list = list.child(drop_zone);
                }
            }
            list = list.child(item);
        }
        let header = repo.compare.as_ref().map(|compare| {
            let branch = compare.branch.clone();
            let merge = compare.branch.clone();
            v_stack()
                .p(spacing::MD)
                .gap(spacing::SM)
                .border_b_1()
                .border_color(p.divider)
                .child(
                    h_stack()
                        .child(
                            label(
                                fill(c.compare_title, &[("branch", &branch)]),
                                BODY_STRONG,
                                p.text_primary,
                            )
                            .flex_1(),
                        )
                        .child(
                            Button::icon_button("close-compare", IconName::Close, c.close)
                                .kind(ButtonKind::Subtle)
                                .on_click(Self::act(cx, |this, _, _| this.session.close_compare())),
                        ),
                )
                .child(caption(
                    fill(
                        c.compare_ahead,
                        &[("n", &compare.ahead.len().to_string()), ("branch", &branch)],
                    ),
                    p,
                ))
                .child(caption(
                    fill(
                        c.compare_behind,
                        &[("n", &compare.behind.to_string()), ("branch", &branch)],
                    ),
                    p,
                ))
                .child(
                    Button::standard(
                        "compare-merge",
                        fill(c.merge_into, &[("branch", repo.branch().unwrap_or("HEAD"))]),
                    )
                    .disabled(compare.ahead.is_empty())
                    .on_click(Self::act(cx, move |this, _, _| {
                        this.session
                            .branch_picked(BranchPurpose::Merge, merge.clone())
                    })),
                )
        });
        let dragging = cx.has_active_drag();
        let drop_branches = dragging.then(|| {
            let mut strip = v_stack()
                .p(spacing::SM)
                .border_b_1()
                .border_color(p.divider)
                .child(caption(c.cherry_pick_to, p));
            for (i, branch) in repo
                .info
                .branches
                .iter()
                .filter(|b| !b.remote && !b.current)
                .enumerate()
            {
                let name = branch.name.clone();
                strip = strip.child(
                    row(("drop-branch", i), false, px(28.0), p)
                        .child(
                            Icon::empty()
                                .path(BRANCH_ICON)
                                .size(px(12.0))
                                .text_color(p.text_secondary),
                        )
                        .child(label(branch.name.clone(), BODY, p.text_primary))
                        .drag_over::<DraggedCommit>(move |s, _, _, _| {
                            s.bg(with_alpha(p.accent, 0.25))
                        })
                        .on_drop(
                            cx.listener(move |this, dragged: &DraggedCommit, window, cx| {
                                this.session.cherry_pick(dragged.index, Some(name.clone()));
                                this.drive(window, cx);
                            }),
                        ),
                );
            }
            strip
        });
        v_stack()
            .relative()
            .flex_1()
            .min_h_0()
            .children(header)
            .child(
                div()
                    .id("history-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if commits.is_empty() && repo.history.loaded {
                        caption(c.no_commits, p).p(spacing::LG).into_any_element()
                    } else {
                        list.into_any_element()
                    })
                    .when(
                        repo.compare.is_none() && !repo.history.end && repo.history.loaded,
                        |d| {
                            d.child(
                                div().p(spacing::SM).child(
                                    Button::standard("load-more", c.load_more)
                                        .full_width()
                                        .on_click(Self::act(cx, |this, _, _| {
                                            this.session.load_more_history()
                                        })),
                                ),
                            )
                        },
                    ),
            )
            .when(repo.compare.is_none() && !commits.is_empty(), |d| {
                d.child(
                    caption(c.drag_hint, p)
                        .p(spacing::MD)
                        .border_t_1()
                        .border_color(p.divider),
                )
            })
            // Over the list's foot, so the rows under the pointer do not move
            // when a drag starts.
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

    // --- the right side ---------------------------------------------------------

    fn detail_pane(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut pane = v_stack().flex_1().min_w_0().h_full();
        if let Some(stash) = &repo.stash {
            let (restore, drop) = (stash.name.clone(), stash.name.clone());
            pane = pane.child(
                h_stack()
                    .flex_none()
                    .gap(spacing::SM)
                    .p(spacing::MD)
                    .border_b_1()
                    .border_color(p.divider)
                    .child(
                        label(
                            fill(
                                c.stash_for_branch,
                                &[("branch", repo.branch().unwrap_or("HEAD"))],
                            ),
                            BODY_STRONG,
                            p.text_primary,
                        )
                        .flex_1(),
                    )
                    .child(
                        Button::accent("stash-restore", c.restore)
                            .disabled(self.session.is_busy())
                            .on_click(Self::act(cx, move |this, _, _| {
                                this.session.restore_stash(restore.clone())
                            })),
                    )
                    .child(
                        Button::standard("stash-discard", c.discard).on_click(Self::act(
                            cx,
                            move |this, _, _| {
                                this.session.dialog = Some(Dialog::DropStash { name: drop.clone() })
                            },
                        )),
                    )
                    .child(
                        Button::icon_button("stash-close", IconName::Close, c.close)
                            .kind(ButtonKind::Subtle)
                            .on_click(Self::act(cx, |this, _, _| this.session.close_stash())),
                    ),
            );
            pane = pane.child(self.file_strip(
                &stash.files,
                stash.file,
                "stash-file",
                p,
                cx,
                |this, index| this.session.select_stash_file(index),
            ));
        } else if repo.tab == Tab::History
            && let Some(commit) = repo
                .history
                .selected
                .and_then(|i| repo.history.commits.get(i))
        {
            pane = pane.child(self.commit_header(commit, repo, p, c, cx));
            pane = pane.child(self.file_strip(
                &repo.history.files,
                repo.history.file,
                "commit-file",
                p,
                cx,
                |this, index| this.session.select_commit_file(index),
            ));
        } else if repo.tab == Tab::Graph
            && let Some(commit) = repo.graph.selected_commit()
        {
            pane = pane.child(self.commit_header(commit, repo, p, c, cx));
            pane = pane.child(self.file_strip(
                &repo.graph.files,
                repo.graph.file,
                "graph-file",
                p,
                cx,
                |this, index| this.session.select_graph_file(index),
            ));
        } else if repo.tab == Tab::Graph && repo.stash.is_none() {
            return pane.child(div().flex().flex_1().items_center().justify_center().child(
                caption(crate::graph::graph_copy(self.locale).select_hint, p),
            ));
        }
        pane.child(self.diff_view(repo, p, c, window, cx))
    }

    fn file_strip(
        &self,
        files: &[CommitFile],
        selected: Option<usize>,
        id: &'static str,
        p: &'static Palette,
        cx: &mut Context<Self>,
        select: fn(&mut GitApp, usize),
    ) -> impl IntoElement {
        let mut list = v_stack().py(px(1.0));
        for (index, file) in files.iter().enumerate() {
            let kind = match file.kind {
                git_core::ChangeKind::Added => Badge::Added,
                git_core::ChangeKind::Deleted => Badge::Deleted,
                git_core::ChangeKind::Renamed | git_core::ChangeKind::Copied => Badge::Renamed,
                _ => Badge::Modified,
            };
            list = list.child(
                row((id, index), selected == Some(index), px(28.0), p)
                    .child(
                        label(
                            match &file.original_path {
                                Some(original) => format!(
                                    "{} → {}",
                                    original.to_string_lossy(),
                                    file.path.to_string_lossy()
                                ),
                                None => file.path.to_string_lossy().to_string(),
                            },
                            BODY,
                            p.text_primary,
                        )
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                    )
                    .child(badge(kind, p))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.is_keyboard() {
                            return;
                        }
                        select(this, index);
                        this.drive(window, cx);
                    })),
            );
        }
        div()
            .id(id)
            .flex_none()
            .max_h(px(160.0))
            .overflow_y_scroll()
            .border_b_1()
            .border_color(p.divider)
            .child(list)
    }

    fn commit_header(
        &self,
        commit: &Commit,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        let sha = commit.sha.clone();
        let co = commit.co_authors();
        let mut who = commit.author_name.clone();
        for (name, _) in &co {
            who.push_str(", ");
            who.push_str(name);
        }
        let url = repo
            .hosted
            .as_ref()
            .map(|h| format!("{}/commit/{}", h.web_url(), commit.sha));
        let body = commit
            .body
            .lines()
            .filter(|l| git_core::log::parse_co_author(l).is_none())
            .collect::<Vec<_>>()
            .join("\n");
        v_stack()
            .flex_none()
            .gap(px(4.0))
            .p(spacing::MD)
            .border_b_1()
            .border_color(p.divider)
            .child(label(commit.summary.clone(), SUBTITLE, p.text_primary))
            .when(!body.trim().is_empty(), |d| {
                d.child(label(body.clone(), BODY, p.text_secondary))
            })
            .child(
                h_stack()
                    .gap(spacing::SM)
                    .child(caption(format!("{} · {}", who, commit.short_sha()), p))
                    .when(commit.is_merge(), |d| d.child(caption(c.merge_commit, p)))
                    .child(
                        Button::subtle("copy-sha", c.copy_sha)
                            .icon(IconName::Copy)
                            .on_click(Self::act(cx, move |this, _, cx| {
                                this.copy_text(sha.clone(), cx)
                            })),
                    )
                    .children(url.map(|url| {
                        Button::subtle("commit-on-github", c.view_on_github_commit)
                            .icon(IconName::ExternalLink)
                            .on_click(Self::act(cx, move |this, _, _| this.open_url(&url)))
                    })),
            )
    }

    fn diff_view(
        &self,
        repo: &Repo,
        p: &'static Palette,
        c: &'static Copy,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(shown) = &repo.diff else {
            return v_stack()
                .flex_1()
                .items_center()
                .justify_center()
                .child(label(c.select_file, BODY, p.text_secondary));
        };
        let path = match &shown.key {
            DiffKey::Working(path) | DiffKey::Commit { path, .. } | DiffKey::Stash { path, .. } => {
                path.clone()
            }
        };
        let working = matches!(shown.key, DiffKey::Working(_));
        let selectable = self.session.lines_selectable();
        let split = self.session.settings.split_diff;
        let wrap = self.session.settings.wrap_lines;
        // A rename is one file: its header names both paths, as the list
        // row does (ticket 123).
        let original = match &shown.key {
            DiffKey::Working(path) => repo
                .status
                .changes
                .iter()
                .find(|change| &change.path == path)
                .and_then(|change| change.original_path.clone()),
            DiffKey::Commit { path, .. } => {
                let files = if repo.tab == Tab::Graph {
                    &repo.graph.files
                } else {
                    &repo.history.files
                };
                files
                    .iter()
                    .find(|file| &file.path == path)
                    .and_then(|file| file.original_path.clone())
            }
            DiffKey::Stash { path, .. } => repo
                .stash
                .as_ref()
                .and_then(|stash| stash.files.iter().find(|file| &file.path == path))
                .and_then(|file| file.original_path.clone()),
        };
        let renamed = original.is_some();
        let shown_path = match &original {
            Some(original) => format!(
                "{} → {}",
                original.to_string_lossy(),
                path.to_string_lossy()
            ),
            None => path.to_string_lossy().to_string(),
        };
        let header = h_stack()
            .flex_none()
            // A narrow diff (the Graph's details) puts its controls on a
            // second line instead of pushing them out of the pane.
            .min_h(px(44.0))
            .flex_wrap()
            .py(px(4.0))
            .px(spacing::MD)
            .gap(spacing::SM)
            .border_b_1()
            .border_color(p.divider)
            .child(
                label(shown_path, BODY_STRONG, p.text_primary)
                    .flex_1()
                    .min_w(px(160.0))
                    .truncate(),
            )
            .child(
                Button::icon_button(
                    "wrap-lines",
                    Icon::empty().path(better_ui::fluent::glyphs::WRAP),
                    c.wrap_lines,
                )
                .kind(ButtonKind::Subtle)
                .checked(wrap)
                .on_click(Self::act(cx, |this, _, _| {
                    let wrap = this.session.settings.wrap_lines;
                    this.session.set_wrap_lines(!wrap);
                })),
            )
            .child(
                SelectorBar::new(
                    "diff-mode",
                    vec![SelectorItem::new(c.unified), SelectorItem::new(c.split)],
                    Some(usize::from(split)),
                )
                .on_select(Self::act_with(cx, |this, index: usize, _, _| {
                    this.session.set_split_diff(index == 1)
                })),
            )
            .child(
                CheckBox::new(
                    "hide-ws",
                    CheckState::from_bool(self.session.settings.hide_whitespace),
                )
                .label(c.hide_whitespace)
                .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                    this.session.set_hide_whitespace(s == CheckState::Checked)
                })),
            )
            .child(
                Button::icon_button("expand", IconName::Maximize, c.expand_file)
                    .kind(ButtonKind::Subtle)
                    .checked(repo.whole_file)
                    .on_click(Self::act(cx, |this, _, _| {
                        let whole = this.session.repo.as_ref().is_some_and(|r| r.whole_file);
                        this.session.set_whole_file(!whole);
                    })),
            )
            .when(
                working && selectable && repo.inclusion.check(&path) == CheckState::Indeterminate,
                |d| {
                    d.child(
                        Button::standard("discard-lines", c.discard_lines)
                            .on_click(Self::act(cx, |this, _, _| this.session.ask_discard_lines())),
                    )
                },
            );
        let body: AnyElement = match &shown.content {
            DiffContent::Text(diff) => self
                .text_diff(
                    diff,
                    &path,
                    working && selectable,
                    (split, wrap, renamed, shown.version),
                    p,
                    c,
                    window,
                    cx,
                )
                .into_any_element(),
            DiffContent::Large(diff) if repo.show_large => self
                .text_diff(
                    diff,
                    &path,
                    working && selectable,
                    (split, wrap, renamed, shown.version),
                    p,
                    c,
                    window,
                    cx,
                )
                .into_any_element(),
            DiffContent::Large(_) => v_stack()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(spacing::SM)
                .child(label(c.large_diff, BODY_STRONG, p.text_primary))
                .child(caption(c.large_diff_body, p))
                .child(
                    Button::standard("show-large", c.show_diff)
                        .on_click(Self::act(cx, |this, _, _| this.session.show_large_diff())),
                )
                .into_any_element(),
            DiffContent::Image { old, new } => {
                let side = |title: &'static str, bytes: &Option<Vec<u8>>| {
                    v_stack()
                        .flex_1()
                        .items_center()
                        .gap(spacing::SM)
                        .p(spacing::LG)
                        .child(label(title, BODY_STRONG, p.text_primary))
                        .child(match bytes.as_deref().and_then(image) {
                            Some(image) => img(image)
                                .max_w(px(480.0))
                                .max_h(px(480.0))
                                .into_any_element(),
                            None => caption(c.image_none, p).into_any_element(),
                        })
                        .child(caption(
                            bytes
                                .as_ref()
                                .map(|b| fluent::words::bytes(b.len() as u64))
                                .unwrap_or_default(),
                            p,
                        ))
                };
                h_stack()
                    .items_start()
                    .flex_1()
                    .child(side(c.image_before, old))
                    .child(side(c.image_after, new))
                    .into_any_element()
            }
            DiffContent::Conflicted => caption(c.conflicted_diff, p)
                .p(spacing::LG)
                .into_any_element(),
            DiffContent::Failed(error) => {
                let (_, message, details) = c.problem(&Problem::Git {
                    kind: crate::session::OpKind::Refresh,
                    error: error.clone(),
                });
                v_stack()
                    .p(spacing::LG)
                    .gap(spacing::SM)
                    .child(label(message, BODY, p.text_primary))
                    .child(label(details, CODE, p.text_secondary))
                    .into_any_element()
            }
        };
        v_stack()
            .flex_1()
            .min_h_0()
            .child(header)
            .when(working && self.session.settings.hide_whitespace, |d| {
                d.child(
                    caption(c.whitespace_hidden_note, p)
                        .px(spacing::MD)
                        .py(px(4.0)),
                )
            })
            .child(body)
    }

    /// `(split, wrap, renamed, version)`: side by side, lines wrapped,
    /// whether the file is a rename (which says so when its content is
    /// unchanged), and the diff's [`crate::session::ShownDiff::version`].
    #[allow(clippy::too_many_arguments)]
    fn text_diff(
        &self,
        diff: &FileDiff,
        path: &Path,
        selectable: bool,
        (split, wrap, renamed, version): (bool, bool, bool, u64),
        p: &'static Palette,
        c: &'static Copy,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let FileDiff::Text { hunks } = diff else {
            return caption(c.binary_file, p).p(spacing::LG).into_any_element();
        };
        if hunks.is_empty() {
            return caption(
                if renamed {
                    c.renamed_no_content
                } else {
                    c.no_content_changes
                },
                p,
            )
            .p(spacing::LG)
            .into_any_element();
        }
        let mono = cx.theme().mono_font_family.clone();
        // Where lines break: the diff's own width as last laid out (the
        // canvas below measures it), less the gutter, the two number
        // columns and the mark, in columns of the code font.
        let wrap_at = wrap.then(|| {
            let advance = code_advance(window, &mono);
            let measured = self.diff_width.get();
            let width = if measured > 0.0 {
                measured
            } else {
                f32::from(window.viewport_size().width) * 0.6
            };
            let gutter = if selectable {
                f32::from(GUTTER_WIDTH)
            } else {
                0.0
            };
            let number = f32::from(NUMBER_WIDTH);
            // Room for the scroll bar, so the end of a row is never under it.
            let scroll_bar = 12.0;
            let unified = width - gutter - 2.0 * number - 14.0 - scroll_bar;
            let split = (width - 1.0) / 2.0 - gutter - number - 14.0 - scroll_bar / 2.0;
            WrapAt {
                unified: crate::wrap::columns(unified, advance),
                split: crate::wrap::columns(split, advance),
            }
        });
        let key = (version, split, wrap_at.map(|w| (w.unified, w.split)));
        let (rows, texts, hunks) = {
            let mut cache = self.diff_layout.borrow_mut();
            match cache.as_ref() {
                Some(layout) if layout.key == key => {}
                // Same diff, other width or mode: the expanded text stays.
                Some(layout) if layout.key.0 == version => {
                    let rows = flatten(&layout.hunks, &layout.texts, split, wrap_at);
                    let (texts, hunks) = (layout.texts.clone(), layout.hunks.clone());
                    *cache = Some(DiffLayout {
                        key,
                        rows: Arc::new(rows),
                        texts,
                        hunks,
                    });
                }
                _ => {
                    let texts: Vec<Vec<String>> = hunks
                        .iter()
                        .map(|hunk| {
                            hunk.lines
                                .iter()
                                .map(|line| crate::wrap::expand(&line.text))
                                .collect()
                        })
                        .collect();
                    let rows = flatten(hunks, &texts, split, wrap_at);
                    *cache = Some(DiffLayout {
                        key,
                        rows: Arc::new(rows),
                        texts: Arc::new(texts),
                        hunks: Arc::new(hunks.clone()),
                    });
                }
            }
            let layout = cache.as_ref().expect("filled above");
            (
                layout.rows.clone(),
                layout.texts.clone(),
                layout.hunks.clone(),
            )
        };
        // What is ticked, read per row on screen.
        let included: Arc<Option<crate::changes::Include>> = Arc::new(
            self.session
                .repo
                .as_ref()
                .and_then(|r| r.inclusion.include_of(path).cloned()),
        );
        let weak = cx.entity().downgrade();
        let list = uniform_list("diff", rows.len(), move |range, _window, _cx| {
            range
                .map(|index| {
                    diff_row(
                        index,
                        rows[index],
                        &hunks,
                        &texts,
                        &included,
                        selectable,
                        &mono,
                        c,
                        p,
                        weak.clone(),
                    )
                })
                .collect()
        })
        .track_scroll(&self.diff_scroll)
        // The last line scrolls fully above the window's bottom trailing
        // corner (ticket 141).
        .pb(window_chrome::corner_depth(px(0.0), window, cx))
        .size_full();
        // Measures the diff's width as it is laid out; a change (a drag of
        // the pane edge, a resized window) lays the rows out again.
        let measured = self.diff_width.clone();
        let probe = canvas(
            move |bounds, window, _| {
                let width = f32::from(bounds.size.width);
                if (measured.get() - width).abs() >= 1.0 {
                    measured.set(width);
                    if wrap {
                        window.refresh();
                    }
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .child(probe)
            .into_any_element()
    }

    // --- context menu ------------------------------------------------------------

    fn context_menu(&self, c: &'static Copy, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.context_menu.read(cx).is_open() {
            return None;
        }
        let (target, position) = self.context.clone()?;
        let entries = self.context_entries(&target, c, cx);
        // Focus goes back where it was: the open dropdown's search box, or
        // the window, never to the closed menu, where keys reach nothing.
        let back = match self.flyout {
            Some(Open::Repositories) => self.inputs.repo_search.focus_handle(cx),
            Some(Open::Worktrees) => self.inputs.worktree_search.focus_handle(cx),
            Some(Open::Branches) if self.branch_tab == 0 => {
                self.inputs.branch_search.focus_handle(cx)
            }
            _ => self.focus_handle.clone(),
        };
        Some(
            ContextMenu::new("context", entries, self.context_menu.clone(), position)
                .return_focus(back)
                .into_any_element(),
        )
    }

    fn item(
        &self,
        text: impl Into<SharedString>,
        cx: &mut Context<Self>,
        f: impl Fn(&mut GitApp, &mut Window, &mut Context<GitApp>) + 'static,
    ) -> MenuEntry {
        MenuItem::new(text).on_select(Self::act(cx, f)).entry()
    }

    fn context_entries(
        &self,
        target: &ContextTarget,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        let Some(repo) = self.session.repo.as_ref() else {
            if let ContextTarget::Repository(path) = target {
                return self.repository_entries(path.clone(), c, cx);
            }
            return Vec::new();
        };
        match target {
            ContextTarget::Change(path) => {
                let root = repo.root.clone();
                let full = root.join(path);
                let (p1, p2, p3, p4, p5, p6) = (
                    path.clone(),
                    path.clone(),
                    path.clone(),
                    full.clone(),
                    full.clone(),
                    full.display().to_string(),
                );
                let relative = path.to_string_lossy().to_string();
                let mut entries = vec![
                    self.item(c.discard_changes, cx, move |this, _, _| {
                        this.session.ask_discard(vec![p1.clone()])
                    }),
                    MenuEntry::Separator,
                    self.item(c.ignore_file, cx, move |this, _, _| {
                        this.session.ignore_file(&p2)
                    }),
                ];
                if let Some(ext) = path
                    .extension()
                    .map(|e| format!("*.{}", e.to_string_lossy()))
                {
                    entries.push(self.item(
                        fill(c.ignore_extension, &[("ext", &ext)]),
                        cx,
                        move |this, _, _| this.session.ignore_extension(&p3),
                    ));
                }
                entries.extend([
                    MenuEntry::Separator,
                    self.item(c.copy_path, cx, move |this, _, cx| {
                        this.copy_text(p6.clone(), cx)
                    }),
                    self.item(c.copy_relative, cx, move |this, _, cx| {
                        this.copy_text(relative.clone(), cx)
                    }),
                    self.item(c.show_in_folder, cx, move |this, _, _| {
                        this.show_in_files(p4.parent().map(Path::to_path_buf))
                    }),
                    self.item(c.open_file_in_editor, cx, move |this, _, _| {
                        this.open_in_editor(Some(p5.clone()))
                    }),
                ]);
                entries
            }
            ContextTarget::Commit(index) => {
                let Some(commit) = repo.history.commits.get(*index).cloned() else {
                    return Vec::new();
                };
                let menu = crate::commit_menu::CommitMenu {
                    head: *index == 0 && !repo.status.unborn,
                    // History lists HEAD and its ancestors only.
                    ancestor: true,
                    tags: commit.tags().into_iter().map(str::to_string).collect(),
                };
                self.commit_entries(&commit, menu, c, cx)
            }
            ContextTarget::GraphCommit(index) => self.graph_entries(*index, c, cx),
            ContextTarget::GraphLabels(labels) => {
                self.graph_label_entries(labels, c, cx, &|this, name, cx| {
                    this.context_entries(&ContextTarget::Branch(name), c, cx)
                })
            }
            ContextTarget::Branch(name) => {
                let current = repo.branch() == Some(name.as_str());
                let remote_only = repo
                    .info
                    .branches
                    .iter()
                    .any(|b| b.remote && &b.name == name);
                let (n1, n2, n3, n4, n5) = (
                    name.clone(),
                    name.clone(),
                    name.clone(),
                    name.clone(),
                    name.clone(),
                );
                let mut entries = vec![self.item(c.switch, cx, move |this, _, _| {
                    this.flyout = None;
                    this.session.switch_branch(&n1)
                })];
                if !remote_only {
                    entries.push(self.item(c.rename_branch, cx, move |this, _, _| {
                        this.flyout = None;
                        this.session.dialog = Some(Dialog::RenameBranch { from: n2.clone() })
                    }));
                    if !current {
                        entries.push(self.item(c.delete_branch, cx, move |this, _, _| {
                            this.flyout = None;
                            this.session.ask_delete_branch(n3.clone())
                        }));
                    }
                }
                if !current {
                    entries.push(MenuEntry::Separator);
                    entries.push(self.item(
                        fill(c.pick_merge, &[("branch", repo.branch().unwrap_or("HEAD"))]),
                        cx,
                        move |this, _, _| {
                            this.flyout = None;
                            this.session.branch_picked(BranchPurpose::Merge, n4.clone())
                        },
                    ));
                    entries.push(self.item(c.compare_to_branch, cx, move |this, _, _| {
                        this.flyout = None;
                        this.session.compare(n5.clone())
                    }));
                }
                entries
            }
            ContextTarget::Repository(path) => self.repository_entries(path.clone(), c, cx),
            ContextTarget::Worktree(path) => self.worktree_context(path, c, cx),
        }
    }

    fn repository_entries(
        &self,
        path: PathBuf,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<MenuEntry> {
        let (a, b, d, e) = (
            path.clone(),
            path.clone(),
            path.clone(),
            path.display().to_string(),
        );
        let has_alias = self.session.repositories.alias(&path).is_some();
        let mut entries = vec![self.item(c.alias, cx, move |this, _, _| {
            this.flyout = None;
            this.session.dialog = Some(Dialog::Alias { path: a.clone() })
        })];
        if has_alias {
            let clear = path.clone();
            entries.push(self.item(c.remove_alias, cx, move |this, _, _| {
                this.session.set_alias(&clear, "")
            }));
        }
        entries.extend([
            self.item(c.show_in_files, cx, move |this, _, _| {
                this.show_in_files(Some(b.clone()))
            }),
            self.item(c.copy_path, cx, move |this, _, cx| {
                this.copy_text(e.clone(), cx)
            }),
            MenuEntry::Separator,
            self.item(c.remove_repository, cx, move |this, _, _| {
                this.flyout = None;
                this.session.ask_remove_repository(d.clone())
            }),
        ]);
        entries
    }

    // --- dialogs -------------------------------------------------------------------

    fn dialog(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.session.dialog.clone()?;
        let texts = self.dialog_texts(cx);
        let repo = self.session.repo.as_ref();
        // Why the primary button is off, under the field it is about.
        let blocked = self.dialog_blocked(cx);
        let errors = self.field_errors(blocked);
        let fields = &self.inputs.dialog;
        let field = |i: usize, header: &str| {
            TextBox::new(("dialog-field", i), &fields[i])
                .header(header.to_string())
                .error(errors[i])
        };
        let area = |header: &str| {
            TextBox::new("dialog-area", &fields[2])
                .header(header.to_string())
                .multi_line(4)
                .error(errors[2])
        };
        let mut primary: Option<&str>;
        let mut destructive = false;
        let mut content: Vec<AnyElement> = Vec::new();
        let title: String;
        let mut push = |e: AnyElement| content.push(e);
        match &dialog {
            Dialog::Trust { path, programs } => {
                title = c.trust_title.into();
                primary = Some(c.trust_open);
                push(
                    label(path.display().to_string(), BODY_STRONG, p.text_primary)
                        .into_any_element(),
                );
                push(label(c.trust_body, BODY, p.text_secondary).into_any_element());
                if !programs.is_empty() {
                    push(label(c.trust_programs, BODY_STRONG, p.text_primary).into_any_element());
                    for program in programs {
                        push(label(program.clone(), CODE, p.caution).into_any_element());
                    }
                }
            }
            Dialog::AddLocal => {
                title = c.add_local_title.into();
                primary = Some(c.add);
                push(self.folder_field(0, c.folder, c, cx).into_any_element());
            }
            Dialog::Clone { tab, selected } => {
                title = c.clone_title.into();
                primary = Some(c.clone);
                let tab_index = if *tab == CloneTab::GitHub { 0 } else { 1 };
                push(
                    SelectorBar::new(
                        "clone-tabs",
                        vec![
                            SelectorItem::new(c.clone_github_tab),
                            SelectorItem::new(c.clone_url_tab),
                        ],
                        Some(tab_index),
                    )
                    .on_select(Self::act_with(cx, |this, index: usize, _, _| {
                        this.session.dialog = Some(Dialog::Clone {
                            tab: if index == 0 {
                                CloneTab::GitHub
                            } else {
                                CloneTab::Url
                            },
                            selected: None,
                        });
                    }))
                    .into_any_element(),
                );
                if *tab == CloneTab::Url {
                    push(field(0, c.clone_url_label).into_any_element());
                } else if self.session.github.account.is_none() {
                    push(
                        Button::accent("clone-sign-in", c.sign_in)
                            .on_click(Self::act(cx, |this, _, _| {
                                this.session.start_sign_in(github_api::Host::github())
                            }))
                            .into_any_element(),
                    );
                } else {
                    push(TextBox::search("clone-search", &fields[0]).into_any_element());
                    match &self.session.github.repositories {
                        None => push(caption(c.loading, p).into_any_element()),
                        Some(repos) => {
                            let query = texts[0].clone();
                            let mut list = v_stack();
                            for (i, r) in crate::clone::filter(repos, &query)
                                .into_iter()
                                .take(200)
                                .enumerate()
                            {
                                let url = r.clone_url.clone();
                                let is = selected.as_deref() == Some(url.as_str());
                                list = list.child(
                                    row(("clone-repo", i), is, ROW_HEIGHT, p)
                                        .child(
                                            Icon::new(if r.private {
                                                IconName::EyeOff
                                            } else {
                                                IconName::Globe
                                            })
                                            .size(px(14.0))
                                            .text_color(p.text_secondary),
                                        )
                                        .child(
                                            label(r.full_name.clone(), BODY, p.text_primary)
                                                .truncate(),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.session.dialog = Some(Dialog::Clone {
                                                tab: CloneTab::GitHub,
                                                selected: Some(url.clone()),
                                            });
                                            cx.notify();
                                        })),
                                );
                            }
                            push(
                                div()
                                    .id("clone-list")
                                    .max_h(px(240.0))
                                    .overflow_y_scroll()
                                    .child(list)
                                    .into_any_element(),
                            );
                        }
                    }
                }
                push(self.folder_field(1, c.clone_into, c, cx).into_any_element());
            }
            Dialog::NewRepository {
                gitignore,
                license,
                readme,
            } => {
                title = c.new_repository_title.into();
                primary = Some(c.create);
                push(field(0, c.name).into_any_element());
                push(field(2, c.description).into_any_element());
                push(self.folder_field(1, c.local_path, c, cx).into_any_element());
                let gitignores: Vec<String> = if self.session.github.gitignores.is_empty() {
                    publish::GITIGNORES
                        .iter()
                        .map(|(n, _)| n.to_string())
                        .collect()
                } else {
                    self.session.github.gitignores.clone()
                };
                let licenses: Vec<String> = if self.session.github.licenses.is_empty() {
                    publish::LICENSES
                        .iter()
                        .map(|(n, _)| n.to_string())
                        .collect()
                } else {
                    self.session
                        .github
                        .licenses
                        .iter()
                        .map(|(_, n)| n.clone())
                        .collect()
                };
                let items = |names: &[String]| -> Vec<ComboItem> {
                    std::iter::once(ComboItem::new(c.none))
                        .chain(names.iter().map(ComboItem::new))
                        .collect()
                };
                let (g, l, r) = (*gitignore, *license, *readme);
                push(
                    CheckBox::new("readme", CheckState::from_bool(r))
                        .label(c.initialize_readme)
                        .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                            this.session.dialog = Some(Dialog::NewRepository {
                                gitignore: g,
                                license: l,
                                readme: s == CheckState::Checked,
                            })
                        }))
                        .into_any_element(),
                );
                push(
                    ComboBox::new(
                        "gitignore",
                        items(&gitignores),
                        Some(g.map_or(0, |i| i + 1)),
                    )
                    .header(c.gitignore)
                    .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                        this.session.dialog = Some(Dialog::NewRepository {
                            gitignore: i.checked_sub(1),
                            license: l,
                            readme: r,
                        })
                    }))
                    .into_any_element(),
                );
                push(
                    ComboBox::new("license", items(&licenses), Some(l.map_or(0, |i| i + 1)))
                        .header(c.license)
                        .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                            this.session.dialog = Some(Dialog::NewRepository {
                                gitignore: g,
                                license: i.checked_sub(1),
                                readme: r,
                            })
                        }))
                        .into_any_element(),
                );
            }
            Dialog::Publish {
                private,
                organization,
            } => {
                title = c.publish_title.into();
                primary = Some(c.publish);
                push(field(0, c.name).into_any_element());
                push(field(1, c.description).into_any_element());
                let (pr, org) = (*private, *organization);
                push(
                    CheckBox::new("private", CheckState::from_bool(pr))
                        .label(c.keep_private)
                        .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                            this.session.dialog = Some(Dialog::Publish {
                                private: s == CheckState::Checked,
                                organization: org,
                            })
                        }))
                        .into_any_element(),
                );
                if !self.session.github.organizations.is_empty() {
                    let items: Vec<ComboItem> = std::iter::once(ComboItem::new(c.personal_account))
                        .chain(self.session.github.organizations.iter().map(ComboItem::new))
                        .collect();
                    push(
                        ComboBox::new("org", items, Some(org.map_or(0, |i| i + 1)))
                            .header(c.organization)
                            .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                                this.session.dialog = Some(Dialog::Publish {
                                    private: pr,
                                    organization: i.checked_sub(1),
                                })
                            }))
                            .into_any_element(),
                    );
                }
            }
            Dialog::Alias { .. } => {
                title = c.alias_title.into();
                primary = Some(c.save);
                push(field(0, c.name).into_any_element());
            }
            Dialog::RemoveRepository { path } => {
                title = c.remove_repository_title.into();
                primary = Some(c.remove);
                destructive = true;
                push(
                    label(
                        fill(
                            c.remove_repository_body,
                            &[("name", &self.session.repository_name(path))],
                        ),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
            }
            Dialog::NewBranch { start_label, .. } => {
                title = c.new_branch_title.into();
                primary = Some(c.create);
                push(field(0, c.branch_name).into_any_element());
                let base = start_label
                    .clone()
                    .or_else(|| repo.and_then(|r| r.branch().map(str::to_string)))
                    .unwrap_or_else(|| "HEAD".into());
                push(caption(fill(c.branch_from, &[("start", &base)]), p).into_any_element());
                let sanitized = git_core::sanitize_ref_name(&texts[0]);
                if !sanitized.is_empty() && sanitized != texts[0].trim() {
                    push(
                        caption(fill(c.branch_will_be, &[("name", &sanitized)]), p)
                            .into_any_element(),
                    );
                }
            }
            Dialog::RenameBranch { .. } => {
                title = c.rename_branch_title.into();
                primary = Some(c.rename);
                push(field(0, c.branch_name).into_any_element());
            }
            Dialog::DeleteBranch {
                name,
                has_remote,
                delete_remote,
            } => {
                title = c.delete_branch_title.into();
                primary = Some(c.delete);
                destructive = true;
                push(
                    label(
                        fill(c.delete_branch_body, &[("name", name)]),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
                if *has_remote {
                    let (n, h) = (name.clone(), *has_remote);
                    push(
                        CheckBox::new("delete-remote", CheckState::from_bool(*delete_remote))
                            .label(c.delete_remote_too)
                            .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                                this.session.dialog = Some(Dialog::DeleteBranch {
                                    name: n.clone(),
                                    has_remote: h,
                                    delete_remote: s == CheckState::Checked,
                                })
                            }))
                            .into_any_element(),
                    );
                }
            }
            Dialog::SwitchWithChanges { target, bring } => {
                title = c.switch_title.into();
                primary = Some(c.switch);
                push(label(c.switch_body, BODY, p.text_primary).into_any_element());
                let current = repo.and_then(|r| r.branch()).unwrap_or("HEAD").to_string();
                let t2 = target.clone();
                push(
                    RadioButtons::new(
                        "switch-choice",
                        vec![
                            RadioOption::new(fill(c.leave_changes, &[("branch", &current)])),
                            RadioOption::new(fill(c.bring_changes, &[("target", target)])),
                        ],
                        Some(usize::from(*bring)),
                    )
                    .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                        this.session.dialog = Some(Dialog::SwitchWithChanges {
                            target: t2.clone(),
                            bring: i == 1,
                        })
                    }))
                    .into_any_element(),
                );
            }
            Dialog::PickBranch { purpose, selected } => {
                let current = repo.and_then(|r| r.branch()).unwrap_or("HEAD").to_string();
                title = match purpose {
                    BranchPurpose::Merge => fill(c.pick_merge, &[("branch", &current)]),
                    BranchPurpose::SquashMerge => fill(c.pick_squash, &[("branch", &current)]),
                    BranchPurpose::Rebase => fill(c.pick_rebase, &[("branch", &current)]),
                    BranchPurpose::Compare => c.pick_compare.to_string(),
                };
                primary = Some(match purpose {
                    BranchPurpose::Merge => c.merge,
                    BranchPurpose::SquashMerge => c.squash_and_merge,
                    BranchPurpose::Rebase => c.rebase,
                    BranchPurpose::Compare => c.compare,
                });
                push(TextBox::search("pick-search", &fields[0]).into_any_element());
                let mut list = v_stack();
                let purpose = *purpose;
                if let Some(repo) = repo {
                    let rows = crate::branches::rows(
                        &repo.info.branches,
                        repo.default_branch().as_deref(),
                        &repo.recent_branches,
                        &texts[0],
                    );
                    for (i, b) in rows.into_iter().filter(|b| !b.current).enumerate() {
                        let name = b.name.clone();
                        let is = selected.as_deref() == Some(name.as_str());
                        list = list.child(
                            row(("pick", i), is, ROW_HEIGHT, p)
                                .child(
                                    Icon::empty()
                                        .path(BRANCH_ICON)
                                        .size(px(14.0))
                                        .text_color(p.text_secondary),
                                )
                                .child(label(b.name.clone(), BODY, p.text_primary).truncate())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.dialog = Some(Dialog::PickBranch {
                                        purpose,
                                        selected: Some(name.clone()),
                                    });
                                    cx.notify();
                                })),
                        );
                    }
                }
                push(
                    div()
                        .id("pick-list")
                        .max_h(px(280.0))
                        .overflow_y_scroll()
                        .child(list)
                        .into_any_element(),
                );
            }
            Dialog::DiscardFiles { paths } => {
                title = c.discard_title.into();
                primary = Some(c.discard);
                destructive = true;
                push(label(c.discard_body, BODY, p.text_primary).into_any_element());
                for path in paths.iter().take(8) {
                    push(
                        label(path.to_string_lossy().to_string(), CODE, p.text_secondary)
                            .into_any_element(),
                    );
                }
                if paths.len() > 8 {
                    push(
                        caption(
                            fill(c.discard_more, &[("n", &(paths.len() - 8).to_string())]),
                            p,
                        )
                        .into_any_element(),
                    );
                }
            }
            Dialog::DiscardLines { path } => {
                title = c.discard_title.into();
                primary = Some(c.discard);
                destructive = true;
                push(
                    label(
                        fill(c.discard_lines_body, &[("name", &path.to_string_lossy())]),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
            }
            Dialog::UndoCommit => {
                title = c.undo_title.into();
                primary = Some(c.undo);
                push(label(c.undo_body, BODY, p.text_primary).into_any_element());
            }
            Dialog::ResetTo { summary, .. } => {
                title = c.reset_title.into();
                primary = Some(c.reset);
                destructive = true;
                push(
                    label(
                        fill(c.reset_body, &[("summary", summary)]),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
            }
            Dialog::ForcePush { .. } => {
                title = c.force_push_title.into();
                primary = Some(c.force_push);
                destructive = true;
                push(label(c.force_push_body, BODY, p.text_primary).into_any_element());
                // What the remote loses: how many commits, and the newest by
                // sha and subject.
                match &self.session.force_push_loss {
                    None => push(caption(c.force_push_unknown, p).into_any_element()),
                    Some(loss) if loss.total == 0 => {
                        push(caption(c.force_push_loses_none, p).into_any_element())
                    }
                    Some(loss) => {
                        let n = loss.total.to_string();
                        let head = if loss.more_than {
                            fill(c.force_push_loses_many, &[("n", &n)])
                        } else if loss.total == 1 {
                            c.force_push_loses_one.to_string()
                        } else {
                            fill(c.force_push_loses, &[("n", &n)])
                        };
                        let mut list =
                            v_stack()
                                .gap(px(2.0))
                                .child(label(head, BODY_STRONG, p.text_primary));
                        for (sha, summary) in &loss.listed {
                            list = list.child(
                                h_stack()
                                    .gap(spacing::SM)
                                    .child(
                                        t(div().flex_none(), CODE, p.text_secondary)
                                            .child(sha.clone()),
                                    )
                                    .child(
                                        label(summary.clone(), BODY, p.text_primary)
                                            .min_w_0()
                                            .truncate(),
                                    ),
                            );
                        }
                        let rest = loss.total.saturating_sub(loss.listed.len());
                        if rest > 0 && !loss.more_than {
                            list = list
                                .child(caption(fill(c.and_more, &[("n", &rest.to_string())]), p));
                        }
                        push(list.into_any_element());
                    }
                }
            }
            Dialog::CreateTag { .. } => {
                title = c.tag_title.into();
                primary = Some(c.create);
                push(field(0, c.tag_name).into_any_element());
            }
            Dialog::DeleteTag { name } => {
                title = c.delete_tag_title.into();
                primary = Some(c.delete);
                destructive = true;
                push(
                    label(
                        fill(c.delete_tag_body, &[("tag", name)]),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
            }
            Dialog::Squash { .. } => {
                title = c.squash_title.into();
                primary = Some(c.squash);
                push(label(c.squash_body, BODY, p.text_primary).into_any_element());
                push(area(c.title).into_any_element());
            }
            Dialog::AddWorktree { create } => {
                title = c.add_worktree_title.into();
                primary = Some(c.create);
                push(self.folder_field(0, c.folder, c, cx).into_any_element());
                push(field(1, c.worktree_branch).into_any_element());
                push(
                    CheckBox::new("wt-create", CheckState::from_bool(*create))
                        .label(c.worktree_new_branch)
                        .on_change(Self::act_with(cx, |this, s: CheckState, _, _| {
                            this.session.dialog = Some(Dialog::AddWorktree {
                                create: s == CheckState::Checked,
                            })
                        }))
                        .into_any_element(),
                );
            }
            Dialog::RenameWorktree { .. } => {
                title = c.rename_worktree_title.into();
                primary = Some(c.rename);
                push(field(0, c.new_folder).into_any_element());
            }
            Dialog::LockWorktree { path } => {
                title = c.lock_worktree_title().into();
                primary = Some(c.lock());
                push(
                    label(
                        fill(
                            c.lock_worktree_body(),
                            &[("name", &crate::repositories::folder_name(path))],
                        ),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
                push(field(0, c.lock_reason()).into_any_element());
            }
            Dialog::ClearWorktrees { paths } => {
                title = c.clear_worktrees_title().into();
                primary = Some(c.clear());
                if let [path] = paths.as_slice() {
                    push(
                        label(
                            fill(
                                c.clear_worktree_body(),
                                &[("path", &path.display().to_string())],
                            ),
                            BODY,
                            p.text_primary,
                        )
                        .into_any_element(),
                    );
                } else {
                    push(
                        label(
                            fill(c.clear_worktrees_body(), &[("n", &paths.len().to_string())]),
                            BODY,
                            p.text_primary,
                        )
                        .into_any_element(),
                    );
                    for path in paths {
                        push(
                            label(path.display().to_string(), CODE, p.text_primary)
                                .into_any_element(),
                        );
                    }
                }
            }
            Dialog::RemoveWorktree { path, force } => {
                title = c.remove_worktree_title.into();
                primary = Some(c.remove);
                destructive = true;
                push(
                    label(
                        fill(
                            c.remove_worktree_body,
                            &[("path", &path.display().to_string())],
                        ),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
                // What it holds, read off the window's thread when asked.
                let holds = match self.session.worktree_changes_for(path) {
                    None => c.worktree_changes_reading().to_string(),
                    Some(None) => c.worktree_changes_unknown().to_string(),
                    Some(Some(found)) if found.changed + found.untracked == 0 => {
                        c.worktree_changes_none().to_string()
                    }
                    Some(Some(found)) => fill(
                        c.worktree_changes_some(),
                        &[
                            ("changed", &found.changed.to_string()),
                            ("untracked", &found.untracked.to_string()),
                        ],
                    ),
                };
                push(label(holds, BODY, p.text_primary).into_any_element());
                let path = path.clone();
                push(
                    CheckBox::new("wt-force", CheckState::from_bool(*force))
                        .label(c.force_remove)
                        .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                            this.session.dialog = Some(Dialog::RemoveWorktree {
                                path: path.clone(),
                                force: s == CheckState::Checked,
                            })
                        }))
                        .into_any_element(),
                );
            }
            Dialog::Remotes { editing } => {
                title = c.remotes_title.into();
                primary = Some(if editing.is_some() {
                    c.change_url
                } else {
                    c.add_remote
                });
                let mut list = v_stack().gap(spacing::SM);
                if let Some(repo) = repo {
                    for (i, remote) in repo.info.remotes.iter().enumerate() {
                        let (edit, remove) = (remote.name.clone(), remote.name.clone());
                        list = list.child(
                            h_stack()
                                .gap(spacing::SM)
                                .p(spacing::SM)
                                .rounded(radius::SM)
                                .bg(p.card)
                                .child(
                                    v_stack()
                                        .flex_1()
                                        .min_w_0()
                                        .child(label(
                                            remote.name.clone(),
                                            BODY_STRONG,
                                            p.text_primary,
                                        ))
                                        .child(
                                            label(remote.fetch_url.clone(), CODE, p.text_secondary)
                                                .truncate(),
                                        ),
                                )
                                .child(Button::subtle(("remote-edit", i), c.change_url).on_click(
                                    Self::act(cx, move |this, window, cx| {
                                        this.session.dialog = Some(Dialog::Remotes {
                                            editing: Some(edit.clone()),
                                        });
                                        let url = this
                                            .session
                                            .repo
                                            .as_ref()
                                            .and_then(|r| {
                                                r.info.remotes.iter().find(|x| x.name == edit)
                                            })
                                            .map(|r| r.fetch_url.clone())
                                            .unwrap_or_default();
                                        GitApp::set_input(
                                            &this.inputs.dialog[0],
                                            &edit,
                                            window,
                                            cx,
                                        );
                                        GitApp::set_input(&this.inputs.dialog[1], &url, window, cx);
                                    }),
                                ))
                                .child(Button::subtle(("remote-remove", i), c.remove).on_click(
                                    Self::act(cx, move |this, _, _| {
                                        this.session.remove_remote(remove.clone())
                                    }),
                                )),
                        );
                    }
                }
                push(list.into_any_element());
                push(field(0, c.remote_name).into_any_element());
                push(field(1, c.remote_url).into_any_element());
            }
            Dialog::CreatePull { base, draft } => {
                title = c.pull_title.into();
                primary = Some(c.create);
                let head = repo.and_then(|r| r.branch()).unwrap_or("HEAD").to_string();
                let default = repo
                    .and_then(Repo::default_branch)
                    .unwrap_or_else(|| "main".into());
                let remote_names: Vec<String> = repo
                    .map(|r| {
                        r.info
                            .branches
                            .iter()
                            .filter(|b| b.remote)
                            .map(|b| b.name_on_remote().to_string())
                            .filter(|n| *n != head)
                            .collect::<std::collections::BTreeSet<_>>()
                            .into_iter()
                            .collect()
                    })
                    .unwrap_or_default();
                let chosen = base.clone().unwrap_or(default);
                let selected = remote_names.iter().position(|n| *n == chosen);
                let names = remote_names.clone();
                let d = *draft;
                push(caption(fill(c.pull_from_to, &[("head", &head)]), p).into_any_element());
                push(
                    ComboBox::new(
                        "pull-base",
                        remote_names.iter().map(ComboItem::new).collect(),
                        selected,
                    )
                    .on_select(Self::act_with(cx, move |this, i: usize, _, _| {
                        this.session.dialog = Some(Dialog::CreatePull {
                            base: names.get(i).cloned(),
                            draft: d,
                        })
                    }))
                    .into_any_element(),
                );
                push(field(0, c.title).into_any_element());
                push(area(c.body).into_any_element());
                let b = base.clone();
                push(
                    CheckBox::new("pull-draft", CheckState::from_bool(d))
                        .label(c.draft)
                        .on_change(Self::act_with(cx, move |this, s: CheckState, _, _| {
                            this.session.dialog = Some(Dialog::CreatePull {
                                base: b.clone(),
                                draft: s == CheckState::Checked,
                            })
                        }))
                        .into_any_element(),
                );
            }
            Dialog::CreateIssue => {
                title = c.issue_title.into();
                primary = Some(c.create);
                push(field(0, c.title).into_any_element());
                push(area(c.body).into_any_element());
            }
            Dialog::SignIn { enterprise } => {
                title = c.sign_in_title.into();
                primary = None;
                match &self.session.github.sign_in {
                    SignIn::Idle if *enterprise => {
                        primary = Some(c.sign_in);
                        push(field(0, c.enterprise_host).into_any_element());
                        push(field(1, c.enterprise_client_id).into_any_element());
                        push(caption(c.enterprise_client_id_hint, p).into_any_element());
                    }
                    SignIn::Idle | SignIn::Starting => push(
                        h_stack()
                            .gap(spacing::SM)
                            .child(
                                ProgressRing::new("sign-in-ring", Progress::Indeterminate)
                                    .size(px(16.0)),
                            )
                            .child(label(c.sign_in_starting, BODY, p.text_primary))
                            .into_any_element(),
                    ),
                    SignIn::Waiting { code, .. } => {
                        let (code_text, uri) =
                            (code.user_code.clone(), code.verification_uri.clone());
                        push(label(c.sign_in_code, BODY, p.text_primary).into_any_element());
                        push(
                            label(code.user_code.clone(), TITLE, p.text_primary).into_any_element(),
                        );
                        push(
                            Button::accent("sign-in-open", c.sign_in_open)
                                .on_click(Self::act(cx, move |this, _, cx| {
                                    this.copy_text(code_text.clone(), cx);
                                    this.open_url(&uri);
                                }))
                                .into_any_element(),
                        );
                        push(
                            h_stack()
                                .gap(spacing::SM)
                                .child(
                                    ProgressRing::new("sign-in-wait", Progress::Indeterminate)
                                        .size(px(16.0)),
                                )
                                .child(caption(c.sign_in_waiting, p))
                                .into_any_element(),
                        );
                    }
                    SignIn::Failed(problem) => {
                        push(
                            label(c.sign_in_problem(problem), BODY, p.critical).into_any_element(),
                        );
                        let host = self
                            .session
                            .github
                            .host
                            .clone()
                            .unwrap_or_else(github_api::Host::github);
                        push(
                            Button::standard("sign-in-retry", c.try_again)
                                .on_click(Self::act(cx, move |this, _, _| {
                                    this.session.start_sign_in(host.clone())
                                }))
                                .into_any_element(),
                        );
                    }
                }
            }
            Dialog::MarkResolved { path } => {
                title = c.mark_resolved_title.into();
                primary = Some(c.mark_resolved);
                push(
                    label(
                        fill(c.mark_resolved_body, &[("name", &path.to_string_lossy())]),
                        BODY,
                        p.text_primary,
                    )
                    .into_any_element(),
                );
            }
            Dialog::Fork => {
                title = c.fork_title.into();
                primary = Some(c.fork);
                push(label(c.fork_body, BODY, p.text_primary).into_any_element());
            }
            Dialog::DropStash { .. } => {
                title = c.drop_stash_title.into();
                primary = Some(c.discard);
                destructive = true;
                push(label(c.drop_stash_body, BODY, p.text_primary).into_any_element());
            }
            Dialog::Identity => {
                title = c.identity_title().into();
                primary = Some(c.identity_save());
                for element in self.identity_content(
                    &texts,
                    field(0, c.identity_name()),
                    field(1, c.identity_email()),
                    p,
                    c,
                ) {
                    push(element);
                }
            }
        }
        // A reason about the dialog as a whole (nothing chosen in a list).
        if let Some(reason) = blocked.filter(|b| b.field.is_none()) {
            content.push(
                caption(c.invalid_message(reason.reason), p)
                    .text_color(p.critical)
                    .into_any_element(),
            );
        }
        let _ = (window, destructive);
        let buttons = DialogButtons {
            primary: primary.map(SharedString::from),
            secondary: None,
            close: Some(if primary.is_some() { c.cancel } else { c.close }.into()),
            default: if primary.is_some() {
                DefaultButton::Primary
            } else {
                DefaultButton::Close
            },
        };
        let mut dialog_view = ContentDialog::new("dialog", title)
            .buttons(buttons)
            .primary_enabled(blocked.is_none())
            .on_result(Self::act_with(
                cx,
                |this, result: DialogResult, window, cx| {
                    if result == DialogResult::Primary {
                        this.confirm_dialog(window, cx);
                    } else {
                        this.session.cancel_dialog();
                    }
                },
            ));
        for element in content {
            dialog_view = dialog_view.child(element);
        }
        Some(dialog_view.into_any_element())
    }

    fn folder_field(
        &self,
        index: usize,
        header: &str,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Div {
        h_stack()
            .items_end()
            .gap(spacing::SM)
            .child(
                div().flex_1().child(
                    TextBox::new(("folder-field", index), &self.inputs.dialog[index])
                        .header(header.to_string())
                        .error(self.field_errors(self.dialog_blocked(cx))[index]),
                ),
            )
            .child(
                Button::standard(("choose-folder", index), c.choose).on_click(Self::act(
                    cx,
                    move |this, window, cx| {
                        this.choose_folder(window, cx, move |app, path, window, cx| {
                            GitApp::set_input(
                                &app.inputs.dialog[index],
                                &path.display().to_string(),
                                window,
                                cx,
                            );
                        });
                    },
                )),
            )
    }

    // --- settings --------------------------------------------------------------------

    fn settings_page(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entries = vec![
            NavItem::new(c.settings_accounts, IconName::User).entry(),
            NavItem::new(c.settings_integrations, IconName::SquareTerminal).entry(),
            NavItem::new(c.settings_git, IconName::Settings2).entry(),
            NavItem::new(c.settings_appearance, IconName::Palette).entry(),
            NavItem::new(c.settings_prompts, IconName::Info).entry(),
            NavItem::new(c.keyboard_shortcuts, IconName::ALargeSmall).entry(),
        ];
        let s = &self.session.settings;
        let mut page = v_stack().gap(px(4.0)).p(spacing::XL).max_w(px(900.0));
        let card = |id: &'static str, header: &str| SettingsCard::new(id, header.to_string());
        match self.settings_page {
            0 => {
                let account = self.session.github.account.as_ref();
                page =
                    page.child(label(c.settings_accounts, TITLE, p.text_primary).mb(spacing::LG));
                page = page.child(match account {
                    Some(a) => card("account", "GitHub")
                        .icon(IconName::User)
                        .description(fill(
                            c.signed_in_as,
                            &[("login", &a.login), ("host", &a.host.name)],
                        ))
                        .content(
                            Button::standard("sign-out", c.sign_out)
                                .on_click(Self::act(cx, |this, _, _| this.session.sign_out())),
                        ),
                    None => card("account", "GitHub")
                        .icon(IconName::User)
                        .description(if self.session.sign_in_available() {
                            c.not_signed_in
                        } else {
                            c.sign_in_unavailable
                        })
                        .content(
                            h_stack()
                                .gap(spacing::SM)
                                .child(
                                    Button::accent("sign-in", c.sign_in)
                                        .disabled(!self.session.sign_in_available())
                                        .on_click(Self::act(cx, |this, _, _| {
                                            this.session.start_sign_in(github_api::Host::github())
                                        })),
                                )
                                .child(
                                    Button::standard("sign-in-ent", c.sign_in_enterprise)
                                        .disabled(!self.session.sign_in_available())
                                        .on_click(Self::act(cx, |this, _, _| {
                                            this.session.github.sign_in = SignIn::Idle;
                                            this.session.dialog =
                                                Some(Dialog::SignIn { enterprise: true });
                                        })),
                                ),
                        ),
                });
            }
            1 => {
                page = page
                    .child(label(c.settings_integrations, TITLE, p.text_primary).mb(spacing::LG))
                    .child(
                        card("editor", c.editor)
                            .icon(IconName::File)
                            .description(c.editor_hint)
                            .content(
                                div()
                                    .w(px(260.0))
                                    .child(TextBox::new("editor-field", &self.inputs.editor)),
                            ),
                    )
                    .child(
                        card("terminal", c.terminal)
                            .icon(IconName::SquareTerminal)
                            .description(c.terminal_hint)
                            .content(
                                div()
                                    .w(px(260.0))
                                    .child(TextBox::new("terminal-field", &self.inputs.terminal)),
                            ),
                    );
            }
            2 => {
                page = page
                    .child(label(c.settings_git, TITLE, p.text_primary).mb(spacing::SM))
                    .child(caption(c.git_hint, p).mb(spacing::LG))
                    .child(
                        card("git-name", c.git_name).content(
                            div()
                                .w(px(260.0))
                                .child(TextBox::new("git-name-field", &self.inputs.git_name)),
                        ),
                    )
                    .child(
                        card("git-email", c.git_email).content(
                            div()
                                .w(px(260.0))
                                .child(TextBox::new("git-email-field", &self.inputs.git_email)),
                        ),
                    )
                    .child(
                        card("git-branch", c.git_default_branch).content(
                            div()
                                .w(px(260.0))
                                .child(TextBox::new("git-branch-field", &self.inputs.git_branch)),
                        ),
                    )
                    .child(
                        div()
                            .pt(spacing::MD)
                            .flex()
                            .gap(spacing::SM)
                            .child(Button::accent("git-save", c.save).on_click(Self::act(
                                cx,
                                |this, _, cx| {
                                    let values = vec![
                                        (
                                            "user.name".to_string(),
                                            this.inputs.git_name.read(cx).value().to_string(),
                                        ),
                                        (
                                            "user.email".to_string(),
                                            this.inputs.git_email.read(cx).value().to_string(),
                                        ),
                                        (
                                            "init.defaultBranch".to_string(),
                                            this.inputs.git_branch.read(cx).value().to_string(),
                                        ),
                                    ];
                                    this.session.set_git_config(values);
                                },
                            )))
                            .child(
                                // Puts back what git's configuration says, so typed
                                // text has a way out other than saving it.
                                Button::standard("git-revert", c.revert_settings).on_click(
                                    Self::act(cx, |this, window, cx| {
                                        this.revert_git_identity(window, cx)
                                    }),
                                ),
                            ),
                    );
            }
            3 => {
                // What the window is in, which a command-line choice may
                // differ from the stored one in.
                let shown = better_ui::fluent::theme::Appearance::current(cx);
                let theme = Appearance::ALL
                    .iter()
                    .position(|a| a.fluent() == shown)
                    .unwrap_or(0);
                let language = match Locale::from_argument(&s.language) {
                    Some(Locale::EnUs) => 1,
                    Some(Locale::ZhTw) => 2,
                    _ => 0,
                };
                page = page
                    .child(label(c.settings_appearance, TITLE, p.text_primary).mb(spacing::LG))
                    .child(
                        card("theme", c.theme).icon(IconName::Palette).content(
                            RadioButtons::new(
                                "theme-choice",
                                vec![
                                    RadioOption::new(c.theme_system),
                                    RadioOption::new(c.theme_light),
                                    RadioOption::new(c.theme_dark),
                                ],
                                Some(theme),
                            )
                            .on_select(Self::act_with(
                                cx,
                                |this, i: usize, window, cx| {
                                    let a = Appearance::ALL[i.min(2)];
                                    this.set_appearance(a, window, cx);
                                },
                            )),
                        ),
                    )
                    .child(
                        card("language", c.language).icon(IconName::Globe).content(
                            RadioButtons::new(
                                "language-choice",
                                better_ui::fluent::words::language_choices(self.words())
                                    .into_iter()
                                    .map(RadioOption::new)
                                    .collect(),
                                Some(language),
                            )
                            .on_select(Self::act_with(
                                cx,
                                |this, i: usize, _, _| {
                                    this.set_locale(
                                        [Locale::System, Locale::EnUs, Locale::ZhTw][i.min(2)],
                                    )
                                },
                            )),
                        ),
                    );
            }
            4 => {
                let words = self.words();
                let toggle =
                    |id: &'static str, on: bool, f: fn(&mut crate::settings::Settings, bool)| {
                        ToggleSwitch::new(id, on)
                            .locale(words)
                            .on_change(Self::act_with(cx, move |this, v: bool, _, _| {
                                f(&mut this.session.settings, v);
                                this.session.save_settings();
                            }))
                    };
                page = page
                    .child(label(c.settings_prompts, TITLE, p.text_primary).mb(spacing::LG))
                    .child(card("p-discard", c.confirm_discard).content(toggle(
                        "t-discard",
                        s.confirm_discard,
                        |s, v| s.confirm_discard = v,
                    )))
                    .child(card("p-remove", c.confirm_remove).content(toggle(
                        "t-remove",
                        s.confirm_remove_repository,
                        |s, v| s.confirm_remove_repository = v,
                    )))
                    .child(card("p-undo", c.confirm_undo).content(toggle(
                        "t-undo",
                        s.confirm_undo_commit,
                        |s, v| s.confirm_undo_commit = v,
                    )))
                    .child(card("p-fetch", c.background_fetch).content(toggle(
                        "t-fetch",
                        s.background_fetch,
                        |s, v| s.background_fetch = v,
                    )));
            }
            _ => {
                page =
                    page.child(label(c.keyboard_shortcuts, TITLE, p.text_primary).mb(spacing::LG));
                for (action, _, keys_text) in keys::SHORTCUTS {
                    page = page.child(
                        h_stack()
                            .h(px(32.0))
                            .px(spacing::MD)
                            .rounded(radius::SM)
                            .bg(p.card)
                            .child(label(shortcut_name(*action, c), BODY, p.text_primary).flex_1())
                            .child(label(*keys_text, CODE, p.text_secondary)),
                    );
                }
            }
        }
        v_stack()
            .flex_1()
            .min_h_0()
            .child(
                h_stack()
                    .flex_none()
                    .h(px(48.0))
                    .px(spacing::MD)
                    .gap(spacing::SM)
                    .border_b_1()
                    .border_color(p.divider)
                    .child(
                        Button::icon_button("settings-back", IconName::ArrowLeft, c.close)
                            .kind(ButtonKind::Subtle)
                            .on_click(Self::act(cx, |this, _, _| this.session.page = Page::Main)),
                    )
                    .child(label(c.settings, SUBTITLE, p.text_primary)),
            )
            .child(
                div().flex_1().min_h_0().child(
                    NavigationView::new("settings-nav", entries, Some(self.settings_page))
                        .locale(self.words())
                        .on_select(Self::act_with(cx, |this, i: usize, _, _| {
                            this.settings_page = i
                        }))
                        .content(
                            div()
                                .id("settings-scroll")
                                .size_full()
                                .overflow_y_scroll()
                                .child(page),
                        ),
                ),
            )
    }
}

fn shortcut_name(action: Action, c: &'static Copy) -> &'static str {
    match action {
        Action::Settings => c.settings,
        Action::NewRepository => c.new_repository,
        Action::AddLocalRepository => c.add_local,
        Action::CloneRepository => c.clone_repository,
        Action::ShowChanges => c.show_changes,
        Action::ShowHistory => c.show_history,
        Action::ShowGraph => crate::graph::graph_copy_for(c).tab,
        Action::FindInGraph => crate::graph::graph_copy_for(c).shortcut_find,
        Action::RepositoryList => c.repository_list,
        Action::BranchList => c.branch_list,
        Action::WorktreeList => c.worktrees,
        Action::GoToSummary => c.go_to_summary,
        Action::ToggleStash => c.show_stash,
        Action::ToggleFilter => c.toggle_filter,
        Action::Refresh => c.refresh,
        Action::Push => c.push_origin,
        Action::Pull => c.pull_origin,
        Action::Fetch => c.fetch_origin,
        Action::RemoveRepository => c.remove_repository,
        Action::ViewOnGitHub => c.view_on_github,
        Action::OpenInTerminal => c.open_in_terminal,
        Action::ShowInFiles => c.show_in_files,
        Action::OpenInEditor => c.open_in_editor,
        Action::CreateIssue => c.create_issue,
        Action::NewWorktree => c.new_worktree,
        Action::NewBranch => c.new_branch,
        Action::RenameBranch => c.rename_branch,
        Action::DeleteBranch => c.delete_branch,
        Action::DiscardAll => c.discard_all,
        Action::StashAll => c.stash_all,
        Action::UpdateFromDefault => c.compare_to_branch,
        Action::CompareToBranch => c.compare_to_branch,
        Action::MergeIntoCurrent => c.merge_into_current,
        Action::SquashAndMerge => c.squash_merge,
        Action::RebaseCurrent => c.rebase_current,
        Action::CompareOnGitHub => c.compare_on_github,
        Action::ViewBranchOnGitHub => c.view_branch_on_github,
        Action::PullRequest => c.create_pull,
        Action::Commit => c.commit,
    }
}

fn line_colors(kind: DiffLineKind, p: &Palette) -> (Rgba, Rgba, &'static str) {
    match kind {
        DiffLineKind::Added => (with_alpha(p.success, TINT), p.success, "+"),
        DiffLineKind::Removed => (with_alpha(p.critical, TINT), p.critical, "-"),
        DiffLineKind::Context => (with_alpha(p.window_background, 0.0), p.text_secondary, " "),
    }
}

fn number(n: Option<u32>, p: &Palette) -> Div {
    t(
        div()
            .w(NUMBER_WIDTH)
            .flex_none()
            .text_right()
            .pr(spacing::SM),
        CODE,
        p.text_tertiary,
    )
    .child(n.map(|n| n.to_string()).unwrap_or_default())
}

/// One row's part of a line, in the code font. A continuation row starts
/// with nothing in the number columns and an indent marker in the mark's
/// place, drawn by the caller.
fn line_text(text: &str, part: Part, mono: &SharedString, c: &Copy, p: &Palette) -> Div {
    let shown = t(
        div()
            .whitespace_nowrap()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .font_family(mono.clone()),
        CODE,
        p.text_primary,
    )
    .child(text.get(part.from..part.to).unwrap_or_default().to_string());
    if part.hidden == 0 {
        return shown;
    }
    // A line past the row cap says so, rather than ending as if it ended.
    h_stack().flex_1().min_w_0().child(shown).child(
        label(
            fill(c.line_truncated, &[("n", &part.hidden.to_string())]),
            CAPTION,
            p.text_tertiary,
        )
        .flex_none()
        .px(spacing::SM),
    )
}

/// Whether a diff line is ticked for the commit.
fn ticked(include: &Option<crate::changes::Include>, line: &git_core::DiffLine) -> bool {
    match include {
        Some(crate::changes::Include::All) => true,
        Some(crate::changes::Include::Lines { lines, .. }) => {
            lines.contains(&git_core::line_key(line))
        }
        _ => false,
    }
}

/// How far one column of the code font advances at the diff's size,
/// measured by the window's own text system.
fn code_advance(window: &Window, mono: &SharedString) -> f32 {
    let mut font = window.text_style().font();
    font.family = mono.clone();
    const SAMPLE: &str = "0000000000";
    let run = TextRun {
        len: SAMPLE.len(),
        font,
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let width = window
        .text_system()
        .shape_line(SAMPLE.into(), px(CODE.size), &[run], None)
        .width;
    (f32::from(width) / SAMPLE.len() as f32).max(1.0)
}

/// The mark column: `+`, `-` or nothing on a line's first row, and a
/// continuation arrow under it on the rows it wraps onto.
fn mark_cell(mark: &'static str, color: Rgba, first: bool, p: &Palette) -> Div {
    let (text, color) = if first {
        (mark, color)
    } else {
        ("↪", p.text_tertiary)
    };
    t(div().w(px(14.0)).flex_none(), CODE, color).child(text)
}

fn gutter(
    id: ElementId,
    included: Option<bool>,
    p: &'static Palette,
    toggle: Option<Toggle>,
) -> AnyElement {
    let mut g = div()
        .id(id)
        .w(GUTTER_WIDTH)
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    if let Some(on) = included {
        g = g
            .bg(if on {
                with_alpha(p.accent, 0.35)
            } else {
                with_alpha(p.accent, 0.0)
            })
            .border_r_1()
            .border_color(with_alpha(p.accent, 0.6))
            .child(if on {
                Icon::new(IconName::Check)
                    .size(px(10.0))
                    .text_color(p.text_primary)
                    .into_any_element()
            } else {
                div().into_any_element()
            });
    }
    if let Some(toggle) = toggle {
        g = g
            .cursor_pointer()
            .on_click(move |_, window, cx| toggle(window, cx));
    }
    g.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn diff_row(
    index: usize,
    kind: DiffRow,
    hunks: &Arc<Vec<git_core::Hunk>>,
    texts: &Arc<Vec<Vec<String>>>,
    included: &Arc<Option<crate::changes::Include>>,
    selectable: bool,
    mono: &SharedString,
    c: &'static Copy,
    p: &'static Palette,
    weak: WeakEntity<GitApp>,
) -> AnyElement {
    let toggler = |targets: Vec<(usize, usize)>, hunk: Option<usize>| -> Toggle {
        let weak = weak.clone();
        Box::new(move |window, cx| {
            let _ = weak.update(cx, |app, cx| {
                match hunk {
                    Some(h) => app.session.toggle_hunk(h),
                    None => app.session.toggle_lines(&targets),
                }
                app.drive(window, cx);
                cx.notify();
            });
        })
    };
    match kind {
        DiffRow::Hunk(h) => {
            let header = hunks[h].header.clone();
            let mut r = div()
                .id(("hunk", index))
                .w_full()
                .h(DIFF_ROW)
                .flex()
                .items_center()
                .bg(with_alpha(p.accent, 0.08));
            if selectable {
                r = r.child(gutter(
                    ("hunk-g", index).into(),
                    Some(
                        hunks[h]
                            .lines
                            .iter()
                            .any(|l| l.kind != DiffLineKind::Context && ticked(included, l)),
                    ),
                    p,
                    Some(toggler(Vec::new(), Some(h))),
                ));
            }
            r.child(t(div().pl(spacing::SM), CODE, p.text_secondary).child(header))
                .into_any_element()
        }
        DiffRow::Line(h, l, part) => {
            let line = &hunks[h].lines[l];
            let (bg, mark_color, mark) = line_colors(line.kind, p);
            let changed = line.kind != DiffLineKind::Context;
            let mut r = div()
                .id(("line", index))
                .w_full()
                .h(DIFF_ROW)
                .flex()
                .items_center()
                .bg(bg);
            if selectable {
                r = r.child(gutter(
                    ("line-g", index).into(),
                    changed.then(|| ticked(included, line)),
                    p,
                    changed.then(|| toggler(vec![(h, l)], None)),
                ));
            }
            let first = part.first;
            r.child(number(line.old_line.filter(|_| first), p))
                .child(number(line.new_line.filter(|_| first), p))
                .child(mark_cell(mark, mark_color, first, p))
                .child(line_text(&texts[h][l], part, mono, c, p))
                .into_any_element()
        }
        DiffRow::Split(h, pair, left_part, right_part) => {
            let half = |side: Option<usize>, part: Option<Part>, left: bool| -> AnyElement {
                match side {
                    // The line's rows ran out before the other side's: the
                    // rest of this row is the line's color and nothing else.
                    Some(l) if part.is_none() => div()
                        .flex_1()
                        .h_full()
                        .bg(line_colors(hunks[h].lines[l].kind, p).0)
                        .into_any_element(),
                    Some(l) => {
                        let part = part.expect("checked above");
                        let first = part.first;
                        let line = &hunks[h].lines[l];
                        let (bg, mark_color, mark) = line_colors(line.kind, p);
                        let changed = line.kind != DiffLineKind::Context;
                        let mut d = div()
                            .id((if left { "sl" } else { "sr" }, index))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .bg(bg);
                        if selectable && changed {
                            d = d.child(gutter(
                                (if left { "slg" } else { "srg" }, index).into(),
                                Some(ticked(included, line)),
                                p,
                                Some(toggler(vec![(h, l)], None)),
                            ));
                        }
                        let n = if left { line.old_line } else { line.new_line };
                        d.child(number(n.filter(|_| first), p))
                            .child(mark_cell(mark, mark_color, first, p))
                            .child(line_text(&texts[h][l], part, mono, c, p))
                            .into_any_element()
                    }
                    None => div()
                        .flex_1()
                        .h_full()
                        .bg(with_alpha(p.text_primary, 0.03))
                        .into_any_element(),
                }
            };
            div()
                .id(("split", index))
                .w_full()
                .h(DIFF_ROW)
                .flex()
                .child(half(pair.left, left_part, true))
                .child(div().w(px(1.0)).h_full().bg(p.divider))
                .child(half(pair.right, right_part, false))
                .into_any_element()
        }
    }
}

/// The dialog's primary action.
pub(crate) fn confirm(
    app: &mut GitApp,
    dialog: Dialog,
    texts: Vec<String>,
    window: &mut Window,
    cx: &mut Context<GitApp>,
) {
    let text = |i: usize| {
        texts
            .get(i)
            .map(|t| t.trim().to_string())
            .unwrap_or_default()
    };
    let s = &mut app.session;
    match dialog {
        Dialog::Trust { path, .. } => s.trust_and_open(path),
        Dialog::AddLocal => s.add_local(PathBuf::from(text(0))),
        Dialog::Clone { tab, selected } => {
            let url = match tab {
                CloneTab::Url => text(0),
                CloneTab::GitHub => selected.unwrap_or_default(),
            };
            match crate::clone::plan(&url, Path::new(&text(1)), crate::clone::non_empty) {
                Ok((url, destination)) => s.clone_repository(url, destination),
                Err(_) => {
                    s.problem = Some(Problem::Git {
                        kind: crate::session::OpKind::Clone,
                        error: git_platform::GitError::Failed {
                            exit_code: None,
                            message: url,
                        },
                    })
                }
            }
        }
        Dialog::NewRepository {
            gitignore,
            license,
            readme,
        } => {
            let name = publish::sanitize_repository_name(&text(0));
            let parent = PathBuf::from(text(1));
            let description = text(2);
            let signed = s.github.account.is_some() && !s.github.licenses.is_empty();
            let (gitignore_text, gitignore_name) = match gitignore {
                Some(i) if signed || !s.github.gitignores.is_empty() => {
                    (None, s.github.gitignores.get(i).cloned())
                }
                Some(i) => (publish::GITIGNORES.get(i).map(|(_, t)| t.to_string()), None),
                None => (None, None),
            };
            let year = publish::current_year(crate::github::now());
            let owner = s.git_config.get("user.name").to_string();
            let (license_text, license_key) = match license {
                Some(i) if signed => (None, s.github.licenses.get(i).map(|(k, _)| k.clone())),
                Some(i) => (
                    publish::LICENSES
                        .get(i)
                        .map(|(_, t)| publish::fill_license(t, year, &owner)),
                    None,
                ),
                None => (None, None),
            };
            let default_branch = s.git_config.get("init.defaultBranch").to_string();
            s.create_repository(
                git_platform::NewRepository {
                    path: parent.join(&name),
                    default_branch,
                    readme: readme.then(|| publish::readme(&name, &description)),
                    gitignore: gitignore_text,
                    license: license_text,
                },
                gitignore_name,
                license_key,
            );
        }
        Dialog::Publish {
            private,
            organization,
        } => {
            let org = organization.and_then(|i| s.github.organizations.get(i).cloned());
            s.publish(&text(0), &text(1), private, org);
        }
        Dialog::Alias { path } => s.set_alias(&path, &text(0)),
        Dialog::RemoveRepository { path } => s.remove_repository(&path),
        Dialog::NewBranch { start, .. } => s.create_branch(&text(0), start),
        Dialog::RenameBranch { from } => s.rename_branch(from, &text(0)),
        Dialog::DeleteBranch {
            name,
            delete_remote,
            ..
        } => s.delete_branch(name, delete_remote),
        Dialog::SwitchWithChanges { target, bring } => s.switch_with(&target, bring),
        Dialog::PickBranch { purpose, selected } => {
            if let Some(branch) = selected {
                s.branch_picked(purpose, branch);
            }
        }
        Dialog::DiscardFiles { paths } => s.discard(paths),
        Dialog::DiscardLines { .. } => s.discard_lines(),
        Dialog::UndoCommit => s.undo_commit(),
        Dialog::ResetTo { sha, .. } => s.reset_to(sha),
        Dialog::ForcePush { expected } => s.push_with(Some(git_platform::Lease { expected })),
        Dialog::CreateTag { sha } => s.create_tag(sha, &text(0)),
        Dialog::DeleteTag { name } => s.delete_tag(name),
        Dialog::Squash { from, to } => {
            let message = texts.get(2).cloned().unwrap_or_default();
            s.dialog = None;
            s.squash(from, to, &message);
        }
        Dialog::AddWorktree { create } => s.add_worktree(PathBuf::from(text(0)), &text(1), create),
        Dialog::RenameWorktree { path } => s.rename_worktree(path, PathBuf::from(text(0))),
        Dialog::RemoveWorktree { path, force } => s.remove_worktree(path, force),
        Dialog::LockWorktree { path } => s.lock_worktree(path, &text(0)),
        Dialog::ClearWorktrees { .. } => s.clear_worktrees(),
        Dialog::Remotes { editing } => {
            match editing {
                Some(name) => s.set_remote_url(name, &text(1)),
                None if !text(0).is_empty() && !text(1).is_empty() => {
                    s.add_remote(&text(0), &text(1))
                }
                None => {}
            }
            s.dialog = Some(Dialog::Remotes { editing: None });
            for input in app.inputs.dialog.clone() {
                GitApp::set_input(&input, "", window, cx);
            }
        }
        Dialog::CreatePull { base, draft } => {
            let base = base
                .or_else(|| s.repo.as_ref().and_then(Repo::default_branch))
                .unwrap_or_else(|| "main".into());
            let body = texts.get(2).cloned().unwrap_or_default();
            s.create_pull(&text(0), &body, base, draft);
        }
        Dialog::CreateIssue => {
            let body = texts.get(2).cloned().unwrap_or_default();
            s.create_issue(&text(0), &body);
        }
        Dialog::SignIn { enterprise } => {
            if enterprise && let Some(host) = github_api::Host::enterprise(&text(0)) {
                s.start_sign_in(host.with_client_id(&text(1)));
            }
        }
        Dialog::MarkResolved { path } => s.mark_resolved(path),
        Dialog::Fork => s.fork(),
        Dialog::DropStash { name } => s.drop_stash(name),
        Dialog::Identity => s.save_identity(&text(0), &text(1)),
    }
}

#[cfg(test)]
mod wrap_rows {
    use super::{DiffLineKind, DiffRow, WrapAt, flatten};
    use git_core::DiffLine;

    fn line(kind: DiffLineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
            old_line: Some(1),
            new_line: Some(1),
            no_newline_at_end: false,
        }
    }

    fn hunk() -> git_core::Hunk {
        git_core::Hunk {
            header: "@@ -1,2 +1,1 @@".into(),
            lines: vec![
                line(DiffLineKind::Removed, "abcdefghij"),
                line(DiffLineKind::Removed, "ab"),
                line(DiffLineKind::Added, "abcd"),
            ],
        }
    }

    fn texts(hunks: &[git_core::Hunk]) -> Vec<Vec<String>> {
        hunks
            .iter()
            .map(|h| {
                h.lines
                    .iter()
                    .map(|l| crate::wrap::expand(&l.text))
                    .collect()
            })
            .collect()
    }

    /// (line, from, to, first) of each line row.
    fn unified(rows: &[DiffRow]) -> Vec<(usize, usize, usize, bool)> {
        rows.iter()
            .filter_map(|r| match r {
                DiffRow::Line(_, l, p) => Some((*l, p.from, p.to, p.first)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_wrapped_line_is_several_rows_and_only_the_first_is_numbered() {
        let hunks = [hunk()];
        let t = texts(&hunks);
        let at = WrapAt {
            unified: 4,
            split: 3,
        };
        let rows = flatten(&hunks, &t, false, Some(at));
        assert_eq!(
            unified(&rows),
            [
                (0, 0, 4, true),
                (0, 4, 8, false),
                (0, 8, 10, false),
                (1, 0, 2, true),
                (2, 0, 4, true),
            ]
        );
        // Unwrapped, each line is one row with all of its text.
        let rows = flatten(&hunks, &t, false, None);
        assert_eq!(
            unified(&rows),
            [(0, 0, 10, true), (1, 0, 2, true), (2, 0, 4, true)]
        );
    }

    #[test]
    fn side_by_side_rows_take_the_taller_side_and_pad_the_other() {
        let hunks = [hunk()];
        let t = texts(&hunks);
        let at = WrapAt {
            unified: 4,
            split: 3,
        };
        let rows = flatten(&hunks, &t, true, Some(at));
        let halves: Vec<(Option<usize>, Option<usize>)> = rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Split(_, _, l, r) => Some((l.map(|p| p.to), r.map(|p| p.to))),
                _ => None,
            })
            .collect();
        // "abcdefghij" against "abcd": four rows on the left, two on the
        // right, padded; then "ab" with nothing beside it.
        assert_eq!(
            halves,
            [
                (Some(3), Some(3)),
                (Some(6), Some(4)),
                (Some(9), None),
                (Some(10), None),
                (Some(2), None),
            ]
        );
    }
}

#[cfg(test)]
mod wrap_cost {
    use super::{DiffLineKind, WrapAt, flatten};
    use git_core::DiffLine;

    /// What laying out a 50,000-line diff costs, which before the cache was
    /// paid on every frame and now only when the diff, its width or its mode
    /// changes. Run with `--ignored --nocapture` to read the numbers.
    #[test]
    #[ignore]
    fn laying_out_fifty_thousand_lines() {
        let lines: Vec<DiffLine> = (0..50_000)
            .map(|i| DiffLine {
                kind: if i % 3 == 0 {
                    DiffLineKind::Added
                } else {
                    DiffLineKind::Context
                },
                text: format!("\tlet value_{i} = compute(\"{}\");", "x".repeat(i % 160)),
                old_line: Some(i as u32),
                new_line: Some(i as u32),
                no_newline_at_end: false,
            })
            .collect();
        let hunks = vec![git_core::Hunk {
            header: "@@ -1,50000 +1,50000 @@".into(),
            lines,
        }];
        let at = WrapAt {
            unified: 100,
            split: 48,
        };
        let started = std::time::Instant::now();
        let texts: Vec<Vec<String>> = hunks
            .iter()
            .map(|h| {
                h.lines
                    .iter()
                    .map(|l| crate::wrap::expand(&l.text))
                    .collect()
            })
            .collect();
        let rows = flatten(&hunks, &texts, false, Some(at));
        let full = started.elapsed();
        let started = std::time::Instant::now();
        let split = flatten(&hunks, &texts, true, Some(at));
        let relayout = started.elapsed();
        let cached = std::sync::Arc::new(rows);
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(cached.clone());
        }
        let hit = started.elapsed() / 1000;
        println!(
            "50k lines: expand+wrap {full:?} ({} rows), split re-wrap {relayout:?} ({} rows), cache hit {hit:?}",
            cached.len(),
            split.len()
        );
    }
}

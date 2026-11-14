//! Drawing the session in `better_ui::fluent`'s controls, laid out the way
//! GitHub Desktop is: a toolbar with the repository, the branch, and the
//! sync button; a 320px pane with Changes and History; and the diff.
//!
//! Nothing here decides anything. Every state shown is read from the
//! session, and every click calls the session or one of [`GitApp`]'s
//! methods.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use git_core::{Commit, CommitFile, ConflictKind, DiffLine, DiffLineKind, FileDiff, Operation};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName, Sizable as _};

use better_ui::fluent::button::{Button, ButtonKind};
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
use crate::remote::SyncAction;
use crate::session::{DiffContent, DiffKey, Page, Problem, Repo, Tab};
use crate::{Appearance, publish};

const TOOLBAR_HEIGHT: Pixels = px(56.0);
const PANE_WIDTH: Pixels = px(320.0);
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

/// What a click on a diff gutter does.
type Toggle = Box<dyn Fn(&mut Window, &mut App)>;

/// One row of the flattened diff.
#[derive(Clone, Copy)]
enum DiffRow {
    Hunk(usize),
    Line(usize, usize),
    Split(usize, git_core::SplitRow),
}

fn flatten(hunks: &[git_core::Hunk], split: bool) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for (h, hunk) in hunks.iter().enumerate() {
        rows.push(DiffRow::Hunk(h));
        if split {
            rows.extend(
                git_core::split_rows(hunk)
                    .into_iter()
                    .map(|r| DiffRow::Split(h, r)),
            );
        } else {
            rows.extend((0..hunk.lines.len()).map(|l| DiffRow::Line(h, l)));
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
        let p = fluent::palette(cx);
        let c = self.copy();
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
        self.toast
            .observe(self.session.notice.as_ref(), crate::toast::now_ms());
        let toast = self.toast(p, cx);
        div()
            .id("better-git")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window_background)
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
            .children(toast)
            .children(dialog)
            .children(context)
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

    fn toolbar_item(
        &self,
        id: &'static str,
        title: &str,
        value: String,
        icon: Icon,
        open: bool,
        p: &'static Palette,
    ) -> Stateful<Div> {
        let hover = p.subtle_fill;
        div()
            .id(id)
            .relative()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .h_full()
            .min_w(px(200.0))
            .max_w(px(300.0))
            .px(spacing::LG)
            .border_r_1()
            .border_color(p.divider)
            .when(open, |d| d.bg(p.subtle_fill))
            .hover(move |s| s.bg(hover))
            .child(icon.size(px(16.0)).text_color(p.text_secondary))
            .child(
                v_stack()
                    .flex_1()
                    .min_w_0()
                    .child(caption(title.to_string(), p))
                    .child(label(value, BODY_STRONG, p.text_primary).truncate()),
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
        let repo_name = repo
            .map(|r| {
                self.session
                    .repositories
                    .alias(&r.root)
                    .map(str::to_string)
                    .unwrap_or_else(|| r.name())
            })
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
                repo_name,
                Icon::new(IconName::Folder),
                self.flyout == Some(Open::Repositories),
                p,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.flyout = if this.flyout == Some(Open::Repositories) {
                    None
                } else {
                    this.session.read_worktrees();
                    Some(Open::Repositories)
                };
                this.drive(window, cx);
            }))
            .when(self.flyout == Some(Open::Repositories), |d| {
                d.child(self.repository_flyout(p, c, cx))
            });
        bar = bar.child(repos);

        if let Some(repo) = repo {
            let branch = repo
                .branch()
                .map(str::to_string)
                .unwrap_or_else(|| c.detached.to_string());
            let branches = self
                .toolbar_item(
                    "toolbar-branch",
                    c.current_branch,
                    branch,
                    Icon::empty().path(BRANCH_ICON),
                    self.flyout == Some(Open::Branches),
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.flyout = if this.flyout == Some(Open::Branches) {
                        None
                    } else {
                        Some(Open::Branches)
                    };
                    cx.notify();
                }))
                .when(self.flyout == Some(Open::Branches), |d| {
                    d.child(self.branch_flyout(p, c, window, cx))
                });
            bar = bar.child(branches);
            bar = bar.child(self.sync_item(repo, p, c, cx));
        }

        bar = bar.child(div().flex_1());
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
                (c.current_repository, &repo_label),
                (c.current_branch, &branch_name),
            ],
            &labels,
            repo.is_some_and(|r| r.github.checks.is_some()),
        );
        bar = bar.child(self.shortcut_buttons(compact, p, cx));
        if let Some(repo) = repo {
            bar = bar.children(self.checks_item(repo, p, c, cx));
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
        let title = match (busy, repo.busy) {
            (true, Some(crate::session::OpKind::Pull)) => c.pulling,
            (true, Some(crate::session::OpKind::Push | crate::session::OpKind::ForcePush)) => {
                c.pushing
            }
            (true, _) => c.fetching,
            _ => title,
        };
        let hover = p.subtle_fill;
        div()
            .id("toolbar-sync")
            .flex()
            .items_center()
            .gap(spacing::MD)
            .h_full()
            .min_w(px(200.0))
            .px(spacing::LG)
            .border_r_1()
            .border_color(p.divider)
            .hover(move |s| s.bg(hover))
            .child(if busy {
                ProgressRing::new("sync-ring", Progress::Indeterminate)
                    .size(px(16.0))
                    .into_any_element()
            } else {
                Icon::new(icon)
                    .size(px(16.0))
                    .text_color(p.text_secondary)
                    .into_any_element()
            })
            .child(
                v_stack()
                    .flex_1()
                    .child(label(title, BODY_STRONG, p.text_primary))
                    .child(caption(fetched, p)),
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
            }))
    }

    fn checks_item(
        &self,
        repo: &Repo,
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
            .gap(spacing::SM)
            .px(spacing::MD)
            .child(Icon::new(icon).size(px(16.0)).text_color(color))
            .child(label(text, BODY, p.text_primary));
        if overall == github_api::CheckState::Failure {
            item = item.child(
                Button::standard("rerun-checks", c.rerun_checks)
                    .on_click(Self::act(cx, |this, _, _| this.session.rerun_checks())),
            );
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
        let current = self.session.repo.as_ref().map(|r| r.root.as_path());
        let rows =
            self.session
                .repositories
                .rows(&query, current, &self.session.recent, Path::exists);
        let mut list = v_stack()
            .max_h(px(420.0))
            .id("repo-list")
            .overflow_y_scroll();
        for (index, r) in rows.into_iter().enumerate() {
            let path = r.path.clone();
            let open_path = r.path.clone();
            let menu_path = r.path.clone();
            let mut item = row(("repo", index), r.current, TALL_ROW, p)
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
            } else {
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
            // Its linked worktrees, under it.
            let worktrees = self.worktree_rows(&r.path, index * 100, p, cx);
            list = list.children(worktrees);
        }
        Flyout::new("repository-flyout")
            .on_dismiss(Self::act(cx, |this, _, _| this.flyout = None))
            .child(
                v_stack()
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
            v_stack()
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
        Flyout::new("branch-flyout")
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
    ) -> Div {
        let repo = self.session.repo.as_ref().expect("a repository is open");
        let narrow = window.viewport_size().width < px(1100.0);
        // The graph needs the room: its list takes most of the window, and
        // the commit's details and diff the rest, as History's do.
        let graph = repo.tab == Tab::Graph;
        let pane = div()
            .flex()
            .flex_col()
            .flex_none()
            .when(graph, |d| d.w(relative(crate::graph_pane::GRAPH_SHARE)))
            .when(!graph, |d| d.w(if narrow { px(300.0) } else { PANE_WIDTH }))
            .h_full()
            .border_r_1()
            .border_color(p.divider)
            .bg(p.layer)
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
        h_stack()
            .items_start()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(pane)
            .child(self.detail_pane(repo, p, c, cx))
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
                    .child(
                        label(display, BODY, p.text_primary)
                            .flex_none()
                            .max_w(px(200.0))
                            .truncate(),
                    )
                    .child(caption(truncate(&r.folder, FOLDER_CHARS), p).truncate()),
            )
            .child(badge(r.badge, p))
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
                        div().flex_1().child(
                            Button::accent("commit", commit_label)
                                .full_width()
                                .disabled(blocked.is_some())
                                .tooltip(reason.unwrap_or(keys::label(Action::Commit)))
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
                        div().flex_1().child(
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
                        .flex_1()
                        .min_w_0()
                        .child(label(commit.summary.clone(), BODY, p.text_primary).truncate())
                        .child(
                            caption(format!("{} · {}{}", commit.author_name, n, unit), p)
                                .truncate(),
                        ),
                )
                .when(!tags.is_empty(), |r| {
                    r.child(label(tags.clone(), CAPTION, p.accent))
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
        pane.child(self.diff_view(repo, p, c, cx))
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
                            file.path.to_string_lossy().to_string(),
                            BODY,
                            p.text_primary,
                        )
                        .flex_1()
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
        let header = h_stack()
            .flex_none()
            .h(px(44.0))
            .px(spacing::MD)
            .gap(spacing::SM)
            .border_b_1()
            .border_color(p.divider)
            .child(
                label(
                    path.to_string_lossy().to_string(),
                    BODY_STRONG,
                    p.text_primary,
                )
                .flex_1()
                .truncate(),
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
                .text_diff(diff, &path, working && selectable, split, p, c, cx)
                .into_any_element(),
            DiffContent::Large(diff) if repo.show_large => self
                .text_diff(diff, &path, working && selectable, split, p, c, cx)
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

    #[allow(clippy::too_many_arguments)]
    fn text_diff(
        &self,
        diff: &FileDiff,
        path: &Path,
        selectable: bool,
        split: bool,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let FileDiff::Text { hunks } = diff else {
            return caption(c.binary_file, p).p(spacing::LG).into_any_element();
        };
        if hunks.is_empty() {
            return caption(c.no_content_changes, p)
                .p(spacing::LG)
                .into_any_element();
        }
        let rows = flatten(hunks, split);
        let hunks = Arc::new(hunks.clone());
        let included: Arc<Vec<Vec<bool>>> = Arc::new(
            hunks
                .iter()
                .map(|hunk| {
                    hunk.lines
                        .iter()
                        .map(|line| {
                            self.session
                                .repo
                                .as_ref()
                                .is_some_and(|r| r.inclusion.line_included(path, line))
                        })
                        .collect()
                })
                .collect(),
        );
        let weak = cx.entity().downgrade();
        uniform_list("diff", rows.len(), move |range, _window, _cx| {
            range
                .map(|index| {
                    diff_row(
                        index,
                        rows[index],
                        &hunks,
                        &included,
                        selectable,
                        p,
                        weak.clone(),
                    )
                })
                .collect()
        })
        .track_scroll(&self.diff_scroll)
        .flex_1()
        .w_full()
        .into_any_element()
    }

    // --- context menu ------------------------------------------------------------

    fn context_menu(&self, c: &'static Copy, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.context_menu.read(cx).is_open() {
            return None;
        }
        let (target, position) = self.context.clone()?;
        let entries = self.context_entries(&target, c, cx);
        Some(
            ContextMenu::new("context", entries, self.context_menu.clone(), position)
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
                            &[("name", &crate::repositories::folder_name(path))],
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
            Dialog::Worktrees => {
                title = c.worktrees_title.into();
                primary = None;
                let mut list = v_stack().gap(spacing::SM);
                if let Some(repo) = repo {
                    for (i, w) in repo.info.worktrees.iter().enumerate() {
                        let (open, rename, remove) =
                            (w.path.clone(), w.path.clone(), w.path.clone());
                        let current = w.path == repo.root;
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
                                        .child(
                                            label(
                                                w.path.display().to_string(),
                                                BODY_STRONG,
                                                p.text_primary,
                                            )
                                            .truncate(),
                                        )
                                        .child(caption(
                                            format!(
                                                "{}{}",
                                                w.branch
                                                    .clone()
                                                    .unwrap_or_else(|| c.detached.to_string()),
                                                if w.main {
                                                    format!(" · {}", c.worktree_main)
                                                } else {
                                                    String::new()
                                                }
                                            ),
                                            p,
                                        )),
                                )
                                .when(!current, |d| {
                                    d.child(
                                        Button::standard(("wt-open", i), c.worktree_open).on_click(
                                            Self::act(cx, move |this, _, _| {
                                                this.session.dialog = None;
                                                this.session.open(open.clone())
                                            }),
                                        ),
                                    )
                                })
                                .when(!w.main, |d| {
                                    d.child(Button::subtle(("wt-rename", i), c.rename).on_click(
                                        Self::act(cx, move |this, _, _| {
                                            this.session.dialog = Some(Dialog::RenameWorktree {
                                                path: rename.clone(),
                                            })
                                        }),
                                    ))
                                    .child(
                                        Button::subtle(("wt-remove", i), c.remove).on_click(
                                            Self::act(cx, move |this, _, _| {
                                                this.session.dialog = Some(Dialog::RemoveWorktree {
                                                    path: remove.clone(),
                                                    force: false,
                                                })
                                            }),
                                        ),
                                    )
                                }),
                        );
                    }
                }
                push(list.into_any_element());
                push(
                    Button::standard("wt-add", c.add_worktree)
                        .icon(IconName::Plus)
                        .on_click(Self::act(cx, |this, _, _| {
                            this.session.dialog = Some(Dialog::AddWorktree { create: true })
                        }))
                        .into_any_element(),
                );
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
                    .child(div().pt(spacing::MD).child(
                        Button::accent("git-save", c.save).on_click(Self::act(
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
                        )),
                    ));
            }
            3 => {
                let theme = match Appearance::from_argument(&s.appearance).unwrap_or_default() {
                    Appearance::Dark => 0,
                    Appearance::Light => 1,
                    Appearance::System => 2,
                };
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
                                    RadioOption::new(c.theme_dark),
                                    RadioOption::new(c.theme_light),
                                    RadioOption::new(c.theme_system),
                                ],
                                Some(theme),
                            )
                            .on_select(Self::act_with(
                                cx,
                                |this, i: usize, window, cx| {
                                    let a =
                                        [Appearance::Dark, Appearance::Light, Appearance::System]
                                            [i.min(2)];
                                    this.set_appearance(a, window, cx);
                                },
                            )),
                        ),
                    )
                    .child(
                        card("language", c.language).icon(IconName::Globe).content(
                            RadioButtons::new(
                                "language-choice",
                                vec![
                                    RadioOption::new(c.language_system),
                                    RadioOption::new("English"),
                                    RadioOption::new("繁體中文"),
                                ],
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
                    .child(card("p-force", c.confirm_force_push).content(toggle(
                        "t-force",
                        s.confirm_force_push,
                        |s, v| s.confirm_force_push = v,
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

fn line_text(line: &DiffLine, p: &Palette) -> Div {
    t(
        div().whitespace_nowrap().flex_1().overflow_hidden(),
        CODE,
        p.text_primary,
    )
    .child(line.text.replace('\t', "    "))
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

fn diff_row(
    index: usize,
    kind: DiffRow,
    hunks: &Arc<Vec<git_core::Hunk>>,
    included: &Arc<Vec<Vec<bool>>>,
    selectable: bool,
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
                        included[h]
                            .iter()
                            .zip(&hunks[h].lines)
                            .any(|(on, l)| *on && l.kind != DiffLineKind::Context),
                    ),
                    p,
                    Some(toggler(Vec::new(), Some(h))),
                ));
            }
            r.child(t(div().pl(spacing::SM), CODE, p.text_secondary).child(header))
                .into_any_element()
        }
        DiffRow::Line(h, l) => {
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
                    changed.then(|| included[h][l]),
                    p,
                    changed.then(|| toggler(vec![(h, l)], None)),
                ));
            }
            r.child(number(line.old_line, p))
                .child(number(line.new_line, p))
                .child(t(div().w(px(14.0)).flex_none(), CODE, mark_color).child(mark))
                .child(line_text(line, p))
                .into_any_element()
        }
        DiffRow::Split(h, pair) => {
            let half = |side: Option<usize>, left: bool| -> AnyElement {
                match side {
                    Some(l) => {
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
                                Some(included[h][l]),
                                p,
                                Some(toggler(vec![(h, l)], None)),
                            ));
                        }
                        d.child(number(if left { line.old_line } else { line.new_line }, p))
                            .child(t(div().w(px(14.0)).flex_none(), CODE, mark_color).child(mark))
                            .child(line_text(line, p))
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
                .child(half(pair.left, true))
                .child(div().w(px(1.0)).h_full().bg(p.divider))
                .child(half(pair.right, false))
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
        Dialog::Worktrees => s.dialog = None,
        Dialog::AddWorktree { create } => s.add_worktree(PathBuf::from(text(0)), &text(1), create),
        Dialog::RenameWorktree { path } => s.rename_worktree(path, PathBuf::from(text(0))),
        Dialog::RemoveWorktree { path, force } => s.remove_worktree(path, force),
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

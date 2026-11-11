//! Drawing the session: `design/compiled-spec.md`, in the tokens of
//! `better_ui::fluent`.
//!
//! Nothing here decides anything. Every state shown is read from the session,
//! and every click calls one of [`GitApp`]'s methods.

use git_core::{DiffLine, DiffLineKind, Hunk};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, h_flex, v_flex};

use better_ui::fluent::{self, Palette, radius, size as fsize, spacing, with_alpha};

use crate::app::{BRANCH_ICON, GitApp};
use crate::i18n::{Copy, copy};
use crate::session::{Badge, ChangeRow, Check, DiffPane, truncate_leading};

const COMMAND_BAR_HEIGHT: Pixels = px(48.0);
const DIFF_HEADER_HEIGHT: Pixels = px(40.0);
const CHECKBOX_SIZE: Pixels = px(20.0);
const BADGE_SIZE: Pixels = px(16.0);
const DIFF_ROW_HEIGHT: Pixels = px(20.0);
const LINE_NUMBER_WIDTH: Pixels = px(48.0);
const MARKER_WIDTH: Pixels = px(20.0);
/// Characters of a folder path kept in a row's caption; the rest is cut at
/// the leading end.
const FOLDER_CHARS: usize = 28;

/// The fill a status role gets behind its own text, and behind diff lines.
const STATUS_TINT: f32 = 0.12;

fn text<E: Styled>(element: E, style: fluent::TypeStyle, color: Rgba) -> E {
    fluent::text(element, style).text_color(color)
}

/// One row of the flattened diff.
enum DiffRow<'a> {
    Header(&'a str),
    Line(&'a DiffLine),
}

fn flatten(hunks: &[Hunk]) -> Vec<DiffRow<'_>> {
    let mut rows = Vec::new();
    for hunk in hunks {
        rows.push(DiffRow::Header(&hunk.header));
        rows.extend(hunk.lines.iter().map(DiffRow::Line));
    }
    rows
}

impl Render for GitApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        let c = copy(self.locale);
        let open = self.session.repository().is_some();

        let content = if open {
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(self.changes_pane(p, c, cx))
                .child(self.diff_pane(p, c, cx))
                .into_any_element()
        } else {
            self.open_state(p, c, cx).into_any_element()
        };

        v_flex()
            .size_full()
            .bg(p.window_background)
            .text_color(p.text_primary)
            .track_focus(&self.focus_handle)
            // Mutter gives an `xdg-toplevel` client no decorations, so the
            // window draws its own titlebar or it cannot be moved or closed.
            .child(better_ui::window_chrome::title_bar(
                Icon::empty().path(BRANCH_ICON).small(),
                c.app_name,
                cx.theme().foreground,
            ))
            .when(open, |column| column.child(self.command_bar(p, c, cx)))
            .children(self.info_bar(p, c, cx))
            .child(content)
    }
}

impl GitApp {
    fn command_bar(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let name = self.session.repository_name().unwrap_or_default();
        let branch = self
            .session
            .branch()
            .map(str::to_string)
            .unwrap_or_else(|| c.detached.to_string());

        let repository = div()
            .id("repository")
            .h(fsize::CONTROL_HEIGHT)
            .px(spacing::MD)
            .flex()
            .items_center()
            .gap(spacing::SM)
            .rounded(radius::SM)
            .border_1()
            .border_color(p.stroke)
            .bg(p.card)
            .cursor_pointer()
            .hover(|style| style.bg(p.subtle_fill))
            .child(text(div(), fluent::BODY_STRONG, p.text_primary).child(name))
            .child(
                Icon::new(IconName::ChevronDown)
                    .small()
                    .text_color(p.text_secondary),
            )
            .on_click(cx.listener(|this, _, window, cx| this.close_repository(window, cx)));

        let branch = h_flex()
            .gap(spacing::XS)
            .items_center()
            .child(
                Icon::empty()
                    .path(BRANCH_ICON)
                    .small()
                    .text_color(p.text_secondary),
            )
            .child(text(div(), fluent::BODY, p.text_primary).child(branch));

        h_flex()
            .h(COMMAND_BAR_HEIGHT)
            .flex_none()
            .w_full()
            .px(spacing::SM)
            .gap(spacing::LG)
            .items_center()
            .bg(p.window_background)
            .child(repository)
            .child(branch)
            .child(div().flex_1())
            .child(self.command_button(p, c.open, cx))
    }

    /// A command bar button: icon and label, no fill until hovered.
    fn command_button(
        &self,
        p: &'static Palette,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("open")
            .h(fsize::CONTROL_HEIGHT)
            .px(spacing::MD)
            .flex()
            .items_center()
            .gap(spacing::SM)
            .rounded(radius::SM)
            .cursor_pointer()
            .hover(|style| style.bg(p.subtle_fill))
            .child(
                Icon::new(IconName::FolderOpen)
                    .small()
                    .text_color(p.text_primary),
            )
            .child(text(div(), fluent::BODY, p.text_primary).child(label))
            .on_click(cx.listener(|this, _, window, cx| this.choose_folder(window, cx)))
    }

    fn info_bar(
        &self,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let problem = self.session.problem()?;
        let (title, message) = c.problem(problem);
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .min_h(px(48.0))
                .px(spacing::LG)
                .py(spacing::MD)
                .gap(spacing::MD)
                .items_start()
                .bg(with_alpha(p.critical, STATUS_TINT))
                .border_y_1()
                .border_color(p.stroke)
                .child(
                    div()
                        .pt(px(2.0))
                        .child(Icon::new(IconName::CircleX).small().text_color(p.critical)),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .flex_wrap()
                        .gap(spacing::MD)
                        .child(text(div(), fluent::BODY_STRONG, p.text_primary).child(title))
                        .child(
                            text(div(), fluent::BODY, p.text_primary)
                                .min_w_0()
                                .whitespace_normal()
                                .child(message),
                        ),
                )
                .child(
                    div()
                        .id("dismiss")
                        .size(px(28.0))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(radius::SM)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.subtle_fill))
                        .child(
                            Icon::new(IconName::Close)
                                .small()
                                .text_color(p.text_primary),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_error(cx))),
                ),
        )
    }

    fn open_state(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let recent: Vec<_> = self
            .session
            .recent()
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let target = path.clone();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let parent = path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default();
                h_flex()
                    .id(("recent", index))
                    .w_full()
                    .h(fsize::ROW_HEIGHT)
                    .px(spacing::MD)
                    .gap(spacing::SM)
                    .items_center()
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .hover(|style| style.bg(p.subtle_fill))
                    .child(
                        Icon::new(IconName::Folder)
                            .small()
                            .text_color(p.text_secondary),
                    )
                    .child(text(div(), fluent::BODY, p.text_primary).child(name))
                    .child(
                        text(div(), fluent::CAPTION, p.text_secondary)
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(parent),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_path(target.clone(), window, cx)
                    }))
            })
            .collect();

        v_flex().flex_1().min_h_0().w_full().items_center().child(
            v_flex()
                .mt(px(96.0))
                .w(px(440.0))
                .max_w_full()
                .px(spacing::XL)
                .gap(spacing::MD)
                .items_center()
                .child(text(div(), fluent::SUBTITLE, p.text_primary).child(c.open_title))
                .child(text(div(), fluent::BODY, p.text_secondary).child(c.open_detail))
                .child(div().mt(spacing::SM).child(self.accent_button(
                    "open-centered",
                    c.open,
                    true,
                    p,
                    cx,
                    |this, window, cx| this.choose_folder(window, cx),
                )))
                .when(!recent.is_empty(), |column| {
                    column.child(
                        v_flex()
                            .mt(spacing::XL)
                            .w_full()
                            .gap(spacing::XS)
                            .child(
                                text(div(), fluent::BODY_STRONG, p.text_primary)
                                    .px(spacing::MD)
                                    .child(c.recent),
                            )
                            .children(recent),
                    )
                }),
        )
    }

    /// The window's one accent button. Disabled, it has the control fill and
    /// secondary text.
    fn accent_button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        enabled: bool,
        p: &'static Palette,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let button = div()
            .id(id)
            .h(fsize::CONTROL_HEIGHT)
            .px(spacing::MD)
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::SM);
        let label = label.into();
        if enabled {
            text(button, fluent::BODY, p.text_on_accent)
                .bg(p.accent)
                .cursor_pointer()
                .hover(|style| style.opacity(0.9))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
        } else {
            text(button, fluent::BODY, p.text_secondary)
                .bg(p.card)
                .border_1()
                .border_color(p.stroke)
                .child(label)
        }
    }

    fn changes_pane(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let rows = self.session.rows();
        let count = rows.len();
        let all = self.session.all_check();
        let all_enabled = rows.iter().any(|row| row.stageable);

        let header = h_flex()
            .h(fsize::ROW_HEIGHT)
            .flex_none()
            .px(spacing::MD)
            .gap(spacing::SM)
            .items_center()
            .child(
                checkbox("all", all, all_enabled, p)
                    .when(all_enabled, |checkbox| {
                        checkbox.on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_all(window, cx)
                        }))
                    })
                    .tooltip(move |window, cx| {
                        let label = if all == Check::Checked {
                            c.unstage_all
                        } else {
                            c.stage_all
                        };
                        gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                    }),
            )
            .child(text(div(), fluent::BODY_STRONG, p.text_primary).child(c.changes))
            .child(text(div(), fluent::BODY, p.text_secondary).child(count.to_string()));

        let list = if rows.is_empty() {
            v_flex()
                .id("changes")
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .child(text(div(), fluent::BODY, p.text_secondary).child(c.no_changes))
        } else {
            v_flex()
                .id("changes")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(spacing::XS)
                .children(
                    rows.into_iter()
                        .enumerate()
                        .map(|(index, row)| self.change_row(index, row, p, c, cx)),
                )
        };

        v_flex()
            .w(fsize::LIST_PANE_WIDTH)
            .flex_none()
            .h_full()
            .bg(p.card)
            .border_r_1()
            .border_color(p.stroke)
            .child(header)
            .child(list)
            .child(self.commit_form(p, c, cx))
    }

    fn change_row(
        &self,
        index: usize,
        row: ChangeRow,
        p: &'static Palette,
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (letter_color, caption) = match row.badge {
            Badge::Modified => (p.caution, truncate_leading(&row.folder, FOLDER_CHARS)),
            Badge::Added => (p.success, truncate_leading(&row.folder, FOLDER_CHARS)),
            Badge::Deleted => (p.critical, truncate_leading(&row.folder, FOLDER_CHARS)),
            Badge::Renamed => (p.accent, truncate_leading(&row.folder, FOLDER_CHARS)),
            Badge::Conflicted => (p.critical, c.conflicted.to_string()),
        };
        let toggle_path = row.path.clone();
        let select_path = row.path.clone();

        let mut checkbox = checkbox(("check", index), row.check, row.stageable, p);
        if row.stageable {
            checkbox = checkbox.on_click(cx.listener(move |this, _, window, cx| {
                // The checkbox stages and leaves the selection alone.
                cx.stop_propagation();
                this.toggle_stage(toggle_path.clone(), window, cx)
            }));
        }

        h_flex()
            .id(("row", index))
            .relative()
            .h(fsize::ROW_HEIGHT)
            .flex_none()
            .px(spacing::SM)
            .gap(spacing::SM)
            .items_center()
            .rounded(radius::SM)
            .cursor_pointer()
            .when(row.selected, |row| row.bg(p.subtle_fill))
            .when(!row.selected, |row| {
                row.hover(|style| style.bg(p.subtle_fill))
            })
            .when(row.selected, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top(px(10.0))
                        .w(fsize::SELECTION_BAR_WIDTH)
                        .h(fsize::SELECTION_BAR_HEIGHT)
                        .rounded(px(1.5))
                        .bg(p.accent),
                )
            })
            .child(checkbox)
            .child(
                div()
                    .size(BADGE_SIZE)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::SM)
                    .bg(with_alpha(letter_color, 0.18))
                    .text_size(px(11.0))
                    .line_height(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(letter_color)
                    .child(row.badge.letter()),
            )
            .child(
                text(div(), fluent::BODY, p.text_primary)
                    .flex_none()
                    .max_w(px(180.0))
                    .truncate()
                    .child(row.name),
            )
            .child(
                text(div(), fluent::CAPTION, p.text_secondary)
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(caption),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.select(select_path.clone(), window, cx)
                }),
            )
    }

    fn commit_form(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let label = c.commit_label(self.session.branch());
        let enabled = self.session.can_commit();
        v_flex()
            .flex_none()
            .p(spacing::LG)
            .gap(spacing::MD)
            .border_t_1()
            .border_color(p.stroke)
            .child(Input::new(&self.summary_input))
            .child(Input::new(&self.description_input).h(px(76.0)))
            .child(
                self.accent_button("commit", label, enabled, p, cx, |this, window, cx| {
                    this.commit(window, cx)
                })
                .w_full(),
            )
    }

    fn diff_pane(&self, p: &'static Palette, c: &'static Copy, cx: &mut Context<Self>) -> Div {
        let mono = cx.theme().mono_font_family.clone();
        let header_path = self
            .session
            .selection()
            .map(|selection| selection.path.to_string_lossy().into_owned());

        let centered = |message: &'static str| {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(text(div(), fluent::BODY, p.text_secondary).child(message))
                .into_any_element()
        };

        let body = match self.session.diff_pane() {
            DiffPane::NothingSelected => centered(c.select_file),
            DiffPane::Conflicted => centered(c.conflicted),
            DiffPane::Binary => centered(c.binary_file),
            DiffPane::NoChanges => centered(c.no_changes),
            DiffPane::Text(hunks) => {
                let rows = flatten(hunks);
                let count = rows.len();
                // The list is as wide as its widest row, so long lines scroll
                // sideways with the card instead of wrapping.
                let widest = rows
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, row)| match row {
                        DiffRow::Header(header) => header.chars().count(),
                        DiffRow::Line(line) => line.text.chars().count(),
                    })
                    .map(|(index, _)| index);
                let list = uniform_list(
                    "diff",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        let p = fluent::palette(cx);
                        let DiffPane::Text(hunks) = this.session.diff_pane() else {
                            return Vec::new();
                        };
                        let rows = flatten(hunks);
                        range
                            .filter_map(|index| rows.get(index).map(|row| diff_row(row, p)))
                            .collect::<Vec<_>>()
                    }),
                )
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(widest)
                .track_scroll(&self.diff_scroll)
                .size_full();

                div()
                    .flex_1()
                    .min_h_0()
                    .px(spacing::LG)
                    .pb(spacing::LG)
                    .child(
                        div()
                            .size_full()
                            .overflow_hidden()
                            .rounded(radius::SM)
                            .border_1()
                            .border_color(p.stroke)
                            .bg(p.card)
                            .font_family(mono)
                            .text_size(px(fluent::CODE.size))
                            .line_height(px(fluent::CODE.line_height))
                            .child(list),
                    )
                    .into_any_element()
            }
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(p.window_background)
            .child(
                h_flex()
                    .h(DIFF_HEADER_HEIGHT)
                    .flex_none()
                    .px(spacing::LG)
                    .items_center()
                    .when_some(header_path, |header, path| {
                        header.child(
                            text(div(), fluent::BODY, p.text_primary)
                                .min_w_0()
                                .truncate()
                                .child(path),
                        )
                    }),
            )
            .child(body)
    }
}

/// A three-state checkbox: WinUI's 20px square with a 4px radius, filled
/// with the accent when checked or indeterminate.
fn checkbox(
    id: impl Into<ElementId>,
    check: Check,
    enabled: bool,
    p: &'static Palette,
) -> Stateful<Div> {
    let filled = check != Check::Unchecked;
    let mut element = div()
        .id(id)
        .size(CHECKBOX_SIZE)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::SM);
    element = if filled {
        element.bg(p.accent)
    } else {
        element.border_1().border_color(p.text_secondary)
    };
    element = match check {
        Check::Checked => element.child(
            Icon::new(IconName::Check)
                .xsmall()
                .text_color(p.text_on_accent),
        ),
        Check::Indeterminate => element.child(
            Icon::new(IconName::Minus)
                .xsmall()
                .text_color(p.text_on_accent),
        ),
        Check::Unchecked => element,
    };
    if enabled {
        element.cursor_pointer()
    } else {
        element.opacity(0.4)
    }
}

fn diff_row(row: &DiffRow<'_>, p: &'static Palette) -> AnyElement {
    let number = |value: Option<u32>| {
        div()
            .w(LINE_NUMBER_WIDTH)
            .flex_none()
            .pr(spacing::SM)
            .text_right()
            .text_color(p.text_secondary)
            .child(value.map(|value| value.to_string()).unwrap_or_default())
    };
    match row {
        DiffRow::Header(header) => div()
            .h(DIFF_ROW_HEIGHT)
            .w_full()
            .px(spacing::SM)
            .flex()
            .items_center()
            .whitespace_nowrap()
            .bg(p.card)
            .text_color(p.text_secondary)
            .child(header.to_string())
            .into_any_element(),
        DiffRow::Line(line) => {
            let (fill, marker) = match line.kind {
                DiffLineKind::Added => (Some(with_alpha(p.success, STATUS_TINT)), "+"),
                DiffLineKind::Removed => (Some(with_alpha(p.critical, STATUS_TINT)), "-"),
                DiffLineKind::Context => (None, ""),
            };
            h_flex()
                .h(DIFF_ROW_HEIGHT)
                .w_full()
                .items_center()
                .whitespace_nowrap()
                .when_some(fill, |row, fill| row.bg(fill))
                .child(number(line.old_line))
                .child(number(line.new_line))
                .child(
                    div()
                        .w(MARKER_WIDTH)
                        .flex_none()
                        .text_color(p.text_secondary)
                        .child(marker),
                )
                .child(
                    div()
                        .flex_none()
                        .pr(spacing::LG)
                        .text_color(p.text_primary)
                        .child(line.text.clone()),
                )
                .into_any_element()
        }
    }
}

//! The content area, in both view modes, both virtualized.
//!
//! Both modes are one `uniform_list`: it asks for a range of items and draws
//! only those, so the element count per frame is a screenful whether the
//! directory holds twenty entries or a hundred thousand. The list mode's item
//! is one row; the grid mode's item is one row of tiles, which is why the grid
//! is virtualized in the same sense the list is rather than being a wrapping
//! flex that builds every tile.
//!
//! Rows are formatted inside the range callback, from the entries
//! `files_core::DirectoryModel` already holds. Nothing is precomputed and
//! nothing is cached, so a batch arriving mid-scroll costs a merge in the
//! model and nothing at all here.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{ActiveTheme, *};

use crate::app::FilesApp;
use crate::content::{
    ListColumn, SelectionInput, empty_state, header_click, rendered_row, status_line,
    unlistable_reason,
};
use crate::i18n::copy;
use crate::keys::Focus;
use crate::prefs::ViewMode;

/// What a drag from the content area carries.
///
/// A location, not a path: dropping it on Favorites writes a bookmark for the
/// location, and a location is what a bookmark is made of.
#[derive(Clone, Debug)]
pub struct DraggedEntry {
    pub location: files_core::Location,
    pub label: String,
}

/// The little card that follows the pointer during a drag.
pub struct DragPreview {
    label: String,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(self.label.clone())
    }
}

impl FilesApp {
    /// The whole content area: the header, the virtualized body, and the
    /// status line.
    pub(crate) fn content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let mode = self.session.preferences.view_mode;
        let columns = self.columns();
        let model = self.session.pane().model();
        let total = model.visible_len();
        let status = status_line(model, c);
        let empty = empty_state(model, c).map(str::to_string);
        let unlistable = unlistable_reason(self.session.location(), c).map(str::to_string);

        let rows = match mode {
            ViewMode::List => total,
            ViewMode::Grid => total.div_ceil(columns.max(1)),
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(if mode == ViewMode::List {
                self.list_header(cx)
            } else {
                div().into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(unlistable.clone(), |element, message| {
                        element.child(better_ui::state_message(
                            message,
                            String::new(),
                            cx.theme().foreground,
                            cx.theme().muted_foreground,
                        ))
                    })
                    .when(unlistable.is_none(), |element| {
                        element.when_some(empty, |element, message| {
                            element.child(better_ui::state_message(
                                message,
                                String::new(),
                                cx.theme().foreground,
                                cx.theme().muted_foreground,
                            ))
                        })
                    })
                    .when(unlistable.is_none() && total > 0, |element| {
                        element.child(self.virtual_body(rows, columns, mode, cx))
                    }),
            )
            .child(
                div()
                    .w_full()
                    .px_4()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .into_any_element()
    }

    /// The clickable column headers of the detailed list.
    fn list_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let order = self.session.preferences.order();
        h_flex()
            .w_full()
            .px_4()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(ListColumn::ALL.map(|column| {
                let sorted = order.key == column.sort_key();
                div()
                    .id(SharedString::from(format!("column-{}", column.key())))
                    .w(px(column.width()))
                    .flex_shrink_0()
                    .truncate()
                    .text_xs()
                    .font_semibold()
                    .when(sorted, |element| element.text_color(cx.theme().primary))
                    .child(format!(
                        "{}{}",
                        column.header(c),
                        if sorted {
                            match order.direction {
                                files_core::SortDirection::Ascending => " ▲",
                                files_core::SortDirection::Descending => " ▼",
                            }
                        } else {
                            ""
                        }
                    ))
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
        let tile = self.session.preferences.scale.tile_size();
        uniform_list(
            "files-content",
            rows,
            move |range: std::ops::Range<usize>, _window, cx: &mut App| {
                let c = copy(locale);
                let app = entity.read(cx);
                let model = app.session.pane().model();
                let cursor = app.session.content.cursor();
                let mut out = Vec::with_capacity(range.len());
                for row in range {
                    match mode {
                        ViewMode::List => {
                            let Some(entry) = model.visible(row) else {
                                out.push(div().into_any_element());
                                continue;
                            };
                            let rendered = rendered_row(
                                entry,
                                model.selection().contains(&entry.id()),
                                cursor == Some(row),
                                c,
                            );
                            let location = entry
                                .as_local_path()
                                .cloned()
                                .map(files_core::Location::Local);
                            out.push(list_row(&entity, row, rendered, location, cx));
                        }
                        ViewMode::Grid => {
                            let start = row * columns;
                            let mut tiles = Vec::new();
                            for offset in 0..columns {
                                let index = start + offset;
                                let Some(entry) = model.visible(index) else {
                                    break;
                                };
                                let rendered = rendered_row(
                                    entry,
                                    model.selection().contains(&entry.id()),
                                    cursor == Some(index),
                                    c,
                                );
                                let location = entry
                                    .as_local_path()
                                    .cloned()
                                    .map(files_core::Location::Local);
                                tiles.push(grid_tile(&entity, index, rendered, location, tile, cx));
                            }
                            out.push(
                                h_flex()
                                    .w_full()
                                    .gap(px(crate::content::GRID_GAP))
                                    .px_4()
                                    .py_1()
                                    .children(tiles)
                                    .into_any_element(),
                            );
                        }
                    }
                }
                out
            },
        )
        .h_full()
        .into_any_element()
    }
}

/// One row of the detailed list.
fn list_row(
    entity: &Entity<FilesApp>,
    index: usize,
    row: crate::content::RenderedRow,
    location: Option<files_core::Location>,
    cx: &App,
) -> AnyElement {
    let selected = row.selected;
    let focused = row.focused;
    let theme = cx.theme();
    let mut element = h_flex()
        .id(SharedString::from(format!("row-{index}")))
        .w_full()
        .px_4()
        .gap_2()
        .items_center()
        .rounded(theme.radius)
        .when(selected, |element| element.bg(theme.accent))
        .when(focused, |element| {
            element.border_1().border_color(theme.primary)
        })
        .when(row.hidden, |element| element.opacity(0.7))
        .child(
            h_flex()
                .w(px(ListColumn::Name.width()))
                .flex_shrink_0()
                .gap_2()
                .items_center()
                .child(div().child(row.glyph))
                .child(div().truncate().text_sm().child(row.name.clone())),
        )
        .child(cell(row.size.clone(), ListColumn::Size))
        .child(cell(row.modified.clone(), ListColumn::Modified))
        .child(cell(row.type_label.clone(), ListColumn::Type))
        .child(cell(row.extension.clone(), ListColumn::Extension));

    element = attach_row_handlers(element, entity, index, location, row.name);
    element.into_any_element()
}

/// One tile of the icon grid.
fn grid_tile(
    entity: &Entity<FilesApp>,
    index: usize,
    row: crate::content::RenderedRow,
    location: Option<files_core::Location>,
    tile: f32,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let mut element = v_flex()
        .id(SharedString::from(format!("tile-{index}")))
        .w(px(tile))
        .h(px(tile))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap_1()
        .p_2()
        .rounded(theme.radius)
        .when(row.selected, |element| element.bg(theme.accent))
        .when(row.focused, |element| {
            element.border_1().border_color(theme.primary)
        })
        .when(row.hidden, |element| element.opacity(0.7))
        .child(div().text_2xl().child(row.glyph))
        .child(
            div()
                .w_full()
                .truncate()
                .text_xs()
                .text_center()
                .child(row.name.clone()),
        );

    element = attach_tile_handlers(element, entity, index, location, row.name);
    element.into_any_element()
}

fn cell(text: String, column: ListColumn) -> Div {
    div()
        .w(px(column.width()))
        .flex_shrink_0()
        .truncate()
        .text_xs()
        .child(text)
}

/// Click, double-click, context pin, and drag, shared by both view modes.
fn attach_row_handlers(
    element: Stateful<Div>,
    entity: &Entity<FilesApp>,
    index: usize,
    location: Option<files_core::Location>,
    label: String,
) -> Stateful<Div> {
    let clicked = entity.clone();
    let opened = entity.clone();
    let pinned = entity.clone();
    let element = element
        .on_click(move |event, _window, cx| {
            let input = if event.modifiers().control {
                SelectionInput::ToggleClick(index)
            } else if event.modifiers().shift {
                SelectionInput::RangeClick(index)
            } else {
                SelectionInput::Click(index)
            };
            clicked.update(cx, |app, cx| {
                app.session.focus = Focus::Content;
                let columns = app.columns();
                app.session.apply_selection(input, columns);
                cx.notify();
            });
        })
        .on_double_click(move |_event, _window, cx| {
            opened.update(cx, |app, cx| {
                app.session.open_index(index);
                cx.notify();
            });
        })
        // Right-click pins a directory. Issue #6 asks for a drag; a pointer
        // drag is not reachable from a keyboard or a screen reader, so the
        // menu action exists beside it rather than instead of it.
        .on_mouse_down(MouseButton::Right, {
            let location = location.clone();
            move |_event, _window, cx| {
                let Some(location) = location.clone() else {
                    return;
                };
                pinned.update(cx, |app, cx| {
                    app.session.pin(&location);
                    cx.notify();
                });
            }
        });

    match location {
        Some(location) => element.on_drag(
            DraggedEntry {
                location,
                label: label.clone(),
            },
            move |dragged, _offset, _window, cx| {
                let label = dragged.label.clone();
                cx.new(|_| DragPreview { label })
            },
        ),
        None => element,
    }
}

fn attach_tile_handlers(
    element: Stateful<Div>,
    entity: &Entity<FilesApp>,
    index: usize,
    location: Option<files_core::Location>,
    label: String,
) -> Stateful<Div> {
    attach_row_handlers(element, entity, index, location, label)
}

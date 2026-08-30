//! Toolbar, tab strip, and sidebar.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    input::Input,
    scroll::ScrollableElement,
    *,
};

use crate::app::FilesApp;
use crate::content::{SORT_KEYS, sort_key_label};
use crate::i18n::{copy, scale_label, view_mode_label};
use crate::keys::Focus;
use crate::layout::SIDEBAR_WIDTH;
use crate::prefs::ItemScale;
use crate::sidebar::{
    FilesystemProbe, NoDeviceStates, SidebarInputs, SidebarRow, SidebarSection, build_rows,
};
use crate::toolbar::toolbar_state;

impl FilesApp {
    /// Back, Forward, Up, the path field, and the view controls.
    pub(crate) fn toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let state = toolbar_state(self.session.pane().history());
        let hidden_shown = self.session.preferences.show_hidden;
        let mode = self.session.preferences.view_mode;
        let order = self.session.preferences.order();
        let active_jobs = crate::opcenter::active_count(&self.session.jobs);

        h_flex()
            .w_full()
            .min_h(px(56.0))
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                Button::new("go-back")
                    .icon(IconName::ArrowLeft)
                    .tooltip(c.go_back)
                    .disabled(!state.can_go_back)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.session.go_back();
                        this.sync_path_field(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("go-forward")
                    .icon(IconName::ArrowRight)
                    .tooltip(c.go_forward)
                    .disabled(!state.can_go_forward)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.session.go_forward();
                        this.sync_path_field(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("go-up")
                    .icon(IconName::ArrowUp)
                    .tooltip(c.go_to_parent)
                    .disabled(!state.can_go_to_parent)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.session.go_to_parent();
                        this.sync_path_field(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("reload")
                    .icon(IconName::Replace)
                    .tooltip(c.reload)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.reload();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(180.0))
                    .child(Input::new(&self.path_input).cleanable(true)),
            )
            .child(
                Button::new("path-go")
                    .label(c.open)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_path(window, cx))),
            )
            .child(
                Button::new("toggle-view")
                    .label(view_mode_label(mode, c))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.toggle_view_mode();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("toggle-hidden")
                    .when(hidden_shown, |button| button.primary())
                    .label(if hidden_shown {
                        c.hide_hidden
                    } else {
                        c.show_hidden
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.toggle_hidden();
                        cx.notify();
                    })),
            )
            .child(self.sort_controls(cx))
            .child(self.scale_controls(cx))
            .child(
                Button::new("toggle-operations")
                    .when(self.session.operations_open, |button| button.primary())
                    .label(if active_jobs > 0 {
                        format!("{} · {active_jobs}", c.operation_center)
                    } else {
                        c.operation_center.to_string()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.operations_open = !this.session.operations_open;
                        cx.notify();
                    })),
            )
            .child(self.language_control(cx))
            .child(
                // The sort order is drawn even when the controls wrap, so the
                // current order is never off-screen.
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} {}", c.sort_by, sort_key_label(order.key, c))),
            )
            .into_any_element()
    }

    fn sort_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let order = self.session.preferences.order();
        h_flex()
            .gap_1()
            .items_center()
            .children(SORT_KEYS.map(|key| {
                Button::new(SharedString::from(format!("sort-{}", key_id(key))))
                    .when(order.key == key, |button| button.primary())
                    .label(sort_key_label(key, c))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session.set_sort_key(key);
                        cx.notify();
                    }))
            }))
            .child(
                Button::new("sort-direction")
                    .label(if order.direction == files_core::SortDirection::Ascending {
                        c.ascending
                    } else {
                        c.descending
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.toggle_sort_direction();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("folders-first")
                    .when(order.folders_first, |button| button.primary())
                    .label(c.folders_first)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.toggle_folders_first();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn scale_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let current = self.session.preferences.scale;
        h_flex()
            .gap_1()
            .items_center()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.item_size),
            )
            .children(ItemScale::ALL.map(|scale| {
                Button::new(SharedString::from(format!("scale-{}", scale.key())))
                    .when(current == scale, |button| button.primary())
                    .label(scale_label(scale, c))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session.set_scale(scale);
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    fn language_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let locale = self.locale();
        h_flex()
            .gap_1()
            .child(
                Button::new("locale-en")
                    .when(
                        matches!(locale.resolved(), crate::i18n::Locale::EnUs),
                        |button| button.primary(),
                    )
                    .label(c.english)
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.set_locale(crate::i18n::Locale::EnUs, cx)
                        }),
                    ),
            )
            .child(
                Button::new("locale-zh")
                    .when(
                        matches!(locale.resolved(), crate::i18n::Locale::ZhTw),
                        |button| button.primary(),
                    )
                    .label(c.chinese)
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.set_locale(crate::i18n::Locale::ZhTw, cx)
                        }),
                    ),
            )
            .child(
                Button::new("theme")
                    .label(if self.theme == gpui_component::ThemeMode::Dark {
                        c.light_theme
                    } else {
                        c.dark_theme
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        let next = if this.theme == gpui_component::ThemeMode::Dark {
                            gpui_component::ThemeMode::Light
                        } else {
                            gpui_component::ThemeMode::Dark
                        };
                        this.set_theme(next, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// The tab strip.
    pub(crate) fn tab_strip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let active = self.session.active_tab();
        let tabs: Vec<(files_core::TabId, String)> = self
            .session
            .tabs()
            .tabs()
            .iter()
            .map(|tab| (tab.id(), tab.title()))
            .collect();
        let can_restore = self.session.tabs().can_restore();

        h_flex()
            .w_full()
            .px_3()
            .py_1()
            .gap_1()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(tabs.into_iter().map(|(id, title)| {
                let is_active = id == active;
                h_flex()
                    .id(SharedString::from(format!("tab-{}", id.value())))
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .when(is_active, |row| row.bg(cx.theme().accent))
                    .child(div().text_sm().truncate().max_w(px(180.0)).child(title))
                    .child(
                        Button::new(SharedString::from(format!("tab-close-{}", id.value())))
                            .icon(IconName::Close)
                            .tooltip(c.close_tab)
                            .xsmall()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.session.close_tab(id);
                                cx.notify();
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session.activate_tab(id);
                        cx.notify();
                    }))
                    // Middle-click closes, the same as every other tab strip.
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, _, cx| {
                            this.session.close_tab(id);
                            cx.notify();
                        }),
                    )
            }))
            .child(
                Button::new("tab-new")
                    .icon(IconName::Plus)
                    .tooltip(c.new_tab)
                    .xsmall()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let location = this.session.home_location();
                        this.session.open_tab(location, true);
                        cx.notify();
                    })),
            )
            .when(can_restore, |strip| {
                strip.child(
                    Button::new("tab-restore")
                        .label(c.reopen_closed_tab)
                        .xsmall()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.restore_tab();
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    /// The four-section sidebar.
    pub(crate) fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let rows = {
            let inputs = SidebarInputs {
                directories: &self.session.directories,
                mounts: &self.session.mounts,
                bookmarks: &self.session.bookmarks,
                states: &NoDeviceStates,
                probe: &FilesystemProbe,
            };
            build_rows(&inputs, c)
        };
        let favorites = rows
            .iter()
            .filter(|row| row.section == SidebarSection::Favorites)
            .count();

        let mut column = v_flex()
            .w(px(SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .gap_2()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .overflow_y_scrollbar();

        for section in SidebarSection::ALL {
            column = column.child(
                div()
                    .px_2()
                    .pt_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(section.title(c)),
            );
            let section_rows: Vec<&SidebarRow> =
                rows.iter().filter(|row| row.section == section).collect();
            if section_rows.is_empty() {
                let empty = match section {
                    SidebarSection::Devices => c.no_devices,
                    SidebarSection::Favorites => c.no_favorites,
                    _ => c.unavailable,
                };
                column = column.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .italic()
                        .text_color(cx.theme().muted_foreground)
                        .child(empty),
                );
            }
            for row in section_rows {
                column = column.child(self.sidebar_row(row, cx));
            }
            if section == SidebarSection::Favorites {
                column = column.child(self.favorites_drop_zone(favorites, cx));
            }
        }

        column.into_any_element()
    }

    fn sidebar_row(&self, row: &SidebarRow, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let available = row.availability.is_available();
        let location = row.location.clone();
        let bookmark_index = row.bookmark_index;
        let focused = self.session.focus == Focus::Sidebar
            && bookmark_index.is_some()
            && bookmark_index == self.session.sidebar_cursor;

        let detail = if !available {
            Some(c.unavailable.to_string())
        } else if row.identity_volatile {
            Some(c.identity_volatile.to_string())
        } else if row.device_state == Some(storage_core::DeviceStateKind::Unknown) {
            Some(c.device_state_unknown_without_service.to_string())
        } else {
            None
        };

        v_flex()
            .id(SharedString::from(row.key.clone()))
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .when(focused, |element| element.bg(cx.theme().accent))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .min_w_0()
                    .child(Icon::new(icon_for(row.section)).small())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .when(!available, |label| {
                                label.text_color(cx.theme().muted_foreground)
                            })
                            .child(row.label.clone()),
                    ),
            )
            .when_some(detail, |element, detail| {
                element.child(
                    div()
                        .pl_6()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(detail),
                )
            })
            .when(bookmark_index.is_some(), |element| {
                let index = bookmark_index.expect("checked");
                element.child(
                    h_flex()
                        .pl_6()
                        .gap_1()
                        .child(
                            Button::new(SharedString::from(format!("fav-up-{index}")))
                                .label(c.move_up)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.move_bookmark_up(index);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("fav-down-{index}")))
                                .label(c.move_down)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.move_bookmark_down(index);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("fav-rename-{index}")))
                                .label(c.rename_bookmark)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.session.dialog =
                                        Some(crate::session::PendingDialog::RenameBookmark(index));
                                    this.prepare_dialog(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("fav-remove-{index}")))
                                .label(c.remove_from_sidebar)
                                .xsmall()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.session.remove_bookmark(index);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("fav-newtab-{index}")))
                                .label(c.open_in_new_tab)
                                .xsmall()
                                .on_click({
                                    let location = location.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        this.session.open_tab(location.clone(), true);
                                        cx.notify();
                                    })
                                }),
                        ),
                )
            })
            .on_click({
                let location = location.clone();
                cx.listener(move |this, _, window, cx| {
                    if let Some(index) = bookmark_index {
                        this.session.focus = Focus::Sidebar;
                        this.session.sidebar_cursor = Some(index);
                    } else {
                        this.session.focus = Focus::Content;
                    }
                    if available {
                        this.session.navigate_to(location.clone());
                        this.sync_path_field(window, cx);
                    }
                    cx.notify();
                })
            })
            .into_any_element()
    }

    /// The drop target at the bottom of Favorites.
    ///
    /// Dropping a folder here pins it. The same action is on the content
    /// area's context menu, because a pointer drag is not a keyboard-reachable
    /// gesture and Issue #6 asks for both.
    fn favorites_drop_zone(&self, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        div()
            .id("favorites-drop")
            .w_full()
            .mt_1()
            .px_2()
            .py_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_dashed()
            .border_color(cx.theme().border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            // The zone says what it is for rather than lighting up mid-drag.
            // GPUI offers no drag-end hook here, so a highlight switched on
            // when a drag started would stay on after one was abandoned.
            .child(if count == 0 {
                c.no_favorites
            } else {
                c.drop_to_pin
            })
            .on_drop(
                cx.listener(|this, dragged: &crate::views::DraggedEntry, _window, cx| {
                    this.session.pin(&dragged.location);
                    cx.notify();
                }),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.session.pin_current();
                cx.notify();
            }))
            .into_any_element()
    }
}

fn icon_for(section: SidebarSection) -> IconName {
    match section {
        SidebarSection::Places => IconName::Folder,
        SidebarSection::Devices => IconName::HardDrive,
        SidebarSection::Applications => IconName::LayoutDashboard,
        SidebarSection::Favorites => IconName::Star,
    }
}

fn key_id(key: files_core::SortKey) -> &'static str {
    match key {
        files_core::SortKey::Name => "name",
        files_core::SortKey::Modified => "modified",
        files_core::SortKey::Size => "size",
        files_core::SortKey::Type => "type",
        files_core::SortKey::Extension => "extension",
    }
}

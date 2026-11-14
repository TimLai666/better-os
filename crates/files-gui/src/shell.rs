//! The window's frame, in the Fluent kit: the address row (Back, Forward,
//! Up, Reload, the breadcrumbs, search), the command bar, the tab strip, and
//! the navigation pane.
//!
//! Every control here comes from `better_ui::fluent`. What each one does is
//! a call on the session; nothing is decided here.

use std::rc::Rc;

use better_ui::fluent::{
    self, CAPTION,
    breadcrumb::BreadcrumbBar,
    button::Button,
    command_bar::{Command as BarCommand, CommandBar},
    menu::{MenuEntry, MenuItem},
    navigation::{NavAction, NavEntry, NavItem, NavigationView},
    tabs::{Tab, TabView},
    text_box::TextBox,
    words::Locale as KitLocale,
};
use gpui::*;
use gpui_component::IconName;

use crate::app::FilesApp;
use crate::content::{SORT_KEYS, sort_key_label};
use crate::devices::DeviceRow;
use crate::i18n::{Copy, Locale, copy};
use crate::keys::Focus;
use crate::prefs::{ItemScale, ViewMode};
use crate::sidebar::{
    FilesystemProbe, NoDeviceStates, SidebarInputs, SidebarRow, SidebarSection, build_rows,
    shown_rows,
};
use crate::toolbar::toolbar_state;

/// What one line of the navigation pane stands for.
#[derive(Clone, Debug)]
pub(crate) enum NavTarget {
    /// One of a plugin's items.
    PluginItem(Box<(crate::plugins::Plugin, crate::plugins::Item)>),
    /// A plugin's own page (`plugin:<id>`).
    PluginView(String),
    Row(SidebarRow),
    Device(DeviceRow),
    /// A machine on the tailnet, opened over SFTP.
    Machine(crate::share::Machine),
}

impl NavTarget {
    fn location(&self) -> Option<&files_core::Location> {
        match self {
            NavTarget::Row(row) => Some(&row.location),
            NavTarget::Device(_)
            | NavTarget::Machine(_)
            | NavTarget::PluginItem(_)
            | NavTarget::PluginView(_) => None,
        }
    }
}

/// Wraps a window action as a kit handler.
pub(crate) fn act(
    cx: &Context<FilesApp>,
    f: impl Fn(&mut FilesApp, &mut Window, &mut Context<FilesApp>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = cx.entity().downgrade();
    move |window, cx| {
        let _ = this.update(cx, |app, cx| {
            f(app, window, cx);
            cx.notify();
        });
    }
}

pub(crate) fn kit_locale(locale: Locale) -> KitLocale {
    match locale.resolved() {
        Locale::ZhTw => KitLocale::TraditionalChinese,
        _ => KitLocale::English,
    }
}

impl FilesApp {
    /// Back, Forward, Up, Reload, the breadcrumbs (or the typed location),
    /// and the search box.
    pub(crate) fn address_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let state = toolbar_state(self.session.pane().history());

        let nav_button = |id: &'static str, icon: IconName, tip: &'static str, enabled: bool| {
            Button::icon_button(id, icon, tip).disabled(!enabled)
        };
        let back = nav_button("go-back", IconName::ArrowLeft, c.go_back, state.can_go_back)
            .on_click(act(cx, |app, window, _| {
                app.session.go_back();
                app.after_location_change(window);
            }));
        let forward = nav_button(
            "go-forward",
            IconName::ArrowRight,
            c.go_forward,
            state.can_go_forward,
        )
        .on_click(act(cx, |app, window, _| {
            app.session.go_forward();
            app.after_location_change(window);
        }));
        let up = nav_button(
            "go-up",
            IconName::ArrowUp,
            c.go_to_parent,
            state.can_go_to_parent,
        )
        .on_click(act(cx, |app, window, _| {
            app.session.go_to_parent();
            app.after_location_change(window);
        }));
        let reload = Button::icon_button("reload", IconName::Redo, c.reload)
            .on_click(act(cx, |app, _, _| app.session.reload()));

        let address: AnyElement = if self.editing_path {
            div()
                .flex_1()
                .min_w_0()
                .child(TextBox::new("path-field", &self.path_input))
                .into_any_element()
        } else {
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            let crumbs = crate::pathbar::crumbs_named(
                self.session.location(),
                home.as_deref(),
                c,
                &self.session.directories,
            );
            let width =
                (f32::from(self.viewport.width) - self.nav_width() - 180.0 - self.search_width())
                    .max(120.0);
            let visible = crate::pathbar::visible_crumbs(&crumbs, width - 24.0);
            let mut labels: Vec<SharedString> = crumbs
                .iter()
                .map(|crumb| SharedString::from(crumb.label.clone()))
                .collect();
            // A plugin's page is named by its title, nothing to climb to.
            let plugin_title = self.plugin_page_title();
            if let Some(title) = &plugin_title {
                labels = vec![title.clone().into()];
            }
            let crumbs = if plugin_title.is_some() {
                Vec::new()
            } else {
                crumbs
            };
            let targets: Rc<Vec<files_core::Location>> =
                Rc::new(crumbs.into_iter().map(|crumb| crumb.location).collect());
            let this = cx.entity().downgrade();
            let bar = BreadcrumbBar::new("breadcrumbs", labels)
                .single_line()
                .max_visible(visible)
                .on_click(move |index, window, cx| {
                    let Some(location) = targets.get(index).cloned() else {
                        return;
                    };
                    let _ = this.update(cx, |app, cx| {
                        app.session.navigate_to(location);
                        app.after_location_change(window);
                        cx.notify();
                    });
                });
            // The box around the crumbs is the address box: clicking its empty
            // part turns it into a text field, as File Explorer's does.
            div()
                .id("address-box")
                .flex_1()
                .min_w_0()
                .h(px(32.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .rounded(fluent::radius::SM)
                .bg(p.control_fill)
                .border_1()
                .border_color(p.stroke)
                .overflow_hidden()
                .child(
                    match self.session.location() {
                        files_core::Location::Trash(_) => {
                            gpui_component::Icon::empty().path(crate::icons::TRASH)
                        }
                        other => gpui_component::Icon::new(icon_for_location(other)),
                    }
                    .size(px(16.0))
                    .text_color(p.text_secondary),
                )
                .child(div().flex().min_w_0().overflow_hidden().child(bar))
                .child(div().flex_1().h_full())
                .tooltip(fluent::dialog::tooltip_builder(c.edit_location.into()))
                .on_click(cx.listener(|app, _, window, cx| {
                    app.begin_path_edit(window, cx);
                }))
                .into_any_element()
        };

        let search_width = px(self.search_width());
        let search = div().w(search_width).flex_none().child(
            TextBox::search("search-field", &self.search_input).on_search(act(
                cx,
                |app, window, cx| {
                    app.editing_search = true;
                    app.search_input
                        .update(cx, |state, cx| state.focus(window, cx));
                },
            )),
        );

        div()
            .w_full()
            .h(px(48.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(back)
            .child(forward)
            .child(up)
            .child(reload)
            .child(div().w(px(4.0)))
            .child(address)
            .child(div().w(px(4.0)))
            .child(search)
            .into_any_element()
    }

    /// The command bar, as File Explorer has it: New, the clipboard, Rename,
    /// Delete, then Sort and View, and the rest behind `…`.
    pub(crate) fn command_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let mut facts = self.session.selection_facts();
        // On a plugin's page nothing is the folder's: nothing to make, cut,
        // copy, paste, rename or trash, and no sort or view to change.
        let on_plugin_page = self.plugin_view.is_some();
        if on_plugin_page {
            facts.writable = false;
            facts.all_local = false;
            facts.can_paste = false;
            facts.count = 0;
            facts.in_trash = false;
        }
        let in_trash = facts.in_trash;
        let has_selection = facts.count > 0;
        let order = self.session.preferences.order();
        let mode = self.session.preferences.view_mode;
        let scale = self.session.preferences.scale;
        let hidden = self.session.preferences.show_hidden;
        let active_jobs = crate::opcenter::active_count(&self.session.jobs);

        let new_menu = vec![
            MenuItem::new(c.new_folder)
                .icon(IconName::FolderClosed)
                .shortcut("Ctrl+Shift+N")
                .disabled(!facts.writable)
                .on_select(act(cx, |app, window, cx| {
                    app.session.dialog = Some(crate::session::PendingDialog::NewFolder);
                    app.prepare_dialog(window, cx);
                }))
                .entry(),
            MenuItem::new(c.new_file)
                .icon(IconName::File)
                .disabled(!facts.writable)
                .on_select(act(cx, |app, window, cx| {
                    app.session.dialog = Some(crate::session::PendingDialog::NewFile);
                    app.prepare_dialog(window, cx);
                }))
                .entry(),
        ];

        let mut sort_menu: Vec<MenuEntry> = SORT_KEYS
            .iter()
            .map(|&key| {
                MenuItem::new(sort_key_label(key, c))
                    .checked(order.key == key)
                    .on_select(act(cx, move |app, _, _| app.session.set_sort_key(key)))
                    .entry()
            })
            .collect();
        sort_menu.push(MenuEntry::Separator);
        let ascending = order.direction == files_core::SortDirection::Ascending;
        sort_menu.push(
            MenuItem::new(c.ascending)
                .checked(ascending)
                .on_select(act(cx, move |app, _, _| {
                    if !ascending {
                        app.session.toggle_sort_direction();
                    }
                }))
                .entry(),
        );
        sort_menu.push(
            MenuItem::new(c.descending)
                .checked(!ascending)
                .on_select(act(cx, move |app, _, _| {
                    if ascending {
                        app.session.toggle_sort_direction();
                    }
                }))
                .entry(),
        );
        sort_menu.push(MenuEntry::Separator);
        sort_menu.push(
            MenuItem::new(c.folders_first)
                .checked(order.folders_first)
                .on_select(act(cx, |app, _, _| app.session.toggle_folders_first()))
                .entry(),
        );

        let mut view_menu = vec![
            MenuItem::new(c.view_grid)
                .checked(mode == ViewMode::Grid)
                .shortcut("Ctrl+1")
                .on_select(act(cx, |app, _, _| {
                    app.session.set_view_mode(ViewMode::Grid)
                }))
                .entry(),
            MenuItem::new(c.view_list)
                .checked(mode == ViewMode::List)
                .shortcut("Ctrl+2")
                .on_select(act(cx, |app, _, _| {
                    app.session.set_view_mode(ViewMode::List)
                }))
                .entry(),
            MenuItem::new(c.view_columns)
                .checked(mode == ViewMode::Columns)
                .shortcut("Ctrl+3")
                .on_select(act(cx, |app, _, _| {
                    app.session.set_view_mode(ViewMode::Columns)
                }))
                .entry(),
            MenuEntry::Separator,
        ];
        for item_scale in ItemScale::ALL {
            view_menu.push(
                MenuItem::new(crate::i18n::scale_label(item_scale, c))
                    .checked(scale == item_scale)
                    .shortcut(match item_scale {
                        ItemScale::Small => "Ctrl+−",
                        ItemScale::Medium => "Ctrl+0",
                        ItemScale::Large => "Ctrl++",
                    })
                    .on_select(act(cx, move |app, _, _| app.session.set_scale(item_scale)))
                    .entry(),
            );
        }
        view_menu.push(MenuEntry::Separator);
        view_menu.push(
            MenuItem::new(c.show_hidden)
                .checked(hidden)
                .shortcut("Ctrl+H")
                .on_select(act(cx, |app, _, _| app.session.toggle_hidden()))
                .entry(),
        );
        view_menu.push(
            MenuItem::new(c.preview_pane)
                .checked(self.session.preview.open)
                .shortcut("Space")
                .on_select(act(cx, |app, _, _| app.session.toggle_preview()))
                .entry(),
        );
        view_menu.push(MenuEntry::Separator);
        view_menu.push(
            MenuItem::new(c.use_view_everywhere)
                .on_select(act(cx, |app, _, _| app.session.use_view_everywhere()))
                .entry(),
        );

        let mut bar = CommandBar::new("files-commands").locale(kit_locale(self.locale()));
        if in_trash {
            bar = bar
                .command(
                    BarCommand::new(c.restore_from_trash, IconName::Undo2)
                        .disabled(!has_selection)
                        .on_click(act(cx, |app, _, _| {
                            app.session.restore_selection_from_trash()
                        })),
                )
                .command(
                    BarCommand::new(c.delete_permanently, IconName::Delete)
                        .disabled(!has_selection)
                        .shortcut("Shift+Delete")
                        .on_click(act(cx, |app, _, _| app.session.request_permanent_delete())),
                )
                .command(
                    BarCommand::new(c.empty_trash, IconName::Delete)
                        .disabled(!facts.has_items)
                        .on_click(act(cx, |app, window, cx| {
                            let columns = app.columns();
                            app.session.empty_trash(columns);
                            app.prepare_dialog(window, cx);
                        })),
                )
                .separator();
        } else {
            bar = bar
                .menu(
                    BarCommand::new(c.new_menu, IconName::Plus).disabled(!facts.writable),
                    new_menu,
                )
                .separator()
                .command(
                    BarCommand::new(c.cut_items, IconName::Frame)
                        .shortcut("Ctrl+X")
                        .disabled(!facts.all_local)
                        .on_click(act(cx, |app, _, _| app.session.cut_selection())),
                )
                .command(
                    BarCommand::new(c.copy_items, IconName::Copy)
                        .shortcut("Ctrl+C")
                        .disabled(!facts.all_local)
                        .on_click(act(cx, |app, _, _| app.session.copy_selection())),
                )
                .command(
                    BarCommand::new(c.paste_items, IconName::Inbox)
                        .shortcut("Ctrl+V")
                        .disabled(!(facts.can_paste && facts.writable))
                        .on_click(act(cx, |app, _, _| app.session.paste())),
                )
                .command(
                    BarCommand::new(c.rename, IconName::ALargeSmall)
                        .shortcut("F2")
                        .disabled(!(facts.count == 1 && facts.all_local && facts.writable))
                        .on_click(act(cx, |app, window, cx| {
                            app.session.focus = Focus::Content;
                            app.session.dispatch(crate::keys::Command::Rename, 1, 1);
                            app.prepare_dialog(window, cx);
                        })),
                )
                .command(
                    BarCommand::new(c.move_to_trash, IconName::Delete)
                        .shortcut("Delete")
                        .disabled(!(facts.all_local && facts.writable))
                        .on_click(act(cx, |app, _, _| app.session.trash_selection())),
                )
                .separator();
        }
        bar = bar
            .menu(
                BarCommand::new(c.sort_menu, sort_icon(order.direction)).disabled(on_plugin_page),
                sort_menu,
            )
            .menu(
                BarCommand::new(c.view_menu, view_icon(mode)).disabled(on_plugin_page),
                view_menu,
            )
            .separator()
            .command(
                BarCommand::new(c.preview_pane, IconName::PanelRight)
                    .icon_only()
                    .checked(self.session.preview.open)
                    .on_click(act(cx, |app, _, _| app.session.toggle_preview())),
            )
            .command(
                BarCommand::new(
                    if active_jobs > 0 {
                        SharedString::from(format!("{} · {active_jobs}", c.jobs_panel))
                    } else {
                        SharedString::from(c.jobs_panel)
                    },
                    IconName::LoaderCircle,
                )
                .icon_only()
                .checked(self.session.operations_open)
                .shortcut("Ctrl+O")
                .on_click(act(cx, |app, _, _| {
                    app.session.operations_open = !app.session.operations_open;
                })),
            );

        // The rest: the less frequent file commands, then language and
        // appearance, which every Better OS window keeps in the same place.
        let archive = facts.archive;
        // Undo and redo name what they will do, as folio's do ("Undo
        // Rename"); with nothing to undo they are there, greyed.
        let undo_label = self
            .session
            .history
            .undo_name(c)
            .unwrap_or_else(|| c.undo.to_string());
        let redo_label = self
            .session
            .history
            .redo_name(c)
            .unwrap_or_else(|| c.redo.to_string());
        bar = bar
            .secondary(
                BarCommand::new(undo_label, IconName::Undo)
                    .shortcut("Ctrl+Z")
                    .disabled(!self.session.history.can_undo())
                    .on_click(act(cx, |app, _, _| app.session.undo())),
            )
            .secondary(
                BarCommand::new(redo_label, IconName::Redo)
                    .shortcut("Ctrl+Shift+Z")
                    .disabled(!self.session.history.can_redo())
                    .on_click(act(cx, |app, _, _| app.session.redo())),
            )
            .secondary(
                BarCommand::new(c.new_window, IconName::ExternalLink)
                    .shortcut("Ctrl+N")
                    .on_click(act(cx, |app, _, cx| {
                        crate::app::open_window_at(
                            app.session.location().as_local_path().cloned(),
                            cx,
                        )
                    })),
            )
            .secondary(
                BarCommand::new(c.split_view, IconName::PanelRight)
                    .shortcut("Ctrl+Alt+N")
                    .checked(self.split.is_some())
                    .on_click(act(cx, |app, window, cx| app.toggle_split(window, cx))),
            )
            .secondary_entry(MenuEntry::Separator)
            .secondary(
                BarCommand::new(c.select_all, IconName::CircleCheck)
                    .shortcut("Ctrl+A")
                    .on_click(act(cx, |app, _, _| {
                        let columns = app.columns();
                        app.session
                            .apply_selection(crate::content::SelectionInput::SelectAll, columns);
                    })),
            )
            .secondary(
                BarCommand::new(c.copy_path, IconName::Copy)
                    .shortcut("Ctrl+Shift+C")
                    .on_click(act(cx, |app, _, cx| app.copy_path_text(cx))),
            )
            .secondary(
                BarCommand::new(c.duplicate, IconName::Copy)
                    .shortcut("Ctrl+Shift+D")
                    .disabled(!(facts.all_local && facts.writable))
                    .on_click(act(cx, |app, _, _| app.session.duplicate_selection())),
            )
            .secondary(
                BarCommand::new(c.compress, IconName::Inbox)
                    .shortcut("Ctrl+Shift+P")
                    .disabled(!archive.compress)
                    .on_click(act(cx, |app, _, _| app.session.request_compress())),
            )
            .secondary(
                BarCommand::new(c.extract, IconName::FolderOpen)
                    .shortcut("Ctrl+Shift+E")
                    .disabled(!archive.extract)
                    .on_click(act(cx, |app, _, _| app.session.extract_selection())),
            )
            .secondary(
                BarCommand::new(c.pin_to_sidebar, IconName::Star)
                    .shortcut("Ctrl+P")
                    .on_click(act(cx, |app, _, _| app.session.toggle_pin())),
            )
            .secondary(
                BarCommand::new(c.open_terminal_here, IconName::SquareTerminal)
                    .disabled(in_trash)
                    .on_click(act(cx, |app, _, _| {
                        app.session.open_terminal_here(&crate::terminal::on_path)
                    })),
            )
            .secondary_entry(MenuEntry::Separator)
            .secondary_entry(self.language_menu(c, cx))
            .secondary_entry(self.appearance_menu(c, cx))
            .secondary(
                BarCommand::new(c.keyboard_shortcuts, IconName::Info)
                    .shortcut("?")
                    .on_click(act(cx, |app, _, _| app.show_shortcuts = true)),
            );

        // As many primary commands as fit; the rest move behind `…`. Each
        // command is measured the way the bar draws it: 12px padding, a
        // 16px icon and an 8px gap, the label, and 2px between buttons; a
        // menu command adds its chevron.
        let labeled = |label: &str, menu: bool| {
            px(12.0
                + 16.0
                + 8.0
                + crate::layout::label_width(label)
                + 12.0
                + 4.0
                + if menu { 20.0 } else { 0.0 })
        };
        let icon_only = px(44.0);
        let widths: Vec<Pixels> = if in_trash {
            vec![
                labeled(c.restore_from_trash, false),
                labeled(c.delete_permanently, false),
                labeled(c.sort_menu, true),
                labeled(c.view_menu, true),
                icon_only,
                icon_only,
            ]
        } else {
            vec![
                labeled(c.new_menu, true),
                labeled(c.cut_items, false),
                labeled(c.copy_items, false),
                labeled(c.paste_items, false),
                labeled(c.rename, false),
                labeled(c.move_to_trash, false),
                labeled(c.sort_menu, true),
                labeled(c.view_menu, true),
                icon_only,
                icon_only,
            ]
        };
        let available = px(f32::from(self.viewport.width) - self.nav_width() - 32.0 - 20.0);
        let fit = better_ui::fluent::command_bar::fit_count(&widths, available, icon_only);
        let bar = bar.max_primary(fit);

        div().w_full().px(px(4.0)).child(bar).into_any_element()
    }

    fn language_menu(&self, c: &'static Copy, cx: &mut Context<Self>) -> MenuEntry {
        let resolved = self.locale().resolved();
        let system = self.locale() == Locale::System;
        MenuEntry::submenu(
            c.language,
            vec![
                MenuItem::new(c.english)
                    .checked(!system && resolved == Locale::EnUs)
                    .on_select(act(cx, |app, window, cx| {
                        app.set_locale(Locale::EnUs, window, cx)
                    }))
                    .entry(),
                MenuItem::new(c.chinese)
                    .checked(!system && resolved == Locale::ZhTw)
                    .on_select(act(cx, |app, window, cx| {
                        app.set_locale(Locale::ZhTw, window, cx)
                    }))
                    .entry(),
                MenuItem::new(c.theme_system)
                    .checked(system)
                    .on_select(act(cx, |app, window, cx| {
                        app.set_locale(Locale::System, window, cx)
                    }))
                    .entry(),
            ],
        )
    }

    fn appearance_menu(&self, c: &'static Copy, cx: &mut Context<Self>) -> MenuEntry {
        use better_ui::fluent::theme::Appearance;
        let current = self.appearance;
        let item = |label: &'static str, choice: Appearance| {
            MenuItem::new(label)
                .checked(current == choice)
                .on_select(act(cx, move |app, window, cx| {
                    app.set_appearance(choice, window, cx)
                }))
                .entry()
        };
        MenuEntry::submenu(
            c.appearance,
            vec![
                item(c.light_theme, Appearance::Light),
                item(c.dark_theme, Appearance::Dark),
                item(c.theme_system, Appearance::System),
            ],
        )
    }

    /// The search box's own line while a search runs: what it covers, the
    /// hidden-file setting, its progress, and Close.
    pub(crate) fn search_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let scope = self.session.search.scope_label(self.session.location(), c);
        let status = self.session.search.status_line(c);
        let include_hidden = self.session.search.include_hidden;

        div()
            .w_full()
            .px(px(16.0))
            .py(px(4.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(fluent::label(scope, CAPTION, p.text_secondary))
            .child(
                Button::toggle("search-hidden", c.search_include_hidden, include_hidden).on_click(
                    act(cx, |app, _, _| {
                        app.session.toggle_search_hidden();
                        app.session.preferences.search_hidden = app.session.search.include_hidden;
                    }),
                ),
            )
            .child(div().flex_1())
            .child(fluent::label(status, CAPTION, p.text_secondary))
            .child(
                Button::icon_button("search-close", IconName::Close, c.search_close).on_click(act(
                    cx,
                    |app, window, cx| {
                        app.editing_search = false;
                        app.session.close_search();
                        app.sync_search_field(window, cx);
                    },
                )),
            )
            .into_any_element()
    }

    /// The tab strip, shown once there is more than one tab.
    pub(crate) fn tab_strip(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tabs: Vec<(files_core::TabId, String)> = self
            .session
            .tabs()
            .tabs()
            .iter()
            .map(|tab| (tab.id(), tab.title()))
            .collect();
        if tabs.len() < 2 {
            return None;
        }
        let c = copy(self.locale());
        let active = self.session.active_tab();
        let selected = tabs.iter().position(|(id, _)| *id == active);
        let ids: Rc<Vec<files_core::TabId>> = Rc::new(tabs.iter().map(|(id, _)| *id).collect());
        let kit_tabs = tabs
            .iter()
            .map(|(_, title)| Tab::new(title.clone()).icon(IconName::Folder))
            .collect();
        let select_ids = ids.clone();
        let close_ids = ids.clone();
        let select = cx.entity().downgrade();
        let close = select.clone();
        Some(
            TabView::new("files-tabs", kit_tabs, selected)
                .tooltips(c.close_tab, c.new_tab)
                .on_select(move |index, window, cx| {
                    if let Some(id) = select_ids.get(index).copied() {
                        let _ = select.update(cx, |app, cx| {
                            app.session.activate_tab(id);
                            app.after_location_change(window);
                            cx.notify();
                        });
                    }
                })
                .on_close(move |index, window, cx| {
                    if let Some(id) = close_ids.get(index).copied() {
                        let _ = close.update(cx, |app, cx| {
                            app.session.close_tab(id);
                            app.after_location_change(window);
                            cx.notify();
                        });
                    }
                })
                .on_add(act(cx, |app, window, _| {
                    let location = app.session.home_location();
                    app.session.open_tab(location, true);
                    app.after_location_change(window);
                }))
                .into_any_element(),
        )
    }

    /// What the navigation pane lists, in order, with the line each one is.
    pub(crate) fn nav_targets(&self, c: &'static Copy) -> Vec<(NavEntry, Option<NavTarget>)> {
        let rows = {
            let inputs = SidebarInputs {
                directories: &self.session.directories,
                mounts: &self.session.mounts,
                bookmarks: &self.session.bookmarks,
                states: &NoDeviceStates,
                probe: &FilesystemProbe,
            };
            shown_rows(build_rows(&inputs, c))
        };
        let mut out: Vec<(NavEntry, Option<NavTarget>)> = Vec::new();
        let row_item = |row: &SidebarRow| {
            let mut item = NavItem::new(row.label.clone(), icon_for_row(row));
            if let Some(path) = crate::icons::place_icon(&row.location, &row.key) {
                item = item.icon_path(path);
            }
            if !row.availability.is_available() {
                item = item.caption(c.unavailable);
            } else if row.identity_volatile {
                item = item.caption(c.identity_volatile);
            } else if let Some((status, warning)) = row
                .location
                .as_local_path()
                .and_then(|path| self.plugins.place_status(path.as_path()))
            {
                // A plugin's status on a row Better Files already has
                // (folio's `places`).
                item = item.caption(status).caption_warning(warning);
            }
            item.entry()
        };
        for row in rows
            .iter()
            .filter(|row| row.section == SidebarSection::Places)
        {
            out.push((row_item(row), Some(NavTarget::Row(row.clone()))));
        }
        let pinned: Vec<&SidebarRow> = rows
            .iter()
            .filter(|row| row.section == SidebarSection::Favorites)
            .collect();
        if !pinned.is_empty() {
            out.push((NavEntry::Header(c.sidebar_favorites.into()), None));
            for row in pinned {
                out.push((row_item(row), Some(NavTarget::Row(row.clone()))));
            }
        }
        for row in rows
            .iter()
            .filter(|row| row.section == SidebarSection::Applications)
        {
            out.push((NavEntry::Separator, None));
            out.push((row_item(row), Some(NavTarget::Row(row.clone()))));
        }

        // Dropbox, with its sync state, when the Dropbox command and folder
        // are there.
        if let Some((folder, status)) = self.sharing.dropbox()
            && let Ok(location) = files_core::Location::local(&folder)
        {
            let mut item = NavItem::new("Dropbox", IconName::Inbox);
            if let Some(status) = status {
                item = item.caption(status);
            }
            let row = SidebarRow {
                section: SidebarSection::Places,
                key: "place-dropbox".to_string(),
                label: "Dropbox".to_string(),
                location,
                availability: crate::sidebar::Availability::Available,
                bookmark_index: None,
                device_state: None,
                identity_volatile: false,
            };
            out.push((item.entry(), Some(NavTarget::Row(row))));
        }

        // Each plugin's section, while it has items (folio's `section`).
        for (plugin, poll) in self.plugins.snapshot() {
            let (Some(section), Some(poll)) = (plugin.manifest.section.clone(), poll) else {
                continue;
            };
            if poll.items.is_empty() {
                continue;
            }
            out.push((
                NavEntry::Header(plugin.translate(&section.title).into()),
                None,
            ));
            let opened = self
                .plugin_opened
                .get(&plugin.id)
                .cloned()
                .unwrap_or_default();
            for item in section.rows(&poll.items, &opened) {
                let icon = crate::plugin_page::item_icon(item.icon.as_deref());
                let mut nav = NavItem::new(item.name.clone(), icon);
                if let Some(subtitle) = &item.subtitle {
                    nav = nav.caption(subtitle.render(&plugin.catalog));
                }
                out.push((
                    nav.entry(),
                    Some(NavTarget::PluginItem(Box::new((
                        plugin.clone(),
                        item.clone(),
                    )))),
                ));
            }
            // With a view, the section ends with the row that opens it, and
            // how many items there are.
            if let Some(view) = &plugin.manifest.view {
                let label = view.label.as_deref().unwrap_or(&view.title);
                let nav = NavItem::new(
                    plugin.translate(label),
                    crate::plugin_page::item_icon(view.icon.as_deref()),
                )
                .caption(poll.items.len().to_string());
                out.push((nav.entry(), Some(NavTarget::PluginView(plugin.id.clone()))));
            }
        }

        // The tailnet's machines, online ones first, when Tailscale runs.
        let machines = self.sharing.machines();
        if !machines.is_empty() {
            out.push((NavEntry::Header(c.tailnet.into()), None));
            for machine in machines {
                let item = NavItem::new(machine.name.clone(), IconName::Globe)
                    .caption(if machine.online { c.online } else { c.offline })
                    .disabled(!machine.online);
                out.push((item.entry(), Some(NavTarget::Machine(machine))));
            }
        }

        // Drives: the storage layer's rows when it reports states, the mount
        // table's otherwise, every one of them unknown.
        let note = self.session.collection.note(c);
        if self.session.collection.has_states() {
            let devices: Vec<DeviceRow> = self.session.device_rows().to_vec();
            if !devices.is_empty() || note.is_some() {
                out.push((NavEntry::Header(c.sidebar_devices.into()), None));
            }
            for device in devices {
                out.push((
                    self.device_item(&device, c),
                    Some(NavTarget::Device(device)),
                ));
            }
        } else {
            let drives: Vec<&SidebarRow> = rows
                .iter()
                .filter(|row| row.section == SidebarSection::Devices)
                .collect();
            if !drives.is_empty() {
                out.push((NavEntry::Header(c.sidebar_devices.into()), None));
                for row in drives {
                    let item = NavItem::new(row.label.clone(), IconName::HardDrive)
                        .caption(c.device_state_unknown_without_service);
                    out.push((item.entry(), Some(NavTarget::Row(row.clone()))));
                }
            }
        }
        out
    }

    fn device_item(&self, device: &DeviceRow, c: &'static Copy) -> NavEntry {
        let mut caption = device.state_label(c);
        if device.identity_volatile {
            caption = format!("{caption} · {}", c.identity_volatile);
        }
        if device.unsafe_removal.is_some() {
            caption = format!("{caption} · {}", c.device_unsafe_removal);
        }
        let object_path = device.object_path.clone();
        let this = self.weak.clone();
        NavItem::new(device.label.clone(), IconName::HardDrive)
            .caption(caption)
            .caption_warning(device.is_warning() || device.unsafe_removal.is_some())
            .action(NavAction::new(
                IconName::ArrowUp,
                c.device_eject,
                move |_window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        app.session.eject_device(&object_path);
                        cx.notify();
                    });
                },
            ))
            .entry()
    }

    /// The navigation pane around `content`.
    pub(crate) fn navigation(&mut self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let lines = self.nav_targets(c);
        let current = self.session.location().clone();
        // A plugin's page lights its own row, not the folder behind it.
        let selected = match &self.plugin_view {
            Some(page) => lines.iter().position(|(_, target)| {
                matches!(target, Some(NavTarget::PluginView(id)) if *id == page.plugin_id)
            }),
            None => lines.iter().position(|(_, target)| {
                target.as_ref().and_then(NavTarget::location) == Some(&current)
            }),
        };
        let targets: Rc<Vec<Option<NavTarget>>> =
            Rc::new(lines.iter().map(|(_, target)| target.clone()).collect());
        let entries: Vec<NavEntry> = lines.into_iter().map(|(entry, _)| entry).collect();
        let compact = self.nav_compact();

        let select_targets = targets.clone();
        let select = cx.entity().downgrade();
        let context_targets = targets.clone();
        let context = select.clone();
        let view = NavigationView::new("files-nav", entries, selected)
            .compact(compact)
            .title(format!("{} {}", c.brand_name, c.files))
            .locale(kit_locale(self.locale()))
            .content(content)
            .on_select(move |index, window, cx| {
                let Some(Some(target)) = select_targets.get(index).cloned() else {
                    return;
                };
                let _ = select.update(cx, |app, cx| {
                    app.open_nav_target(&target, window);
                    cx.notify();
                });
            })
            .on_toggle(act_bool(cx, |app, compact| app.nav_compact = Some(compact)))
            .on_context(move |index, position, _window, cx| {
                let Some(Some(target)) = context_targets.get(index).cloned() else {
                    return;
                };
                let _ = context.update(cx, |app, cx| {
                    app.open_sidebar_menu(target, position, cx);
                    cx.notify();
                });
            });

        // A folder dropped anywhere on the pane is pinned, as in folio.
        div()
            .id("files-nav-drop")
            .size_full()
            .child(view)
            .on_drop(
                cx.listener(|this, dragged: &crate::views::DraggedEntry, _window, cx| {
                    this.session.pin(&dragged.location);
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    pub(crate) fn open_nav_target(&mut self, target: &NavTarget, window: &mut Window) {
        // Any other row leaves a plugin's page.
        if !matches!(target, NavTarget::PluginView(_) | NavTarget::PluginItem(_)) {
            self.plugin_view = None;
        }
        match target {
            NavTarget::Row(row) => {
                if let Some(index) = row.bookmark_index {
                    self.session.focus = Focus::Sidebar;
                    self.session.sidebar_cursor = Some(index);
                } else {
                    self.session.focus = Focus::Content;
                }
                if row.availability.is_available() {
                    self.session.navigate_to(row.location.clone());
                }
            }
            NavTarget::Device(device) => {
                self.session.open_device(&device.object_path);
            }
            NavTarget::PluginItem(pair) => {
                let (plugin, item) = pair.as_ref();
                self.open_plugin_item(plugin, item);
            }
            NavTarget::PluginView(id) => {
                self.open_plugin_view(id);
            }
            NavTarget::Machine(machine) => {
                self.session
                    .mount_server(crate::share::sftp_address(machine), None);
            }
        }
        self.after_location_change(window);
    }
}

fn act_bool(
    cx: &Context<FilesApp>,
    f: impl Fn(&mut FilesApp, bool) + 'static,
) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    let this = cx.entity().downgrade();
    move |value, _window, cx| {
        let _ = this.update(cx, |app, cx| {
            f(app, value);
            cx.notify();
        });
    }
}

fn sort_icon(direction: files_core::SortDirection) -> IconName {
    match direction {
        files_core::SortDirection::Ascending => IconName::SortAscending,
        files_core::SortDirection::Descending => IconName::SortDescending,
    }
}

fn view_icon(mode: ViewMode) -> IconName {
    match mode {
        ViewMode::Grid => IconName::LayoutDashboard,
        ViewMode::List => IconName::GalleryVerticalEnd,
        ViewMode::Columns => IconName::PanelLeft,
    }
}

fn icon_for_location(location: &files_core::Location) -> IconName {
    match location {
        files_core::Location::Trash(_) => IconName::Delete,
        files_core::Location::Applications => IconName::LayoutDashboard,
        _ => IconName::Folder,
    }
}

fn icon_for_row(row: &SidebarRow) -> IconName {
    match row.section {
        SidebarSection::Places => match &row.location {
            files_core::Location::Trash(_) => IconName::Delete,
            files_core::Location::Recent => IconName::Calendar,
            _ if row.key == "place-home" => IconName::User,
            _ => IconName::Folder,
        },
        SidebarSection::Devices => IconName::HardDrive,
        SidebarSection::Applications => IconName::LayoutDashboard,
        SidebarSection::Favorites => IconName::Star,
    }
}

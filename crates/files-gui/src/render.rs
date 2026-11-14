//! The frame: title bar, navigation pane, address row, command bar, content,
//! the side panels, and whatever floats over them (a context menu, a dialog,
//! the shortcut sheet).

use better_ui::fluent::{
    self, BODY, BODY_STRONG, CAPTION, SUBTITLE,
    info_bar::{InfoBar, Severity},
    menu::{ContextMenu, MenuEntry, MenuItem, open_menu},
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName};

use crate::app::{FilesApp, OpenMenu};
use crate::context_menu::{
    MenuCommand, MenuLine, MenuTarget, SidebarKind, background_menu, selection_menu, sidebar_menu,
};
use crate::i18n::copy;
use crate::shell::{NavTarget, act, kit_locale};

impl Render for FilesApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The viewport is read once per frame and remembered, so a keyboard
        // page is the size of the page that is actually on screen rather than
        // a guess made when the window opened.
        let window_size = window.viewport_size();
        self.window_width = window_size.width;
        self.viewport = window_size;
        if let Some(width) = self.pane_width.filter(|_| self.embedded) {
            self.viewport.width = px(self.nav_width() + width);
        } else if let Some(right) = &self.split {
            let nav = self.nav_width();
            let own = crate::layout::split_viewport_width(f32::from(window_size.width), nav);
            self.viewport.width = px(own);
            let compact = self.nav_compact;
            right.update(cx, |right, _| {
                right.pane_width = Some(own - nav);
                right.nav_compact = compact;
            });
        }
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let compact = self.compact();

        // A menu closed by a click elsewhere, Escape, or a choice is dropped.
        if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| !menu.state.read(cx).is_open())
        {
            self.context_menu = None;
            // The keys go back to the files, as a closed WinUI flyout returns
            // focus to what opened it.
            window.focus(&self.focus_handle, cx);
        }
        if std::mem::take(&mut self.clear_password) {
            self.password_input
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        // A sign-in that has just been asked for gets its fields ready.
        if matches!(
            self.session.dialog,
            Some(crate::session::PendingDialog::ServerLogin { .. })
        ) && !self.login_prepared
        {
            self.login_prepared = true;
            self.prepare_dialog(window, cx);
        } else if !matches!(
            self.session.dialog,
            Some(crate::session::PendingDialog::ServerLogin { .. })
        ) {
            self.login_prepared = false;
        }
        // A filter is cleared when the folder changes; the field follows.
        if !self.filtering
            && self.session.filter().is_empty()
            && !self.filter_input.read(cx).value().is_empty()
        {
            self.filter_input
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        // Nothing has the keyboard (the window just opened, or the control
        // that had it is gone): the files take it.
        if window.focused(cx).is_none() {
            window.focus(&self.focus_handle, cx);
        }

        let notice = self.session.notice.as_ref().map(|notice| {
            let severity = match notice.tone() {
                crate::session::NoticeTone::Success => Severity::Success,
                crate::session::NoticeTone::Information => Severity::Informational,
                crate::session::NoticeTone::Warning => Severity::Warning,
            };
            (severity, notice.message(c))
        });
        let address = self.address_row(cx);
        let commands = self.command_bar(cx);
        let tab_strip = self.tab_strip(cx);
        // A narrow search field says "Search", not half of a sentence.
        let short = self.search_width() < 200.0;
        if short != self.search_short {
            self.search_short = short;
            let hint = if short {
                c.search_short
            } else {
                c.search_placeholder
            };
            self.search_input
                .update(cx, |state, cx| state.set_placeholder(hint, window, cx));
        }
        // Moving anywhere closes a plugin's page.
        if self
            .plugin_view
            .as_ref()
            .is_some_and(|page| &page.at != self.session.location())
        {
            self.plugin_view = None;
        }
        let content = match self.plugin_page(cx) {
            Some(page) => page,
            None => self.content(cx),
        };
        let operations = self
            .session
            .operations_open
            .then(|| self.operation_center(cx));
        let dialog = self.dialog(cx);
        let policy_dialog = self.policy_dialog(cx);
        let search_bar = self.session.search.is_active().then(|| self.search_bar(cx));
        // The preview pane and the details panel are the same slot: both are a
        // column about the selected item, and showing two of them would leave
        // the content area a sliver on a laptop screen.
        let details = self.application_details(cx);
        // A plugin's page shows its own details; the preview is the folder's.
        let preview = (details.is_none()
            && self.session.preview.open
            && !compact
            && self.plugin_view.is_none())
        .then(|| self.preview_pane(cx));
        let chooser = self.chooser_overlay(cx);
        let menu = self.context_menu_element(cx);
        let sheet = self.show_shortcuts.then(|| self.shortcut_sheet(cx));

        let notice_bar = notice.map(|(severity, message)| {
            InfoBar::new("files-notice", severity, message)
                .locale(kit_locale(self.locale()))
                .on_close(act(cx, |app, _, _| app.session.notice = None))
        });

        let page = div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(address)
            .child(commands)
            .children(tab_strip)
            .children(search_bar)
            .children(notice_bar.map(|bar| div().px(px(16.0)).pb(px(8.0)).child(bar)))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_t_1()
                    .border_color(p.divider)
                    .child(content)
                    .children(preview)
                    .children(details)
                    .children(operations),
            )
            .into_any_element();
        // Split view: the right pane is a second Better Files of its own
        // (its own tabs, history and selection) without the title bar and
        // the sidebar, which belong to the window.
        let page = match self.split.clone() {
            Some(right) if !self.embedded => div()
                .size_full()
                .flex()
                .child(div().flex_1().min_w_0().h_full().child(page))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .border_l_1()
                        .border_color(p.divider)
                        .child(right),
                )
                .into_any_element(),
            _ => page,
        };
        let shell = if self.embedded {
            page
        } else {
            self.navigation(page, cx)
        };

        // Mutter gives an `xdg-toplevel` client no decorations, so this window
        // draws its own or it cannot be closed, minimized, maximized or moved.
        // The dialog and the chooser stay inside the content area rather than
        // over the bar, so the window controls are never covered.
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window_background)
            .text_color(p.text_primary)
            .track_focus(&self.focus_handle)
            .key_context("BetterFiles")
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            // The mouse's back and forward buttons, as in folio.
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.session.go_back();
                    this.after_location_change(window);
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.session.go_forward();
                    this.after_location_change(window);
                    cx.notify();
                }),
            )
            // Ctrl+wheel zooms the items instead of scrolling them. It is
            // caught before the list sees the wheel, so the list stays put.
            .child({
                let this = cx.entity().downgrade();
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(
                            move |event: &ScrollWheelEvent, phase, _window, cx| {
                                if phase != DispatchPhase::Capture || !event.modifiers.control {
                                    return;
                                }
                                let dy = match event.delta {
                                    ScrollDelta::Lines(lines) => lines.y,
                                    ScrollDelta::Pixels(pixels) => f32::from(pixels.y),
                                };
                                if dy == 0.0 {
                                    return;
                                }
                                cx.stop_propagation();
                                let _ = this.update(cx, |app, cx| {
                                    let scale =
                                        crate::prefs::zoom_step(app.session.preferences.scale, dy);
                                    if scale != app.session.preferences.scale {
                                        app.session.set_scale(scale);
                                        cx.notify();
                                    }
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_0()
            })
            .when(!self.embedded, |root| {
                root.child(better_ui::window_chrome::title_bar(
                    Icon::new(IconName::Folder).size(px(16.0)),
                    format!("{} {}", c.brand_name, c.files),
                    p.text_primary.into(),
                ))
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(shell)
                    .children(dialog)
                    .children(policy_dialog)
                    .children(chooser)
                    .children(sheet),
            )
            .children(menu)
            .children(self.update_restart.toast_layer(
                match self.locale() {
                    crate::i18n::Locale::ZhTw => {
                        better_ui::fluent::words::Locale::TraditionalChinese
                    }
                    _ => better_ui::fluent::words::Locale::English,
                },
                cx,
            ))
    }
}

impl FilesApp {
    /// Opens the content area's menu at `position`.
    pub(crate) fn open_content_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = fluent::menu::new_state(cx);
        open_menu(&state, None, window, cx);
        if target == MenuTarget::Selection {
            self.sharing.discover_localsend();
        }
        self.context_menu = Some(OpenMenu {
            target,
            nav: None,
            position,
            state,
        });
        cx.notify();
    }

    /// Opens a navigation-pane row's menu at `position`.
    pub(crate) fn open_sidebar_menu(
        &mut self,
        target: NavTarget,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let state = fluent::menu::new_state(cx);
        state.update(cx, |menu, cx| {
            menu.open_with_highlight(None);
            cx.notify();
        });
        self.context_menu = Some(OpenMenu {
            target: MenuTarget::Sidebar(0),
            nav: Some(target),
            position,
            state,
        });
        cx.notify();
    }

    fn menu_lines(&self, menu: &OpenMenu) -> Vec<MenuLine> {
        let c = copy(self.locale());
        match (&menu.target, &menu.nav) {
            (MenuTarget::Selection, _) => selection_menu(&self.session.selection_facts(), c),
            (MenuTarget::Background, _) => background_menu(&self.session.selection_facts(), c),
            (MenuTarget::Sidebar(_), Some(NavTarget::Row(row))) => {
                let kind = match row.bookmark_index {
                    Some(index) => SidebarKind::Bookmark {
                        index,
                        count: self.session.bookmarks.len(),
                    },
                    None => match row.section {
                        crate::sidebar::SidebarSection::Places
                            if matches!(row.location, files_core::Location::Local(_)) =>
                        {
                            SidebarKind::Place {
                                pinned: self.session.bookmarks.contains(&row.location),
                            }
                        }
                        _ => SidebarKind::View,
                    },
                };
                sidebar_menu(kind, c)
            }
            (MenuTarget::Sidebar(_), Some(NavTarget::Device(device))) => sidebar_menu(
                SidebarKind::Device {
                    mounted: device.mount_point.is_some(),
                },
                c,
            ),
            (
                MenuTarget::Sidebar(_),
                Some(NavTarget::Machine(_) | NavTarget::PluginItem(_) | NavTarget::PluginView(_)),
            )
            | (MenuTarget::Sidebar(_), None) => Vec::new(),
        }
    }

    fn context_menu_element(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.context_menu.as_ref()?;
        let lines = self.menu_lines(menu);
        let nav = menu.nav.clone();
        let mut entries: Vec<MenuEntry> = lines
            .into_iter()
            .map(|line| match line {
                MenuLine::Separator => MenuEntry::Separator,
                MenuLine::Command {
                    command,
                    label,
                    shortcut,
                    enabled,
                    checked,
                } => {
                    let nav = nav.clone();
                    let mut item = MenuItem::new(label).disabled(!enabled).on_select(act(
                        cx,
                        move |app, window, cx| {
                            app.run_menu_command(command, nav.as_ref(), window, cx);
                        },
                    ));
                    if let Some(shortcut) = shortcut {
                        item = item.shortcut(shortcut);
                    }
                    if let Some(checked) = checked {
                        item = item.checked(checked);
                    }
                    item.entry()
                }
            })
            .collect();
        // A drive's other removal policy, worded with the policy's own name
        // so the menu matches the state line and the confirmation.
        if let Some(NavTarget::Device(device)) = &nav
            && let Some(policy) = crate::policy::offered_switch(device.policy)
        {
            let c = copy(self.locale());
            let name = match policy {
                storage_core::RemovalPolicy::Performance => c.device_policy_performance,
                storage_core::RemovalPolicy::DirectRemoval => c.device_policy_direct_removal,
            };
            let path = device.object_path.clone();
            entries.push(MenuEntry::Separator);
            entries.push(
                MenuItem::new(c.device_policy_switch.replace("{policy}", name))
                    .on_select(act(cx, move |app, _, _| {
                        app.session.choose_policy(&path, policy)
                    }))
                    .entry(),
            );
        }
        // Share, for the selection: Taildrop to each online machine, and a
        // Dropbox link for something in the Dropbox folder.
        if menu.target == MenuTarget::Selection {
            let c = copy(self.locale());
            let files: Vec<std::path::PathBuf> = self
                .session
                .selected_entries()
                .iter()
                .filter_map(|entry| entry.as_local_path())
                .map(|path| path.as_path().to_path_buf())
                .collect();
            let mut share: Vec<MenuEntry> = Vec::new();
            for machine in self
                .sharing
                .machines()
                .into_iter()
                .filter(|machine| machine.online)
            {
                let files = files.clone();
                let label = c.taildrop_to.replace("{machine}", &machine.name);
                share.push(
                    MenuItem::new(label)
                        .on_select(act(cx, move |app, _, _| {
                            app.taildrop(files.clone(), machine.clone())
                        }))
                        .entry(),
                );
            }
            for peer in self.sharing.localsend_peers() {
                let files: Vec<std::path::PathBuf> = files
                    .iter()
                    .filter(|file| crate::localsend::sendable(file))
                    .cloned()
                    .collect();
                if files.is_empty() {
                    break;
                }
                let label = c.localsend_to.replace("{device}", &peer.alias);
                share.push(
                    MenuItem::new(label)
                        .on_select(act(cx, move |app, _, _| {
                            app.sharing.localsend(files.clone(), peer.clone())
                        }))
                        .entry(),
                );
            }
            if let (Some((folder, _)), [single]) = (self.sharing.dropbox(), files.as_slice())
                && single.starts_with(&folder)
            {
                let single = single.clone();
                share.push(
                    MenuItem::new(c.copy_dropbox_link)
                        .on_select(act(cx, move |app, _, cx| {
                            app.copy_dropbox_link(&single, cx)
                        }))
                        .entry(),
                );
            }
            // Plugins' actions: folio's `actions`, in the menu itself or
            // under Share, a submenu of the plugin's items for `to: items`.
            let selection: Vec<(std::path::PathBuf, Option<String>, bool)> = self
                .session
                .selected_entries()
                .iter()
                .filter_map(|entry| {
                    let path = entry.as_local_path()?.as_path().to_path_buf();
                    Some((
                        path,
                        entry.mime.as_ref().map(|mime| mime.as_str().to_string()),
                        entry.kind == files_core::EntryKind::Directory,
                    ))
                })
                .collect();
            let mut own: Vec<MenuEntry> = Vec::new();
            for (plugin, poll) in self.plugins.snapshot() {
                if plugin.manifest.poll.is_some() && poll.is_none() {
                    // A failing poll offers no actions.
                    continue;
                }
                let items = poll
                    .as_ref()
                    .map(|poll| poll.items.clone())
                    .unwrap_or_default();
                let available = poll.as_ref().and_then(|poll| poll.actions.clone());
                for action in plugin.manifest.actions.clone() {
                    if !crate::plugins::action_shows(
                        &action,
                        available.as_deref(),
                        &selection,
                        items.len(),
                        self.plugins.home(),
                    ) {
                        continue;
                    }
                    let paths: Vec<std::path::PathBuf> =
                        selection.iter().map(|(path, _, _)| path.clone()).collect();
                    let entry = if action.to_items() {
                        let children = items
                            .iter()
                            .map(|item| {
                                let (plugin, command, paths, item, detach) = (
                                    plugin.clone(),
                                    action.command.clone(),
                                    paths.clone(),
                                    item.clone(),
                                    action.detach,
                                );
                                MenuItem::new(item.name.clone())
                                    .on_select(act(cx, move |app, _, _| {
                                        app.plugins.run(
                                            plugin.clone(),
                                            command.clone(),
                                            paths.clone(),
                                            Some(item.clone()),
                                            detach,
                                        )
                                    }))
                                    .entry()
                            })
                            .collect();
                        MenuEntry::submenu(plugin.translate(&action.label), children)
                    } else {
                        let (plugin_run, command, detach) =
                            (plugin.clone(), action.command.clone(), action.detach);
                        MenuItem::new(plugin.translate(&action.label))
                            .on_select(act(cx, move |app, _, _| {
                                app.plugins.run(
                                    plugin_run.clone(),
                                    command.clone(),
                                    paths.clone(),
                                    None,
                                    detach,
                                )
                            }))
                            .entry()
                    };
                    if action.in_share() {
                        share.push(entry);
                    } else {
                        own.push(entry);
                    }
                }
            }
            if !own.is_empty() {
                entries.push(MenuEntry::Separator);
                entries.extend(own);
            }
            if !files.is_empty() && !share.is_empty() {
                entries.push(MenuEntry::Separator);
                entries.push(MenuEntry::submenu(c.share, share));
            }
            // A disk image can be written to a drive, as in folio.
            if let [single] = files.as_slice()
                && crate::context_menu::is_disk_image(single)
            {
                let targets = self.session.image_targets();
                if !targets.is_empty() {
                    entries.push(MenuEntry::Separator);
                    for target in targets {
                        let image = single.clone();
                        let object_path = target.object_path;
                        entries.push(
                            MenuItem::new(c.write_image_to.replace("{device}", &target.name))
                                .on_select(act(cx, move |app, window, cx| {
                                    if let Ok(image) = files_core::LocalPath::new(image.clone()) {
                                        app.session.request_write_image(image, &object_path);
                                        app.prepare_dialog(window, cx);
                                    }
                                }))
                                .entry(),
                        );
                    }
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        Some(
            ContextMenu::new(
                "files-context-menu",
                entries,
                menu.state.clone(),
                menu.position,
            )
            .into_any_element(),
        )
    }

    /// Carries out a menu line: the window's own part (the clipboard, the
    /// dialogs' text, a drive), then the session's.
    pub(crate) fn run_menu_command(
        &mut self,
        command: MenuCommand,
        nav: Option<&NavTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        match (command, nav) {
            (MenuCommand::CopyPath, None) => self.copy_path_text(cx),
            (MenuCommand::Eject, Some(NavTarget::Device(device))) => {
                self.session.eject_device(&device.object_path)
            }
            (MenuCommand::Mount, Some(NavTarget::Device(device))) => {
                self.session.mount_device(&device.object_path)
            }
            (MenuCommand::Format, Some(NavTarget::Device(device))) => {
                self.session.request_format(&device.object_path)
            }
            (MenuCommand::Open, Some(target)) => self.open_nav_target(target, window),
            (_, Some(NavTarget::Device(device))) if command == MenuCommand::OpenInNewTab => {
                if let Some(mount) = device.mount_point.clone()
                    && let Ok(location) = files_core::Location::local(mount)
                {
                    self.session.open_tab(location, true);
                }
            }
            (_, Some(NavTarget::Row(row))) => {
                let columns = self.columns();
                self.session.run_menu_command(command, Some(row), columns);
            }
            (_, Some(NavTarget::Device(_)))
            | (_, Some(NavTarget::Machine(_)))
            | (_, Some(NavTarget::PluginItem(_) | NavTarget::PluginView(_))) => {}
            (_, None) => {
                let columns = self.columns();
                self.session.run_menu_command(command, None, columns);
            }
        }
        self.prepare_dialog(window, cx);
        self.sync_chooser(window, cx);
        self.after_location_change(window);
    }

    /// Every shortcut, on one sheet over the window.
    fn shortcut_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale());
        let p = fluent::palette(cx);
        let locale = self.locale();
        let two_columns = f32::from(self.viewport.width) >= 1_200.0;
        let lines: Vec<AnyElement> = crate::shortcuts::SHORTCUTS
            .iter()
            .map(|shortcut| {
                div()
                    .flex()
                    .gap(px(16.0))
                    .py(px(4.0))
                    .child(
                        fluent::label(shortcut.keys, BODY_STRONG, p.accent_text)
                            .w(px(220.0))
                            .flex_none(),
                    )
                    .child(fluent::label(
                        shortcut.describe(locale),
                        BODY,
                        p.text_primary,
                    ))
                    .into_any_element()
            })
            .collect();
        let half = lines.len().div_ceil(2);
        let mut columns = div().flex().gap(px(32.0));
        if two_columns {
            let mut lines = lines;
            let second: Vec<AnyElement> = lines.split_off(half);
            columns = columns
                .child(div().flex().flex_col().children(lines))
                .child(div().flex().flex_col().children(second));
        } else {
            columns = columns.child(div().flex().flex_col().children(lines));
        }
        div()
            .id("shortcut-sheet")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(fluent::with_alpha(fluent::hex(0x000000), 0.4))
            .on_click({
                let close = act(cx, |app, _, _| app.show_shortcuts = false);
                move |_, window, cx| close(window, cx)
            })
            .child(
                div()
                    .id("shortcut-sheet-card")
                    .max_h(relative(0.9))
                    .max_w(relative(0.94))
                    .overflow_y_scroll()
                    .p(px(24.0))
                    .rounded(fluent::radius::MD)
                    .bg(p.dialog)
                    .border_1()
                    .border_color(p.stroke)
                    .flex()
                    .flex_col()
                    .gap(px(16.0))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(fluent::label(
                                c.keyboard_shortcuts,
                                SUBTITLE,
                                p.text_primary,
                            ))
                            .child(div().flex_1())
                            .child(
                                fluent::button::Button::icon_button(
                                    "shortcut-sheet-close",
                                    IconName::Close,
                                    c.close,
                                )
                                .on_click(act(cx, |app, _, _| app.show_shortcuts = false)),
                            ),
                    )
                    .child(columns)
                    .child(fluent::label("Esc", CAPTION, p.text_secondary)),
            )
            .into_any_element()
    }
}

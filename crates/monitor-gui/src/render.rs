//! The window: title bar, navigation pane, the page's title and commands,
//! and whatever floats over the page (a dialog, a context menu).

use better_ui::fluent::chrome::TitleBar;
use better_ui::fluent::command_bar::{Command, CommandBar};
use better_ui::fluent::dialog::{ContentDialog, DefaultButton, DialogButtons, DialogResult};
use better_ui::fluent::menu::{ContextMenu, MenuEntry, MenuItem};
use better_ui::fluent::navigation::{NavEntry, NavItem, NavigationView};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::{BODY, TITLE, label, palette, spacing};
use gpui::*;
use gpui_component::{Icon, IconName};
use monitor_core::ProcessAction;

use crate::app::{Dialog, EndTarget, MenuTarget, MonitorApp, Page};
use crate::i18n::{Copy, copy};
use crate::layout::{PAGE_MARGIN, commands_that_fit, content_width, navigation_compact};
use crate::shell::property_row;
use crate::tables::{AppSelection, end_offered};

pub(crate) fn page_title(page: Page, c: &'static Copy) -> &'static str {
    match page {
        Page::Overview => c.overview,
        Page::Processes => c.processes,
        Page::Details => c.details,
        Page::Performance => c.performance,
        Page::Startup => c.startup_apps,
        Page::Services => c.services,
        Page::History => c.history,
        Page::Events => c.events,
        Page::SystemInfo => c.system_info,
        Page::Settings => c.settings,
    }
}

fn page_icon(page: Page) -> IconName {
    match page {
        Page::Overview => IconName::LayoutDashboard,
        Page::Processes => IconName::GalleryVerticalEnd,
        Page::Details => IconName::SquareTerminal,
        Page::Performance => IconName::ChartPie,
        Page::Startup => IconName::Play,
        Page::Services => IconName::Settings2,
        Page::History => IconName::Calendar,
        Page::Events => IconName::Bell,
        Page::SystemInfo => IconName::Info,
        Page::Settings => IconName::Settings,
    }
}

impl Render for MonitorApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let width = f32::from(window.viewport_size().width);
        let compact = self
            .compact_choice
            .unwrap_or_else(|| navigation_compact(width));
        let available = content_width(width, compact);

        let content = self.page_frame(available, window, cx);

        let entries: Vec<NavEntry> = Page::ALL[..Page::ALL.len() - 1]
            .iter()
            .map(|page| NavItem::new(page_title(*page, c), page_icon(*page)).entry())
            .collect();
        let nav = NavigationView::new("monitor-nav", entries, Some(self.page.index()))
            .footer(vec![NavItem::new(
                page_title(Page::Settings, c),
                page_icon(Page::Settings),
            )])
            .compact(compact)
            .title(c.app_name)
            .locale(self.locale.words())
            .on_select(Self::act_with(cx, |this, index: usize, _, cx| {
                if let Some(page) = Page::ALL.get(index) {
                    this.navigate(*page, cx);
                }
            }))
            .on_toggle(Self::act_with(cx, |this, compact: bool, _, _| {
                this.compact_choice = Some(compact)
            }))
            .content(content);

        let dialog = self.dialog_element(cx);
        let menu = self.context_menu(cx);

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window_background)
            .text_color(p.text_primary)
            .child(
                TitleBar::new(c.app_name)
                    .icon(Icon::new(IconName::Cpu).size(px(16.0)).text_color(p.accent)),
            )
            .child(div().flex_1().min_h_0().child(nav))
            .children(dialog)
            .children(menu)
            .children(self.update_restart.toast_layer(self.locale.words(), cx))
    }
}

impl MonitorApp {
    /// The commands a page offers beside its title.
    fn commands(&self, cx: &mut Context<Self>) -> Vec<Command> {
        let c = copy(self.locale);
        let mut commands = Vec::new();
        match self.page {
            Page::Processes => {
                let target = self.selected_app_target();
                commands.push(
                    Command::new(c.end_task, IconName::CircleX)
                        .disabled(!end_offered(self.paused, target.is_some()))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some(target) = this.selected_app_target() {
                                this.request_end(target, cx);
                            }
                        })),
                );
                let pid = match &self.app_selection {
                    Some(AppSelection::Process(pid)) => Some(*pid),
                    _ => None,
                };
                commands.push(
                    Command::new(c.properties, IconName::Info)
                        .disabled(pid.is_none())
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some(pid) = pid {
                                this.show_properties(pid, cx);
                            }
                        })),
                );
            }
            Page::Details => {
                let selected = self.selected_pid.and_then(|pid| self.process(pid));
                let target = selected.map(|process| (process.pid, process.display_name()));
                let end = target.clone();
                commands.push(
                    Command::new(c.end_task, IconName::CircleX)
                        .disabled(!end_offered(self.paused, target.is_some()))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some((pid, name)) = end.clone() {
                                this.request_end(EndTarget::Process { pid, name }, cx);
                            }
                        })),
                );
                let tree = target.clone();
                commands.push(
                    Command::new(c.end_process_tree, IconName::ChevronsUpDown)
                        .disabled(!end_offered(self.paused, target.is_some()))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some((pid, name)) = tree.clone() {
                                this.request_end(EndTarget::Tree { pid, name }, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.properties, IconName::Info)
                        .disabled(target.is_none())
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some(pid) = this.selected_pid {
                                this.show_properties(pid, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.tree_view, IconName::ChevronsUpDown)
                        .checked(self.processes.tree_mode())
                        .on_click(Self::act(cx, |this, _, cx| this.toggle_tree_mode(cx))),
                );
            }
            Page::Startup => {
                let selected = self.startup.selected.clone().and_then(|file| {
                    self.startup
                        .entries
                        .iter()
                        .find(|entry| entry.file_name == file)
                        .map(|entry| (entry.file_name.clone(), entry.enabled))
                });
                let (on, off) = (selected.clone(), selected.clone());
                commands.push(
                    Command::new(c.enable, IconName::CircleCheck)
                        .disabled(!matches!(selected, Some((_, false))))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some((file, _)) = on.clone() {
                                this.switch_startup(file, true, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.disable, IconName::CircleX)
                        .disabled(!matches!(selected, Some((_, true))))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some((file, _)) = off.clone() {
                                this.switch_startup(file, false, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.refresh, IconName::Redo)
                        .on_click(Self::act(cx, |this, _, cx| this.reload_startup(cx))),
                );
            }
            Page::Services => {
                let selected = self.services.selected.clone().and_then(|unit| {
                    self.services
                        .services
                        .iter()
                        .find(|service| service.unit == unit)
                        .cloned()
                });
                let switchable = selected.as_ref().filter(|s| s.lock().is_none());
                let enabled =
                    switchable.map(|s| s.startup == crate::startup::StartupState::Enabled);
                let (on, off) = (
                    switchable.map(|s| s.unit.clone()),
                    switchable.map(|s| s.unit.clone()),
                );
                commands.push(
                    Command::new(c.enable, IconName::CircleCheck)
                        .disabled(enabled != Some(false))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some(unit) = on.clone() {
                                this.switch_service(unit, true, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.disable, IconName::CircleX)
                        .disabled(enabled != Some(true))
                        .on_click(Self::act(cx, move |this, _, cx| {
                            if let Some(unit) = off.clone() {
                                this.switch_service(unit, false, cx);
                            }
                        })),
                );
                commands.push(
                    Command::new(c.refresh, IconName::Redo)
                        .on_click(Self::act(cx, |this, _, cx| this.reload_services(cx))),
                );
            }
            Page::Events => {
                commands.push(
                    Command::new(c.mark_incident, IconName::Star)
                        .disabled(!self.can_mark())
                        .on_click(Self::act(cx, |this, _, cx| this.mark_incident(cx))),
                );
            }
            _ => {}
        }
        let live = matches!(
            self.page,
            Page::Overview | Page::Processes | Page::Details | Page::Performance
        );
        if live {
            commands.push(
                Command::new(
                    if self.paused {
                        c.resume_updates
                    } else {
                        c.pause_updates
                    },
                    if self.paused {
                        IconName::Play
                    } else {
                        IconName::Pause
                    },
                )
                .on_click(Self::act(cx, |this, _, cx| this.toggle_pause(cx))),
            );
        }
        commands
    }

    /// The page with its title, commands, bars, and content.
    fn page_frame(
        &mut self,
        available: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let title = page_title(self.page, c);
        let commands = self.commands(cx);
        let labels: Vec<&str> = commands
            .iter()
            .map(|command| command.label.as_ref())
            .collect();
        let fit = commands_that_fit(available, title, &labels);
        let mut bar = CommandBar::new("page-commands")
            .locale(self.locale.words())
            .max_primary(fit);
        for command in commands {
            bar = bar.command(command);
        }
        let header = div()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .min_w_0()
            .pt(spacing::XL)
            .child(
                label(title, TITLE, p.text_primary)
                    .flex_1()
                    .min_w_0()
                    .truncate(),
            )
            .child(bar);
        let searchable = matches!(self.page, Page::Processes | Page::Details);
        let search = searchable.then(|| {
            div()
                .flex()
                .items_center()
                .gap(spacing::MD)
                .child(TextBox::search("process-search", &self.filter).width(px(320.0)))
        });
        let stored = matches!(
            self.page,
            Page::Overview | Page::History | Page::Events | Page::SystemInfo
        );
        let bars = self.info_bars(stored, cx);
        let body = self.render_page(available, window, cx);

        if self.page.fills() {
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(spacing::MD)
                .px(px(PAGE_MARGIN))
                .pb(spacing::LG)
                .child(header)
                .children(search)
                .children(bars)
                .child(div().flex_1().min_h_0().flex().flex_col().child(body))
                .into_any_element()
        } else {
            div()
                .id("page-scroll")
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::LG)
                        .px(px(PAGE_MARGIN))
                        .pb(spacing::XL)
                        .child(header)
                        .children(bars)
                        .child(body),
                )
                .into_any_element()
        }
    }

    /// What ending the selected row on the Processes page would reach.
    pub(crate) fn selected_app_target(&self) -> Option<EndTarget> {
        match self.app_selection.as_ref()? {
            AppSelection::Process(pid) => {
                let process = self.process(*pid)?;
                Some(EndTarget::Process {
                    pid: *pid,
                    name: process.display_name(),
                })
            }
            AppSelection::Group(key) => {
                let row = self
                    .apps
                    .applications()
                    .iter()
                    .chain(self.apps.services())
                    .find(|row| &row.group.key == key)?;
                Some(EndTarget::App {
                    name: row.group.display_name.clone(),
                    pids: row.group.pids().collect(),
                })
            }
        }
    }

    fn dialog_element(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let p = palette(cx);
        match self.dialog.clone()? {
            Dialog::End(target) => {
                let count = self.end_pids(&target).len();
                let (title, body) = crate::i18n::end_question(&target, count, self.locale);
                Some(
                    ContentDialog::new("end-dialog", title)
                        .buttons(DialogButtons {
                            primary: Some(
                                match target {
                                    EndTarget::Tree { .. } => c.end_process_tree,
                                    _ => c.end_task,
                                }
                                .into(),
                            ),
                            secondary: None,
                            close: Some(
                                better_ui::fluent::words::action(
                                    self.locale.words(),
                                    better_ui::fluent::words::Action::Cancel,
                                )
                                .into(),
                            ),
                            default: DefaultButton::Close,
                        })
                        .on_result(Self::act_with(cx, |this, result: DialogResult, _, cx| {
                            if result == DialogResult::Primary {
                                this.confirm_end(cx);
                            } else {
                                this.close_dialog(cx);
                            }
                        }))
                        .child(label(body, BODY, p.text_primary))
                        .into_any_element(),
                )
            }
            Dialog::Properties(pid) => {
                let Some(process) = self.process(pid).cloned() else {
                    self.dialog = None;
                    return None;
                };
                let rows = crate::i18n::properties(&process, self.locale, self.processors());
                let mut list = div().flex().flex_col().min_w(px(420.0));
                for (name, value, missing) in rows {
                    list = list.child(property_row(name, value, missing, p));
                }
                Some(
                    ContentDialog::new("properties-dialog", process.display_name())
                        .buttons(DialogButtons {
                            primary: None,
                            secondary: None,
                            close: Some(
                                better_ui::fluent::words::action(
                                    self.locale.words(),
                                    better_ui::fluent::words::Action::Close,
                                )
                                .into(),
                            ),
                            default: DefaultButton::Close,
                        })
                        .on_result(Self::act_with(cx, |this, _: DialogResult, _, cx| {
                            this.close_dialog(cx)
                        }))
                        .child(list)
                        .into_any_element(),
                )
            }
        }
    }

    fn context_menu(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.menu.clone()?;
        if !self.menu_state.read(cx).is_open() {
            return None;
        }
        let c = copy(self.locale);
        let entries: Vec<MenuEntry> = match &open.target {
            MenuTarget::App(AppSelection::Group(key)) => {
                let row = self
                    .apps
                    .applications()
                    .iter()
                    .chain(self.apps.services())
                    .find(|row| &row.group.key == key)?;
                let target = EndTarget::App {
                    name: row.group.display_name.clone(),
                    pids: row.group.pids().collect(),
                };
                let first = row.group.pids().next();
                let mut entries = vec![
                    MenuItem::new(c.end_task)
                        .icon(IconName::CircleX)
                        .disabled(!end_offered(self.paused, true))
                        .on_select(Self::act(cx, move |this, _, cx| {
                            this.request_end(target.clone(), cx)
                        }))
                        .entry(),
                ];
                if let Some(pid) = first {
                    entries.push(
                        MenuItem::new(c.open_file_location)
                            .icon(IconName::FolderOpen)
                            .on_select(Self::act(cx, move |this, _, cx| {
                                this.open_location(pid, cx)
                            }))
                            .entry(),
                    );
                    entries.push(
                        MenuItem::new(c.go_to_details)
                            .icon(IconName::SquareTerminal)
                            .on_select(Self::act(cx, move |this, _, cx| {
                                this.selected_pid = Some(pid);
                                this.navigate(Page::Details, cx);
                            }))
                            .entry(),
                    );
                }
                entries
            }
            MenuTarget::App(AppSelection::Process(pid)) | MenuTarget::Process(pid) => {
                self.process_menu(*pid, cx)?
            }
            MenuTarget::Startup(file) => {
                let entry = self
                    .startup
                    .entries
                    .iter()
                    .find(|entry| &entry.file_name == file)?;
                let (file, enabled) = (entry.file_name.clone(), entry.enabled);
                vec![
                    MenuItem::new(if enabled { c.disable } else { c.enable })
                        .icon(if enabled {
                            IconName::CircleX
                        } else {
                            IconName::CircleCheck
                        })
                        .on_select(Self::act(cx, move |this, _, cx| {
                            this.switch_startup(file.clone(), !enabled, cx)
                        }))
                        .entry(),
                ]
            }
            MenuTarget::Service(unit) => {
                let service = self
                    .services
                    .services
                    .iter()
                    .find(|service| &service.unit == unit)?;
                let enabled = service.startup == crate::startup::StartupState::Enabled;
                let unit = service.unit.clone();
                vec![
                    MenuItem::new(if enabled { c.disable } else { c.enable })
                        .icon(if enabled {
                            IconName::CircleX
                        } else {
                            IconName::CircleCheck
                        })
                        .disabled(service.lock().is_some())
                        .on_select(Self::act(cx, move |this, _, cx| {
                            this.switch_service(unit.clone(), !enabled, cx)
                        }))
                        .entry(),
                ]
            }
        };
        Some(
            ContextMenu::new("row-menu", entries, self.menu_state.clone(), open.position)
                .into_any_element(),
        )
    }

    /// The context menu of one process.
    fn process_menu(&mut self, pid: u32, cx: &mut Context<Self>) -> Option<Vec<MenuEntry>> {
        let c = copy(self.locale);
        let process = self.process(pid)?.clone();
        let name = process.display_name();
        let paused = process.is_paused();
        let nice_step = process
            .nice
            .copied()
            .and_then(|nice| i32::try_from(nice).ok())
            .map(|nice| (nice + 5).min(monitor_core::NICE_MAXIMUM))
            .unwrap_or(monitor_core::NICE_MAXIMUM);
        let list_paused = self.paused;
        let blocked =
            |action: ProcessAction| self.availability(&process, action).refusal().is_some();
        let (end, tree) = (name.clone(), name);
        let pause_action = if paused {
            ProcessAction::Resume
        } else {
            ProcessAction::Pause
        };
        Some(vec![
            MenuItem::new(c.end_task)
                .icon(IconName::CircleX)
                .disabled(blocked(ProcessAction::Terminate) || !end_offered(list_paused, true))
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.request_end(
                        EndTarget::Process {
                            pid,
                            name: end.clone(),
                        },
                        cx,
                    )
                }))
                .entry(),
            MenuItem::new(c.end_process_tree)
                .icon(IconName::ChevronsUpDown)
                .disabled(blocked(ProcessAction::Terminate) || !end_offered(list_paused, true))
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.request_end(
                        EndTarget::Tree {
                            pid,
                            name: tree.clone(),
                        },
                        cx,
                    )
                }))
                .entry(),
            MenuEntry::Separator,
            MenuItem::new(if paused {
                c.resume_process
            } else {
                c.pause_process
            })
            .icon(if paused {
                IconName::Play
            } else {
                IconName::Pause
            })
            .disabled(blocked(pause_action))
            .on_select(Self::act(cx, move |this, _, cx| {
                this.run_action(pid, pause_action, cx)
            }))
            .entry(),
            MenuItem::new(c.lower_priority)
                .icon(IconName::ArrowDown)
                .disabled(blocked(ProcessAction::SetNice(nice_step)))
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.run_action(pid, ProcessAction::SetNice(nice_step), cx)
                }))
                .entry(),
            MenuEntry::Separator,
            MenuItem::new(c.open_file_location)
                .icon(IconName::FolderOpen)
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.open_location(pid, cx)
                }))
                .entry(),
            MenuItem::new(c.properties)
                .icon(IconName::Info)
                .on_select(Self::act(cx, move |this, _, cx| {
                    this.show_properties(pid, cx)
                }))
                .entry(),
        ])
    }
}

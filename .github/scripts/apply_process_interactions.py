from pathlib import Path
import re


def replace_once(source: str, old: str, new: str, label: str) -> str:
    if new in source:
        return source
    if old not in source:
        raise SystemExit(f"missing anchor: {label}")
    return source.replace(old, new, 1)


main_path = Path("crates/monitor-gui/src/main.rs")
main = main_path.read_text()
main = replace_once(
    main,
    "mod process_control;\n",
    "mod process_actions;\nmod process_control;\n",
    "process_actions module",
)
main_path.write_text(main)

app_path = Path("crates/monitor-gui/src/app.rs")
app = app_path.read_text()
app = replace_once(
    app,
    "    charts_paused: bool,\n    sample_index: usize,",
    "    charts_paused: bool,\n    table_refresh_hold_until: Option<Instant>,\n    sample_index: usize,",
    "refresh hold field",
)
app = replace_once(
    app,
    "            TableState::new(ProcessTableDelegate::new(&settings), window, cx)",
    "            TableState::new(\n                ProcessTableDelegate::new(&settings, monitor_target.clone()),\n                window,\n                cx,\n            )",
    "process delegate monitor",
)
app = replace_once(
    app,
    "            charts_paused: false,\n            sample_index: 0,",
    "            charts_paused: false,\n            table_refresh_hold_until: None,\n            sample_index: 0,",
    "refresh hold initialization",
)

old_app_update = '''        let app_groups = self.app_groups.clone();
        let app_query = self.search_query.clone();
        self.app_table.update(cx, |table, cx| {
            table.delegate_mut().set_groups(app_groups);
            table.delegate_mut().set_filter(app_query);
            table.refresh(cx);
            cx.notify();
        });'''
new_app_update = '''        let table_refresh_held = self.table_refresh_is_held();
        if !table_refresh_held {
            let app_groups = self.app_groups.clone();
            let app_query = self.search_query.clone();
            self.app_table.update(cx, |table, cx| {
                table.delegate_mut().set_groups(app_groups);
                table.delegate_mut().set_filter(app_query);
                table.refresh(cx);
                cx.notify();
            });
        }'''
app = replace_once(app, old_app_update, new_app_update, "app table hold")

old_process_update = '''        if self
            .selected_pid
            .is_some_and(|pid| !processes.iter().any(|process| process.pid == pid))
        {
            self.selected_pid = None;
        }
        let query = self.search_query.clone();
        self.process_table.update(cx, |table, cx| {
            table.delegate_mut().set_processes(processes);
            table.delegate_mut().set_filter(query);
            table.refresh(cx);
            cx.notify();
        });'''
new_process_update = '''        if !table_refresh_held {
            if self
                .selected_pid
                .is_some_and(|pid| !processes.iter().any(|process| process.pid == pid))
            {
                self.selected_pid = None;
            }
            let query = self.search_query.clone();
            self.process_table.update(cx, |table, cx| {
                table.delegate_mut().set_processes(processes);
                table.delegate_mut().set_filter(query);
                table.refresh(cx);
                cx.notify();
            });
        }'''
app = replace_once(app, old_process_update, new_process_update, "process table hold")

hold_methods = '''    pub(crate) fn hold_table_refresh(&mut self) {
        self.table_refresh_hold_until = Some(Instant::now() + Duration::from_secs(2));
    }

    fn table_refresh_is_held(&self) -> bool {
        self.table_refresh_hold_until
            .is_some_and(|deadline| Instant::now() < deadline)
    }

'''
app = replace_once(
    app,
    "    fn set_active_page(&mut self, page: MonitorPage) {",
    hold_methods + "    fn set_active_page(&mut self, page: MonitorPage) {",
    "refresh hold methods",
)
app_path.write_text(app)

process_path = Path("crates/monitor-gui/src/process_table.rs")
process = process_path.read_text()
process = replace_once(
    process,
    "use crate::{\n    linux,",
    "use crate::{\n    app::MonitorWindow,\n    linux,\n    process_actions,",
    "process table imports",
)
process = replace_once(
    process,
    "    selected_pids: BTreeSet<u32>,\n}",
    "    selected_pids: BTreeSet<u32>,\n    monitor: WeakEntity<MonitorWindow>,\n}",
    "process table monitor field",
)
process = replace_once(
    process,
    "    pub fn new(settings: &MonitorSettings) -> Self {",
    "    pub fn new(\n        settings: &MonitorSettings,\n        monitor: WeakEntity<MonitorWindow>,\n    ) -> Self {",
    "process table constructor",
)
process = replace_once(
    process,
    "            selected_pids: BTreeSet::new(),\n        };",
    "            selected_pids: BTreeSet::new(),\n            monitor,\n        };",
    "process table monitor initialization",
)

old_selection = '''        if column == ProcessColumn::Selection {
            let pid = process.pid;
            let checked = self.is_selected(pid);
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    Switch::new(("process-selected", pid.as_u32() as usize))
                        .checked(checked)
                        .on_click(cx.listener(move |table, checked, _, cx| {
                            table.delegate_mut().set_selected(pid, *checked);
                            cx.emit(TableEvent::SelectRow(row_ix));
                            cx.notify();
                        })),
                )
                .into_any_element();
        }'''
new_selection = '''        if column == ProcessColumn::Selection {
            let pid = process.pid;
            let checked = self.is_selected(pid);
            let monitor = self.monitor.clone();
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    Switch::new(("process-selected", pid.as_u32() as usize))
                        .checked(checked)
                        .on_click(cx.listener(move |table, checked, _, cx| {
                            let _ = monitor.update(cx, |monitor, _| {
                                monitor.hold_table_refresh();
                            });
                            table.delegate_mut().set_selected(pid, *checked);
                            cx.emit(TableEvent::SelectRow(row_ix));
                            cx.notify();
                        })),
                )
                .into_any_element();
        }'''
process = replace_once(process, old_selection, new_selection, "selection hold")

old_options = '''        if column == ProcessColumn::Options {
            let process = process.clone();
            let button_id = ("process-options", process.pid.as_u32() as usize);
            return div()
                .child(
                    Button::new(button_id)
                        .outline()
                        .small()
                        .label("Options")
                        .on_click(move |_, _, cx| {
                            open_process_options(process.clone(), cx);
                        }),
                )
                .into_any_element();
        }'''
new_options = '''        if column == ProcessColumn::Options {
            let process = process.clone();
            let monitor = self.monitor.clone();
            let button_id = ("process-options", process.pid.as_u32() as usize);
            return div()
                .child(
                    Button::new(button_id)
                        .outline()
                        .small()
                        .label("Options")
                        .on_click(move |_, _, cx| {
                            let _ = monitor.update(cx, |monitor, _| {
                                monitor.hold_table_refresh();
                            });
                            open_process_options(process.clone(), cx);
                        }),
                )
                .into_any_element();
        }'''
process = replace_once(process, old_options, new_options, "options hold")

context_start = process.index("    fn context_menu(")
context_end = process.index("    fn perform_sort(", context_start)
new_context = '''    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(process) = self.processes.get(row_ix).cloned() else {
            return menu;
        };
        let _ = self.monitor.update(cx, |monitor, _| {
            monitor.hold_table_refresh();
        });
        let pid = process.pid;
        let selected = self.is_selected(pid);
        let selected_pids = self.selected_pids();
        let table = cx.entity().downgrade();
        let selection_monitor = self.monitor.clone();
        let options_monitor = self.monitor.clone();
        let options_process = process.clone();

        let menu = menu.item(
            PopupMenuItem::new(if selected {
                "Remove from batch"
            } else {
                "Add to batch"
            })
            .on_click(move |_, _, cx| {
                let _ = selection_monitor.update(cx, |monitor, _| {
                    monitor.hold_table_refresh();
                });
                let _ = table.update(cx, |table, cx| {
                    table.delegate_mut().set_selected(pid, !selected);
                    cx.emit(TableEvent::SelectRow(row_ix));
                    cx.notify();
                });
            }),
        );
        let menu = process_actions::append_process_information(
            menu.separator(),
            process.clone(),
            self.settings.unit_base,
            row_ix,
        )
        .item(
            PopupMenuItem::new("Process Options").on_click(move |_, _, cx| {
                let _ = options_monitor.update(cx, |monitor, _| {
                    monitor.hold_table_refresh();
                });
                open_process_options(options_process.clone(), cx);
            }),
        );
        process_actions::append_process_signal_actions(
            menu,
            self.monitor.clone(),
            process,
            selected_pids,
            row_ix,
        )
    }

'''
process = process[:context_start] + new_context + process[context_end:]
process_path.write_text(process)

app_table_path = Path("crates/monitor-gui/src/app_table.rs")
app_table = app_table_path.read_text()
app_table = replace_once(
    app_table,
    ".dropdown_menu(move |menu, _, _| {\n                    app_action_menu(",
    ".dropdown_menu(move |menu, _, cx| {\n                    let _ = menu_monitor.update(cx, |monitor, _| {\n                        monitor.hold_table_refresh();\n                    });\n                    app_action_menu(",
    "Apps dropdown hold",
)
app_table = replace_once(
    app_table,
    "fn open_app_signal_dialog(request: AppSignalRequest, window: &mut Window, cx: &mut App) {\n    window.open_dialog",
    "fn open_app_signal_dialog(request: AppSignalRequest, window: &mut Window, cx: &mut App) {\n    let _ = request.monitor.update(cx, |monitor, _| monitor.hold_table_refresh());\n    window.open_dialog",
    "Apps dialog hold",
)
app_table = replace_once(
    app_table,
    "    let _ = monitor.update(cx, |monitor, cx| {\n        monitor.signal_pids(&pids, signal);",
    "    let _ = monitor.update(cx, |monitor, cx| {\n        monitor.hold_table_refresh();\n        monitor.signal_pids(&pids, signal);",
    "Apps signal hold",
)
app_table = replace_once(
    app_table,
    "        _: &mut Context<TableState<Self>>,\n    ) -> PopupMenu {\n        let Some(group)",
    "        cx: &mut Context<TableState<Self>>,\n    ) -> PopupMenu {\n        let _ = self.monitor.update(cx, |monitor, _| {\n            monitor.hold_table_refresh();\n        });\n        let Some(group)",
    "Apps context hold",
)
app_table_path.write_text(app_table)

parity_path = Path("docs/better-monitor-resources-v1.10.2-parity.md")
parity = parity_path.read_text()
parity = re.sub(
    r"^\| Context menu \|.*$",
    "| Context menu | ✅ | Apps and Processes expose row menus; Processes also exposes actions for the explicit multi-PID selection set. |",
    parity,
    count=1,
    flags=re.MULTILINE,
)
parity = re.sub(
    r"^\| Single/multi-selection context menus \|.*$",
    "| Single/multi-selection context menus | ✅ | Row menus expose information, options, single-process actions, and batch actions for the selected PID set. |",
    parity,
    flags=re.MULTILINE,
)
parity = parity.replace(
    "| Temporary refresh hold during interaction | ⬜ | Missing. |",
    "| Temporary refresh hold during interaction | ✅ | Opening menus, changing selection, and invoking actions hold table row refresh for two seconds while collection continues. |",
)
parity_path.write_text(parity)

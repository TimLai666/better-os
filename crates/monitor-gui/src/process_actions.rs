use gpui::*;
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{PopupMenu, PopupMenuItem},
};
use sysinfo::{Pid, Signal};

use crate::{
    app::MonitorWindow, linux, process_table::ProcessInfo, settings::UnitBase,
};

#[derive(Clone)]
struct ProcessSignalRequest {
    monitor: WeakEntity<MonitorWindow>,
    pids: Vec<Pid>,
    signal: Signal,
    title: String,
    description: String,
    confirm_label: &'static str,
    action_id: usize,
    destructive: bool,
}

pub(crate) fn append_process_information(
    menu: PopupMenu,
    process: ProcessInfo,
    unit_base: UnitBase,
    row_ix: usize,
) -> PopupMenu {
    menu.item(
        PopupMenuItem::new("Process information").on_click(move |_, window, cx| {
            open_process_information_dialog(process.clone(), unit_base, row_ix, window, cx);
        }),
    )
}

pub(crate) fn append_process_signal_actions(
    menu: PopupMenu,
    monitor: WeakEntity<MonitorWindow>,
    process: ProcessInfo,
    selected_pids: Vec<Pid>,
    row_ix: usize,
) -> PopupMenu {
    let single_pid = process.pid;
    let single_name = process.name.clone();
    let end_monitor = monitor.clone();
    let force_monitor = monitor.clone();
    let pause_monitor = monitor.clone();
    let resume_monitor = monitor.clone();

    let mut menu = menu
        .separator()
        .item(PopupMenuItem::new("End process").on_click(move |_, window, cx| {
            open_process_signal_dialog(
                ProcessSignalRequest {
                    monitor: end_monitor.clone(),
                    pids: vec![single_pid],
                    signal: Signal::Term,
                    title: format!("End {single_name}?"),
                    description:
                        "The process will receive a graceful termination signal.".to_string(),
                    confirm_label: "End Process",
                    action_id: row_ix,
                    destructive: false,
                },
                window,
                cx,
            );
        }))
        .item(PopupMenuItem::new("Force stop").on_click(move |_, window, cx| {
            open_process_signal_dialog(
                ProcessSignalRequest {
                    monitor: force_monitor.clone(),
                    pids: vec![single_pid],
                    signal: Signal::Kill,
                    title: format!("Force stop {}?", process.name),
                    description:
                        "The process will stop immediately without cleanup.".to_string(),
                    confirm_label: "Force stop",
                    action_id: row_ix,
                    destructive: true,
                },
                window,
                cx,
            );
        }))
        .separator()
        .item(PopupMenuItem::new("Pause").on_click(move |_, _, cx| {
            send_process_signal(
                pause_monitor.clone(),
                vec![single_pid],
                Signal::Stop,
                cx,
            );
        }))
        .item(PopupMenuItem::new("Resume").on_click(move |_, _, cx| {
            send_process_signal(
                resume_monitor.clone(),
                vec![single_pid],
                Signal::Continue,
                cx,
            );
        }));

    let selected_is_only_current = selected_pids.len() == 1 && selected_pids[0] == single_pid;
    if !selected_pids.is_empty() && !selected_is_only_current {
        let count = selected_pids.len();
        let batch_end_monitor = monitor.clone();
        let batch_end_pids = selected_pids.clone();
        let batch_force_monitor = monitor.clone();
        let batch_force_pids = selected_pids.clone();
        let batch_pause_monitor = monitor.clone();
        let batch_pause_pids = selected_pids.clone();
        let batch_resume_monitor = monitor;

        menu = menu
            .separator()
            .item(
                PopupMenuItem::new(format!("End selected ({count})")).on_click(
                    move |_, window, cx| {
                        open_process_signal_dialog(
                            ProcessSignalRequest {
                                monitor: batch_end_monitor.clone(),
                                pids: batch_end_pids.clone(),
                                signal: Signal::Term,
                                title: format!("End {count} selected processes?"),
                                description: format!(
                                    "All {count} selected processes will receive a graceful termination signal."
                                ),
                                confirm_label: "End selected",
                                action_id: row_ix.saturating_add(10_000),
                                destructive: false,
                            },
                            window,
                            cx,
                        );
                    },
                ),
            )
            .item(
                PopupMenuItem::new(format!("Force stop selected ({count})")).on_click(
                    move |_, window, cx| {
                        open_process_signal_dialog(
                            ProcessSignalRequest {
                                monitor: batch_force_monitor.clone(),
                                pids: batch_force_pids.clone(),
                                signal: Signal::Kill,
                                title: format!("Force stop {count} selected processes?"),
                                description: format!(
                                    "All {count} selected processes will stop immediately without cleanup."
                                ),
                                confirm_label: "Force stop selected",
                                action_id: row_ix.saturating_add(20_000),
                                destructive: true,
                            },
                            window,
                            cx,
                        );
                    },
                ),
            )
            .item(
                PopupMenuItem::new(format!("Pause selected ({count})")).on_click(
                    move |_, _, cx| {
                        send_process_signal(
                            batch_pause_monitor.clone(),
                            batch_pause_pids.clone(),
                            Signal::Stop,
                            cx,
                        );
                    },
                ),
            )
            .item(
                PopupMenuItem::new(format!("Resume selected ({count})")).on_click(
                    move |_, _, cx| {
                        send_process_signal(
                            batch_resume_monitor.clone(),
                            selected_pids.clone(),
                            Signal::Continue,
                            cx,
                        );
                    },
                ),
            );
    }

    menu
}

fn open_process_information_dialog(
    process: ProcessInfo,
    unit_base: UnitBase,
    row_ix: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let title = format!("{} Information", process.name);
    let description = process_information(&process, unit_base);
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .child(description.clone())
            .footer(
                h_flex().justify_end().gap_2().child(
                    Button::new(("process-info-close", row_ix))
                        .outline()
                        .label("Close")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

fn open_process_signal_dialog(
    request: ProcessSignalRequest,
    window: &mut Window,
    cx: &mut App,
) {
    hold_table_refresh(request.monitor.clone(), cx);
    window.open_dialog(cx, move |dialog, _, _| {
        let action_request = request.clone();
        let click_request = action_request.clone();
        let confirm = Button::new(("process-signal-confirm", action_request.action_id))
            .label(action_request.confirm_label);
        let confirm = if action_request.destructive {
            confirm.warning()
        } else {
            confirm
        };
        dialog
            .title(request.title.clone())
            .child(request.description.clone())
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new(("process-signal-cancel", action_request.action_id))
                            .outline()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(confirm.on_click(move |_, window, cx| {
                        send_process_signal(
                            click_request.monitor.clone(),
                            click_request.pids.clone(),
                            click_request.signal,
                            cx,
                        );
                        window.close_dialog(cx);
                    })),
            )
    });
}

fn send_process_signal(
    monitor: WeakEntity<MonitorWindow>,
    pids: Vec<Pid>,
    signal: Signal,
    cx: &mut App,
) {
    let _ = monitor.update(cx, |monitor, cx| {
        monitor.hold_table_refresh();
        monitor.signal_pids(&pids, signal);
        cx.notify();
    });
}

fn hold_table_refresh(monitor: WeakEntity<MonitorWindow>, cx: &mut App) {
    let _ = monitor.update(cx, |monitor, _| monitor.hold_table_refresh());
}

fn process_information(process: &ProcessInfo, unit_base: UnitBase) -> String {
    format!(
        "PID: {}\nParent PID: {}\nUser: {}\nState: {}\nCPU: {:.1}%\nMemory: {}\nSwap: {}\nPriority: {}\nNice: {}\nThreads: {}\nOpen files: {}\nExecutable: {}\nWorking directory: {}\nApplication ID: {}\nControl group: {}\nCommand line: {}",
        process.pid,
        process
            .parent_pid
            .map_or_else(|| "N/A".to_string(), |pid| pid.to_string()),
        process.user,
        process.state,
        process.cpu_usage,
        linux::format_bytes(process.memory, unit_base),
        linux::format_bytes(process.swap, unit_base),
        process
            .priority
            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
        process
            .nice
            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
        process
            .threads
            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
        process
            .file_descriptors
            .map_or_else(|| "N/A".to_string(), |value| value.to_string()),
        process.executable.as_deref().unwrap_or("N/A"),
        process.working_directory.as_deref().unwrap_or("N/A"),
        process.app_id.as_deref().unwrap_or("N/A"),
        process.cgroup.as_deref().unwrap_or("N/A"),
        if process.command_line.is_empty() {
            "N/A"
        } else {
            &process.command_line
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_information_keeps_missing_values_explicit() {
        let process = ProcessInfo {
            pid: Pid::from_u32(42),
            parent_pid: None,
            name: "demo".to_string(),
            user: "user".to_string(),
            state: "Run".to_string(),
            cpu_usage: 12.5,
            memory: 1024,
            swap: 0,
            read_speed: 0,
            read_total: 0,
            write_speed: 0,
            write_total: 0,
            total_cpu_time_ticks: None,
            user_cpu_time_ticks: None,
            system_cpu_time_ticks: None,
            priority: None,
            nice: None,
            threads: None,
            file_descriptors: None,
            command_line: String::new(),
            executable: None,
            working_directory: None,
            cgroup: None,
            app_id: None,
        };
        let text = process_information(&process, UnitBase::Binary);
        assert!(text.contains("Parent PID: N/A"));
        assert!(text.contains("Executable: N/A"));
        assert!(text.contains("Command line: N/A"));
    }
}

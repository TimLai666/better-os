mod app;
mod charts;
mod i18n;
mod layout;
mod link;
mod pages;
mod popup;
mod render;
mod restart;
mod shell;
mod startup;
mod stored;
mod tables;
#[cfg(test)]
mod tests;

use app::MonitorApp;
use better_ui::command_line::{WindowCommandLine, WindowOption};
use gpui::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// What this window does with the arguments it was started with.
///
/// `/usr/bin/better-monitor` is the window and `better-monitor-cli` is the
/// command line, so `better-monitor inspect` is the command line's usage typed
/// at the window's name. It is refused with the name that would have answered
/// it rather than opening a window a person at a terminal cannot escape.
const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-monitor",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[WindowOption::Flag("--popup"), WindowOption::Valued("--at")],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-monitor：這是 Better Monitor 的視窗，不接受「{other}」這個參數。\n\
         指令列請改用 better-monitor-cli，例如 better-monitor-cli inspect。\n\
         better-monitor: this is the Better Monitor window and takes no argument \"{other}\".\n\
         For the command line use better-monitor-cli, e.g. better-monitor-cli inspect."
    )
}

const HELP: &str = "\
better-monitor — Better Monitor 的視窗 / the Better Monitor window

用法 / Usage:
  better-monitor              開啟視窗 / open the window
  better-monitor --version    顯示版本 / print the version
  better-monitor --help       顯示這段說明 / print this help
  better-monitor --popup [--at X,Y]
                              工作列的小視窗 / the taskbar popup

指令列是另一個程式 / The command line is a separate program:
  better-monitor-cli --help";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);

    let popup = arguments.iter().any(|argument| argument == "--popup");
    let at = arguments
        .iter()
        .position(|argument| argument == "--at")
        .and_then(|index| arguments.get(index + 1))
        .map(String::as_str)
        .or_else(|| {
            arguments
                .iter()
                .find_map(|argument| argument.strip_prefix("--at="))
        })
        .and_then(popup::parse_point);

    let app = gpui_platform::application().with_assets(Assets);

    if popup {
        app.run(move |cx| {
            gpui_component::init(cx);
            popup::open(at, cx);
        });
        return;
    }

    // The window is where a person looks for Better Monitor, so the taskbar
    // item comes up with it when it is not already running.
    tray::ensure_running();

    app.run(move |cx| {
        // `gpui_component::init` installs its light theme. The window applies
        // the stored appearance (dark when there is none) once it exists.
        gpui_component::init(cx);

        let handoff = better_ui::restart::take_handoff(app::APP_ID);
        let window_options = WindowOptions {
            window_bounds: better_ui::restart::window_bounds(handoff.as_ref())
                .or_else(|| Some(WindowBounds::centered(size(px(1320.0), px(860.0)), cx))),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options(app::APP_ID)
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| MonitorApp::new(window, cx, handoff.as_ref()));
                cx.new(|cx| better_ui::window_chrome::root(view, window, cx))
            })
            .expect("failed to open Better Monitor window");
        })
        .detach();
    });
}

/// Bringing the taskbar item up when the window opens, as Better Awake's
/// window does: the item starts from its autostart entry at login, once, and a
/// person who quit it, or whose login started it before the service could
/// answer, finds it again by opening the window.
mod tray {
    use std::process::{Command, Stdio};

    /// The program the taskbar item's autostart entry runs.
    pub(crate) const TRAY_PROGRAM: &str = "better-monitor-tray";

    /// The item's user unit, and where the package installs it.
    pub(crate) const TRAY_UNIT: &str = "better-monitor-tray.service";
    pub(crate) const TRAY_UNIT_PATH: &str = "/usr/lib/systemd/user/better-monitor-tray.service";

    /// Whether to start it, from whether its bus name has an owner. `None`
    /// means the bus could not be asked, and nothing is started.
    pub(crate) fn should_start(name_has_owner: Option<bool>) -> bool {
        name_has_owner == Some(false)
    }

    fn tray_is_running(bus: &zbus::blocking::fdo::DBusProxy<'_>) -> Option<bool> {
        let name = zbus::names::BusName::try_from(monitor_ipc::TRAY_BUS_NAME).ok()?;
        bus.name_has_owner(name).ok()
    }

    /// Whether a failed activation should be followed by starting the
    /// program directly. Only when the bus has no activation file for the
    /// name at all (a development build run from the source tree): an
    /// installed package always has one, and there the item must run under
    /// its unit, which Better Manager stops when Better Monitor is disabled.
    pub(crate) fn spawn_directly_after(error: &str) -> bool {
        error.contains("ServiceUnknown")
    }

    /// Start the taskbar item when none is running, off the window's thread.
    /// The bus starts it through `better-monitor-tray.service`.
    pub(crate) fn ensure_running() {
        std::thread::spawn(|| {
            let Ok(connection) = zbus::blocking::Connection::session() else {
                return;
            };
            let Ok(bus) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
                return;
            };
            if !should_start(tray_is_running(&bus)) {
                return;
            }
            // Better Monitor switched off in Better Manager stays off: the
            // window does not bring its taskbar item back.
            let on = crate::startup::component_on(
                std::path::Path::new(TRAY_UNIT_PATH).exists(),
                crate::startup::unit_enabled(
                    &crate::startup::RealSystemctl::from_environment(),
                    TRAY_UNIT,
                ),
            );
            if !on {
                return;
            }
            let Ok(name) = zbus::names::WellKnownName::try_from(monitor_ipc::TRAY_BUS_NAME) else {
                return;
            };
            let error = match bus.start_service_by_name(name, 0) {
                Ok(_) => return,
                Err(error) => error.to_string(),
            };
            if !spawn_directly_after(&error) {
                eprintln!("better-monitor: the taskbar item could not be started: {error}");
                return;
            }
            match Command::new(TRAY_PROGRAM)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(error) => {
                    eprintln!("better-monitor: the taskbar item could not be started: {error}")
                }
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_item_is_started_only_when_the_bus_says_none_is_running() {
            assert!(should_start(Some(false)));
            assert!(!should_start(Some(true)));
            assert!(!should_start(None));
        }

        #[test]
        fn only_a_missing_activation_file_falls_back_to_running_the_program() {
            assert!(spawn_directly_after(
                "org.freedesktop.DBus.Error.ServiceUnknown: The name is not activatable"
            ));
            assert!(!spawn_directly_after(
                "org.freedesktop.systemd1.UnitMasked: Unit better-monitor-tray.service is masked."
            ));
        }

        #[test]
        fn the_unit_and_the_activation_file_start_this_program() {
            let unit = include_str!("../../../packaging/monitor/better-monitor-tray.service");
            assert!(
                unit.lines()
                    .any(|line| line == format!("ExecStart=/usr/bin/{TRAY_PROGRAM}"))
            );
            let activation =
                include_str!("../../../packaging/monitor/org.betteros.MonitorTray1.service");
            assert!(
                activation
                    .lines()
                    .any(|line| line == format!("Name={}", monitor_ipc::TRAY_BUS_NAME))
            );
            assert!(
                activation
                    .lines()
                    .any(|line| line == "SystemdService=better-monitor-tray.service")
            );
        }
    }
}

#[cfg(test)]
mod invocation_tests {
    use super::COMMAND_LINE;
    use better_ui::command_line::Invocation;

    fn run(values: &[&str]) -> Invocation {
        let arguments: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        COMMAND_LINE.invocation(&arguments)
    }

    fn printed(invocation: Invocation) -> String {
        match invocation {
            Invocation::Print(text) => text,
            Invocation::OpenWindow => panic!("expected text, the window would open"),
            Invocation::Refuse(text) => panic!("expected text on stdout, got a refusal: {text}"),
        }
    }

    fn refused(invocation: Invocation) -> String {
        match invocation {
            Invocation::Refuse(text) => text,
            Invocation::OpenWindow => panic!("expected a refusal, the window would open"),
            Invocation::Print(text) => panic!("expected a refusal, got: {text}"),
        }
    }

    fn desktop_arguments(entry: &str) -> Vec<&str> {
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .expect("the desktop entry has an Exec line");
        exec.split_whitespace().skip(1).collect()
    }

    #[test]
    fn no_argument_and_the_desktop_entry_open_the_window() {
        assert_eq!(run(&[]), Invocation::OpenWindow);
        let entry = include_str!("../../../packaging/monitor/io.betteros.Monitor.desktop");
        assert_eq!(run(&desktop_arguments(entry)), Invocation::OpenWindow);
    }

    #[test]
    fn version_and_help_are_answered_without_a_window() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-monitor {}", env!("CARGO_PKG_VERSION"))
            );
        }
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("better-monitor-cli --help"), "{help}");
        }
    }

    #[test]
    fn the_popup_flags_open_a_window() {
        assert_eq!(run(&["--popup"]), Invocation::OpenWindow);
        assert_eq!(run(&["--popup", "--at", "1500,12"]), Invocation::OpenWindow);
        assert_eq!(run(&["--popup", "--at=1500,12"]), Invocation::OpenWindow);
    }

    /// `better-monitor inspect` is the command line's usage typed at the
    /// window's name, which is the mistake this refusal exists for.
    #[test]
    fn a_command_line_argument_is_refused_and_names_the_command_line() {
        for arguments in [&["inspect"][..], &["--offline", "inspect"], &["--bogus"]] {
            let refusal = refused(run(arguments));
            assert!(
                refusal.contains(&format!("「{}」", arguments[0])),
                "{refusal}"
            );
            assert!(refusal.contains("better-monitor-cli inspect"), "{refusal}");
        }
    }
}

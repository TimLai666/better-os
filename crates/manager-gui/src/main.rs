mod app;
mod attention;
mod components;
mod deep_link;
mod defaults_app;
mod defaults_model;
#[cfg(test)]
mod defaults_tests;
#[cfg(test)]
mod evidence_tests;
mod home;
mod i18n;
mod layout;
mod model;
mod navigation;
mod pages;
mod pages_defaults;
mod pages_flow;
mod pages_main;
mod pages_settings;
mod render;
mod restart;
mod runner_end;
mod search;
mod settings_page;
mod shell;
#[cfg(test)]
mod tests;

use app::ManagerApp;
use better_ui::command_line::WindowCommandLine;
use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use layout::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

/// The window's application id: the desktop entry's name without its suffix.
pub(crate) const APP_ID: &str = "io.betteros.Manager";

/// What this window does with the arguments it was started with.
///
/// `/usr/bin/better-manager` is the window, and the command line ships beside
/// it as `better-manager-cli` — the same split `better-monitor` already has.
/// Until ticket 49 this binary ignored every argument and opened a window
/// anyway, so `better-manager catalog status` looked like a command that hung:
/// it had run the window, which does not exit.
///
/// Only `--help` and `--version` are answered here, because those are the two
/// a person types at a window's name expecting an answer rather than a window.
/// `--page <page>` opens the control center on a page (spec section 1), and
/// `--component <id>` opens that component's details in Apps: it is how
/// Better Launcher's "Uninstall" brings a person to the place a component is
/// removed. Which page a value names is `deep_link`'s rule.
const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-manager",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[
        better_ui::command_line::WindowOption::Valued("--page"),
        better_ui::command_line::WindowOption::Valued("--component"),
    ],
    refusal,
};

/// The component `--component <id>` asks for, when the id is a valid one.
/// An id that is not a component's shape is ignored rather than refused: the
/// window still opens, on its usual page.
#[cfg(test)]
fn requested_component(arguments: &[String]) -> Option<better_core::ComponentId> {
    match deep_link::request(arguments) {
        Ok(Some(deep_link::Request::Component(id))) => Some(id),
        _ => None,
    }
}

fn refusal(other: &str) -> String {
    format!(
        "better-manager：這是元件管理器的視窗，不接受「{other}」這個參數。\n\
         指令列請改用 better-manager-cli，例如 better-manager-cli catalog status。\n\
         better-manager: this is the manager window and takes no argument \"{other}\".\n\
         For the command line use better-manager-cli, e.g. better-manager-cli catalog status."
    )
}

const HELP: &str = "\
better-manager — Better OS 控制中心 / the Better OS control center

用法 / Usage:
  better-manager              開啟視窗 / open the window
  better-manager --page PAGE  開啟這一頁 / open that page
  better-manager --component ID
                              開啟這個元件的頁面 / open that component's page
  better-manager --version    顯示版本 / print the version
  better-manager --help       顯示這段說明 / print this help

頁面 / Pages:
  home, system, touchpad, keep-awake, keep-awake/until, apps, updates,
  default-apps, fixes, settings

視窗已經開著時，會切換那個視窗並移到最前面，不會開第二個。
With a window already open, that window switches page and comes to the front;
no second window opens.

指令列是另一個程式 / The command line is a separate program:
  better-manager-cli --help";

/// The two-language note for a running window that did not answer.
const UNANSWERED: &str = "\
better-manager：已經開著的視窗沒有回應，所以沒有切換頁面。
better-manager: the window that is already open did not answer, so no page was shown.";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);
    let requested = match deep_link::request(&arguments) {
        Ok(requested) => requested,
        Err(refusal) => {
            eprintln!("{refusal}");
            std::process::exit(2);
        }
    };

    // A restart after an update reopens the window as it was, and what the
    // program was first started with no longer applies.
    let handoff = better_ui::restart::take_handoff(APP_ID);
    let requested = if handoff.is_some() { None } else { requested };

    // One window. Without a session bus there is no way to find another
    // window, and this one opens on its own. Right after a restart the bus
    // may not have noticed yet that the program this one replaced let its
    // name go, so a restart asks again for a moment before treating the name
    // as another window's.
    let mut attempts = if handoff.is_some() { 30 } else { 1 };
    let claimed = loop {
        attempts -= 1;
        let claimed = deep_link::bus::connect(None, deep_link::bus::FORWARD_TIMEOUT)
            .and_then(deep_link::bus::claim);
        match claimed {
            Ok(deep_link::bus::Claim::Running(_)) if attempts > 0 => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            other => break other,
        }
    };
    let bus_owner = match claimed {
        Ok(deep_link::bus::Claim::Owner(owner)) => Some(owner),
        Ok(deep_link::bus::Claim::Running(connection)) => {
            let request = requested.clone().unwrap_or(deep_link::Request::Raise);
            match deep_link::bus::forward(&connection, &request) {
                Ok(()) => std::process::exit(0),
                Err(_) => {
                    eprintln!("{UNANSWERED}");
                    std::process::exit(1);
                }
            }
        }
        Err(_) => None,
    };
    let bus_owner = std::cell::Cell::new(bus_owner);

    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        // gpui-component installs its light theme here. Better OS is
        // dark-first, so the stored appearance is applied once the window
        // exists and the saved settings have been read.
        gpui_component::init(cx);

        // `io.betteros.Manager` is the desktop entry's file name without its
        // suffix. The compositor matches a window to its entry by exactly that
        // string, so the two have to be changed together.
        let window_options = WindowOptions {
            window_bounds: better_ui::restart::window_bounds(handoff.as_ref())
                .or_else(|| Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx))),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..better_ui::window_chrome::window_options(APP_ID)
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| {
                    let mut app = ManagerApp::new(window, cx, bus_owner.take(), handoff.as_ref());
                    if let Some(request) = requested {
                        app.show_request(request, cx);
                    }
                    app
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open Better OS Manager window");
        })
        .detach();
    });
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

    #[test]
    fn a_component_is_asked_for_by_id_and_a_malformed_one_is_ignored() {
        let arguments = |values: &[&str]| -> Vec<String> {
            values.iter().map(|value| value.to_string()).collect()
        };
        assert!(matches!(
            run(&["--component", "better-files"]),
            Invocation::OpenWindow
        ));
        assert_eq!(
            super::requested_component(&arguments(&["--component", "better-files"]))
                .map(|id| id.to_string()),
            Some("better-files".to_string())
        );
        assert_eq!(super::requested_component(&arguments(&[])), None);
        assert_eq!(
            super::requested_component(&arguments(&["--component", "../etc"])),
            None
        );
        assert_eq!(
            super::requested_component(&arguments(&["--component=better-git"]))
                .map(|id| id.to_string()),
            Some("better-git".to_string())
        );
        assert_eq!(
            super::requested_component(&arguments(&["--component"])),
            None
        );
    }

    #[test]
    fn no_argument_opens_the_window() {
        assert!(matches!(run(&[]), Invocation::OpenWindow));
    }

    /// The desktop entry is how almost everyone starts the window, so its own
    /// command line must never be the thing that is refused.
    #[test]
    fn the_desktop_entry_command_opens_the_window() {
        let entry = include_str!("../../../packaging/manager/io.betteros.Manager.desktop");
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .expect("the desktop entry has an Exec line");
        let arguments: Vec<&str> = exec.split_whitespace().skip(1).collect();
        assert!(matches!(run(&arguments), Invocation::OpenWindow), "{exec}");
    }

    /// A second launch exits without a window, so a startup notification
    /// started for it would be left spinning; the entry asks for none.
    #[test]
    fn the_desktop_entry_asks_for_no_startup_notification() {
        let entry = include_str!("../../../packaging/manager/io.betteros.Manager.desktop");
        let value = entry
            .lines()
            .find_map(|line| line.strip_prefix("StartupNotify="))
            .expect("the entry states StartupNotify");
        assert_eq!(value, "false");
    }

    #[test]
    fn version_names_the_window_and_the_workspace_version() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                printed(run(&[flag])),
                format!("better-manager {}", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn help_points_at_the_command_line() {
        for flag in ["--help", "-h"] {
            let help = printed(run(&[flag]));
            assert!(help.contains("better-manager-cli --help"), "{help}");
        }
    }

    /// Every page name `--page` takes is in `--help`, and each one opens the
    /// window rather than being refused as an unknown argument.
    #[test]
    fn help_lists_every_page_and_each_page_opens_the_window() {
        let help = printed(run(&["--help"]));
        assert!(help.contains("--page PAGE"), "{help}");
        for name in crate::deep_link::PAGE_NAMES {
            assert!(help.contains(name), "{help} lists {name}");
            assert!(matches!(run(&["--page", name]), Invocation::OpenWindow));
            assert!(matches!(
                run(&[&format!("--page={name}")]),
                Invocation::OpenWindow
            ));
            let arguments = vec!["--page".to_string(), name.to_string()];
            assert!(crate::deep_link::request(&arguments).is_ok(), "{name}");
        }
    }

    /// An unknown page passes the argument check (`--page` is an option) and
    /// is then refused by name, in both languages, before any window or bus.
    #[test]
    fn an_unknown_page_is_refused_by_name() {
        let arguments = vec!["--page".to_string(), "bogus".to_string()];
        let refusal = crate::deep_link::request(&arguments).expect_err("refused");
        assert!(
            refusal.contains("better-manager：沒有「bogus」這一頁"),
            "{refusal}"
        );
        assert!(
            refusal.contains("better-manager: there is no page \"bogus\""),
            "{refusal}"
        );
    }

    #[test]
    fn a_command_line_argument_is_refused_and_redirected() {
        for arguments in [
            &["catalog", "status"][..],
            &["--bogus"],
            &["list"],
            &["reconcile", "--adopt", "better-awake"],
        ] {
            let refusal = refused(run(arguments));
            assert!(
                refusal.contains(&format!("「{}」", arguments[0])),
                "{refusal}"
            );
            assert!(
                refusal.contains("better-manager-cli catalog status"),
                "{refusal}"
            );
        }
    }

    #[test]
    fn the_first_argument_decides() {
        assert!(matches!(run(&["--help", "catalog"]), Invocation::Print(_)));
        assert!(matches!(run(&["catalog", "--help"]), Invocation::Refuse(_)));
    }
}

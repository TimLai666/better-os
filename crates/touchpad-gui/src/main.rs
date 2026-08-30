//! Better Touchpad.
//!
//! ```text
//! better-touchpad [--lang zh-TW|en-US] [--offline]
//! better-touchpad --safe-mode      # disable Better Touchpad integration
//! better-touchpad --normal-mode    # enable it again
//! ```
//!
//! `--safe-mode` is the recovery entry point Issue #3 requires. It writes one
//! marker file and exits without opening a window or touching a setting, so it
//! works from a text console when the desktop has become hard to use — and it
//! works even when the configuration itself is the thing that is broken.

use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};
use gpui_component_assets::Assets;
use touchpad_core::TouchpadStore;
use touchpad_gui::{
    Locale, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, Page, Startup, StartupOptions, TouchpadApp,
};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    if arguments.iter().any(|argument| argument == "--safe-mode") {
        return match TouchpadStore::for_user().enable_safe_mode() {
            Ok(()) => println!(
                "Better Touchpad integration is off. Run `better-touchpad --normal-mode` to turn it back on."
            ),
            Err(error) => {
                eprintln!("could not turn safe mode on: {error}");
                std::process::exit(1);
            }
        };
    }
    if arguments.iter().any(|argument| argument == "--normal-mode") {
        return match TouchpadStore::for_user().clear_safe_mode() {
            Ok(()) => println!("Better Touchpad integration is on."),
            Err(error) => {
                eprintln!("could not turn safe mode off: {error}");
                std::process::exit(1);
            }
        };
    }

    let options = StartupOptions {
        locale: locale_from(&arguments),
        // The headless smoke test opens the window with no session bus, so it
        // must be able to say "do not go looking for one".
        offline: arguments.iter().any(|argument| argument == "--offline"),
        page: page_from(&arguments),
        ..StartupOptions::default()
    };

    // Startup time is a published figure, so it is measurable from the shipped
    // binary rather than only from a test. It prints nothing unless asked, so
    // the headless launch smoke still expects silence.
    let launched = std::time::Instant::now();
    let trace = std::env::var("BETTER_TOUCHPAD_TRACE_STARTUP").as_deref() == Ok("1");

    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1040.0), px(720.0)), cx)),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                // Better OS is dark-first; gpui-component installs its light
                // theme at init.
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let assembled = std::time::Instant::now();
                let startup = Startup::run(options);
                let read_in = assembled.elapsed();
                let app = cx.new(|cx| TouchpadApp::new(startup, window, cx));
                let root = cx.new(|cx| Root::new(app, window, cx));
                if trace {
                    eprintln!(
                        "better-touchpad: window ready in {:?} ({read_in:?} reading the desktop)",
                        launched.elapsed()
                    );
                }
                root
            })
            .expect("failed to open the Better Touchpad window");
        })
        .detach();
    });
}

/// `--page gestures` opens on that screen. The headless launch smoke uses it,
/// so "the Gestures screen renders" is something a command proves rather than
/// something a reviewer assumes.
fn page_from(arguments: &[String]) -> Page {
    let wanted = arguments.iter().enumerate().find_map(|(index, argument)| {
        if argument == "--page" {
            return arguments.get(index + 1).cloned();
        }
        argument.strip_prefix("--page=").map(str::to_string)
    });
    wanted
        .as_deref()
        .and_then(Page::parse)
        .unwrap_or(Page::Overview)
}

fn locale_from(arguments: &[String]) -> Locale {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--lang" {
            return match arguments.next().map(String::as_str) {
                Some(language) => Locale::from_language(language),
                None => Locale::System,
            };
        }
        if let Some(language) = argument.strip_prefix("--lang=") {
            return Locale::from_language(language);
        }
    }
    Locale::System
}

//! `better-screenshot`: Print Screen in the Better OS session
//! (engineering/specs/screenshot.md). One process at a time: the first owns
//! the bus names and draws the interface; a later start hands its request to
//! it and exits.

use better_ui::command_line::Invocation;
use screenshot_core::session::Request;
use screenshot_gui::cli::COMMAND_LINE;
use screenshot_gui::service::{self, Claim};
use screenshot_gui::{app, assets};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match COMMAND_LINE.invocation(&arguments) {
        Invocation::OpenWindow => {}
        Invocation::Print(text) => {
            println!("{text}");
            return;
        }
        Invocation::Refuse(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
    }
    let request = Request::from_arguments(&arguments);

    let (service, messages) = match service::claim(request) {
        Ok(Claim::Owned(service, messages)) => (service, messages),
        Ok(Claim::HandedOver) => return,
        Err(error) => {
            eprintln!("better-screenshot: no session bus: {error}");
            std::process::exit(1);
        }
    };

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            // The process outlives its surfaces: it holds the clipboard and
            // the recording with none open.
            cx.set_quit_mode(gpui::QuitMode::Explicit);
            let shooter = app::start(service, messages, request, cx);
            // Kept for the life of the application.
            std::mem::forget(shooter);
        });
}

//! `better-files`, the Better OS file manager.

use files_gui::launch::{HostProbe, Invocation, invocation, plan_start};

fn main() {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let request = match invocation(&arguments) {
        Invocation::Open(request) => request,
        Invocation::Print(text) => {
            println!("{text}");
            return;
        }
        Invocation::Refuse(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
    };
    if let Err(reason) = files_gui::refuse_root() {
        eprintln!("better-files: {reason}");
        std::process::exit(1);
    }
    let cwd = std::env::current_dir().ok();
    let plan = plan_start(&request, cwd.as_deref(), &HostProbe);
    // The window says the same thing; this line is for a person who started
    // it from a terminal and is looking there.
    if plan.needs_notice() {
        let notice = files_gui::session::Notice::Start {
            problem: plan.problem,
            ignored: plan.ignored,
        };
        let c = files_gui::i18n::copy(files_gui::i18n::Locale::System);
        eprintln!("better-files: {}", notice.message(c));
    }
    files_gui::run(plan);
}

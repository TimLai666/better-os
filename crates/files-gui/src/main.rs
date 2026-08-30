//! `better-files`, the Better OS file manager.

fn main() {
    if let Err(reason) = files_gui::refuse_root() {
        eprintln!("better-files: {reason}");
        std::process::exit(1);
    }
    files_gui::run();
}

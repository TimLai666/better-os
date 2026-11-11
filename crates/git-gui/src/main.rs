//! `better-git`, the Better Git window.

use git_gui::{Start, invocation};

fn main() {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match invocation(&arguments) {
        Start::Open(options) => git_gui::run(options),
        Start::Print(text) => println!("{text}"),
        Start::Refuse(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
    }
}

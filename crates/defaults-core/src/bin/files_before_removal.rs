//! `better-files-before-removal`: run by Better Manager, as the person,
//! right before the better-files package is removed
//! (`/usr/lib/better-os/before-removal/better-files`) and when Better Files
//! is turned off (`/usr/lib/better-os/on-disable/better-files`). It gives
//! back the defaults Better Files took over (the file manager the session
//! bus starts, the folder handler) through the same Restore the Default apps
//! page runs; with `--undo` (the removal was refused, or Better Files was
//! turned on again) it takes exactly those again. See
//! `defaults_core::removal` and ADR 0041.
//!
//! Exit status: 0 done (or nothing to do), 3 not done (the removal goes on
//! and says so), 4 no session (Better Manager finishes it at its next start
//! in the person's session), 2 a wrong argument.

use std::process::ExitCode;

use defaults_core::removal::{PutBack, better_files_program};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let undo = match arguments.as_slice() {
        [] => false,
        [only] if only == "--undo" => true,
        _ => {
            eprintln!("better-files-before-removal takes no argument but --undo");
            return ExitCode::from(2);
        }
    };
    match better_files_program(undo) {
        PutBack::Done => ExitCode::SUCCESS,
        PutBack::NotRestored(reason) => {
            eprintln!("{reason}");
            ExitCode::from(3)
        }
        PutBack::Deferred => {
            eprintln!(
                "no session to put the defaults back in; Better Manager finishes it at its next start"
            );
            ExitCode::from(4)
        }
    }
}

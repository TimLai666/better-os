//! `better-touchpad-before-removal`: run by Better Manager, as the person,
//! right before the better-touchpad package is removed, and with `--undo`
//! when that removal was then refused. See `touchpad_platform::removal`.
//!
//! Exit status: 0 put back (or nothing to put back), 3 could not put back
//! (the removal goes on and says so), 4 no session, so a pending restore was
//! left for Better Manager's next start, 2 a wrong argument.

use std::process::ExitCode;

use touchpad_core::{RunState, TouchpadStore};
use touchpad_platform::GnomeBackend;
use touchpad_platform::removal::{
    PutBack, put_back_before_removal, refused_before_removal, undo_before_removal, user_state_home,
};

const NOT_RESTORED: u8 = 3;
const DEFERRED: u8 = 4;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let undo = match arguments.as_slice() {
        [] => false,
        [only] if only == "--undo" => true,
        _ => {
            eprintln!("better-touchpad-before-removal takes no argument but --undo");
            return ExitCode::from(2);
        }
    };
    let store = TouchpadStore::new(TouchpadStore::user_directory());
    let mut backend = GnomeBackend::connect(None);
    let reachable = backend.status().reachable;
    let state_home = user_state_home();

    // A session whose settings database could not be decided is not a
    // missing session: waiting for the next start would wait forever, so the
    // removal goes on and Activity says why, in the person's words.
    if let Some(said) = refused_before_removal(&store, &backend) {
        eprintln!("{said}");
        return ExitCode::from(NOT_RESTORED);
    }

    if undo {
        return match undo_before_removal(&store, &mut backend, &state_home) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(NOT_RESTORED)
            }
        };
    }
    match put_back_before_removal(&store, &mut backend, reachable, &state_home) {
        PutBack::NothingChanged => ExitCode::SUCCESS,
        PutBack::Restored(RunState::Failed) => {
            eprintln!("the desktop did not take the touchpad settings back");
            ExitCode::from(NOT_RESTORED)
        }
        PutBack::Restored(_) => ExitCode::SUCCESS,
        PutBack::Unreadable(reason) => {
            eprintln!("{reason}");
            ExitCode::from(NOT_RESTORED)
        }
        PutBack::Deferred => {
            eprintln!("no session to write in; put back at Better Manager's next start");
            ExitCode::from(DEFERRED)
        }
    }
}

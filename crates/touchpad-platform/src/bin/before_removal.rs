//! `better-touchpad-before-removal`: run by Better Manager, as the person,
//! before the better-touchpad package is removed. It writes every captured
//! touchpad setting back (see `touchpad_platform::removal`) and exits 0 when
//! that worked or there was nothing to put back, and 1 otherwise, saying why.

use std::process::ExitCode;

use touchpad_core::{RunState, TouchpadStore};
use touchpad_platform::GnomeBackend;
use touchpad_platform::removal::{PutBack, put_back_before_removal};

fn main() -> ExitCode {
    if std::env::args_os().len() > 1 {
        eprintln!("better-touchpad-before-removal takes no arguments");
        return ExitCode::from(2);
    }
    let store = TouchpadStore::new(TouchpadStore::user_directory());
    let mut backend = GnomeBackend::connect(None);
    match put_back_before_removal(&store, &mut backend) {
        Ok(PutBack::NothingChanged) => ExitCode::SUCCESS,
        Ok(PutBack::Restored(RunState::Failed)) => {
            eprintln!("the touchpad settings could not be put back");
            ExitCode::FAILURE
        }
        Ok(PutBack::Restored(state)) => {
            eprintln!("the touchpad settings were put back ({state:?})");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("the touchpad capture could not be read: {error}");
            ExitCode::FAILURE
        }
    }
}

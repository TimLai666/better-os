//! `better-manager-before-removal`: run as each person right before the
//! better-manager package is removed, and with `--undo` when that removal is
//! abandoned. See `patch_platform::removal`.
//!
//! Exit status: 0 every applied fix undone (or none was applied), 3 not all
//! of it (a key the person changed afterwards is left and named, or
//! something could not be read or written; the removal goes on and says so),
//! 2 a wrong argument. It never exits 4: it needs no session, only the
//! person's files.

use std::process::ExitCode;

use patch_platform::removal::{Removal, before_removal, undo_before_removal};
use patch_platform::{LinuxHost, Paths};

const NOT_RESTORED: u8 = 3;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let undo = match arguments.as_slice() {
        [] => false,
        [only] if only == "--undo" => true,
        _ => {
            eprintln!("better-manager-before-removal takes no argument but --undo");
            return ExitCode::from(2);
        }
    };
    let (Some(mut host), Some(paths)) = (LinuxHost::for_user(), Paths::for_user()) else {
        eprintln!("no home directory: the fixes Better Manager applied were not undone");
        return ExitCode::from(NOT_RESTORED);
    };
    if undo {
        return match undo_before_removal(&mut host, &paths) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(NOT_RESTORED)
            }
        };
    }
    match before_removal(&mut host, &paths) {
        Removal::NothingApplied | Removal::Undone => ExitCode::SUCCESS,
        Removal::LeftAsChanged(keys) => {
            for key in keys {
                eprintln!("left as you changed it after the fix: {key}");
            }
            ExitCode::from(NOT_RESTORED)
        }
        Removal::Failed(reason) => {
            eprintln!("not every fix could be undone: {reason}");
            ExitCode::from(NOT_RESTORED)
        }
    }
}

//! `better-manager-before-removal`: run as each person by the
//! better-manager package's prerm right before the package is removed, and
//! with `--undo` by its postinst when that removal is abandoned. See
//! `patch_platform::removal`.
//!
//! It finds the person's files from `HOME` and, when the record or the
//! overrides are kept somewhere else, the location file the window left
//! (`patch_platform::paths::for_removal`). The dpkg run's stamp comes in
//! `BETTER_OS_REMOVAL_RUN`.
//!
//! Exit status: 0 every applied fix undone (or none was applied), 3 not all
//! of it (a key the person changed afterwards is left, a fix whose undo
//! failed stays recorded, or nothing could be read; each is named on
//! standard error and the removal goes on), 2 a wrong argument. It never
//! exits 4: it needs no session, only the person's files.

use std::path::PathBuf;
use std::process::ExitCode;

use patch_platform::LinuxHost;
use patch_platform::command::SystemCommands;
use patch_platform::display::NoDisplayService;
use patch_platform::paths::for_removal;
use patch_platform::removal::{RUN_VARIABLE, Removal, before_removal, undo_before_removal};

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
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
    else {
        eprintln!("no home directory: the fixes Better Manager applied were not undone");
        return ExitCode::from(NOT_RESTORED);
    };
    let run = std::env::var(RUN_VARIABLE)
        .ok()
        .filter(|run| !run.is_empty());
    let (paths, warning) = for_removal(&home);
    if let Some(warning) = &warning {
        eprintln!("{warning}");
    }
    // Undoing reads and writes files only: no command, no display.
    let mut host = LinuxHost::new(
        paths.clone(),
        Box::new(SystemCommands),
        Box::new(NoDisplayService),
        None,
    );
    if undo {
        return match undo_before_removal(&mut host, &home, run.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(NOT_RESTORED)
            }
        };
    }
    let status = match before_removal(&mut host, &paths, &home, run.as_deref()) {
        Removal::NothingApplied | Removal::Undone => ExitCode::SUCCESS,
        Removal::Partly(lines) => {
            for line in lines {
                eprintln!("{line}");
            }
            ExitCode::from(NOT_RESTORED)
        }
        Removal::Failed(reason) => {
            eprintln!("the fixes Better Manager applied were not undone: {reason}");
            ExitCode::from(NOT_RESTORED)
        }
    };
    if warning.is_some() {
        return ExitCode::from(NOT_RESTORED);
    }
    status
}

//! `better-files-before-removal`: run by Better Manager, as the person,
//! right before the better-files package is removed. It restores the
//! defaults Better Files took over (the file manager the session bus starts,
//! the folder handler) to what they were before, through the same Restore
//! the Default apps page runs. See `defaults_core::removal` and ADR 0041.
//!
//! Exit status: 0 put back (or nothing to put back), 3 could not put back
//! (the removal goes on and says so), 2 a wrong argument. `--undo` (the
//! removal was refused after this ran) does nothing: the defaults stay as
//! they were before Better Files, and "Use Better OS defaults" applies them
//! again.

use std::process::ExitCode;

use better_core::ComponentManifest;
use defaults_core::adapters::{AdapterMode, AdapterSession, desktop_session};
use defaults_core::status::SystemContext;
use defaults_store::SnapshotStore;

const NOT_RESTORED: u8 = 3;
const MANIFEST: &str = include_str!("../../../../components/manifests/better-files.yaml");

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => {}
        [only] if only == "--undo" => return ExitCode::SUCCESS,
        _ => {
            eprintln!("better-files-before-removal takes no argument but --undo");
            return ExitCode::from(2);
        }
    }
    let manifest = match ComponentManifest::parse_yaml(MANIFEST) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("the built-in Better Files manifest does not read: {error}");
            return ExitCode::from(NOT_RESTORED);
        }
    };
    let distribution = better_core::host::read_os_release()
        .map(|content| better_core::host::resolve_distribution_id(&content))
        .unwrap_or_default();
    let mut session = match AdapterSession::open(&AdapterMode::Production) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(NOT_RESTORED);
        }
    };
    let outcome = defaults_core::removal::restore_component(
        manifest,
        SystemContext::new(distribution, desktop_session()),
        session.adapters_mut(),
        &SnapshotStore::from_default_path(),
    );
    match outcome {
        Ok(outcome) if !outcome.has_failures() => ExitCode::SUCCESS,
        Ok(_) => {
            eprintln!("a default Better Files set could not be put back");
            ExitCode::from(NOT_RESTORED)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(NOT_RESTORED)
        }
    }
}

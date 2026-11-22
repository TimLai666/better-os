//! `better-os-login-status`: what the login supervisor recorded this
//! startup, for a person at the text login (design/login-screen-spec.md
//! section 5.10). Reads only.

use std::path::Path;

use better_core::local_time::{TimeZone, calendar_time};
use login_supervisor::bus::{self, LastRecord};
use login_supervisor::read::paths;
use login_supervisor::text::{Language, status};

fn main() {
    if let Some(argument) = std::env::args().nth(1) {
        if argument == "--help" {
            println!("better-os-login-status: prints what the Better OS login supervisor recorded");
            return;
        }
        eprintln!("better-os-login-status takes no arguments");
        std::process::exit(2);
    }
    let (record, from_files) = match bus::ask() {
        Ok(record) => (record, false),
        Err(_) => (
            LastRecord::read(
                Path::new(paths::STATE),
                &manager_ipc::boot_watch::Records::system(),
            ),
            true,
        ),
    };
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()));
    let language = Language::pick(std::env::var("TERM").ok().as_deref(), locale.as_deref());
    let date = |at: u64| calendar_time(at, TimeZone::System);
    for line in status(
        language,
        record.state.as_ref(),
        record.event.as_ref(),
        from_files,
        &date,
    ) {
        println!("{line}");
    }
}

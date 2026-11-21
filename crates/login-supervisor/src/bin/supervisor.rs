//! `better-os-login-supervisor`: the display manager's process. Its unit is
//! `better-os-login-supervisor.service` (`Type=notify`, `WatchdogSec=10`).

use std::path::{Path, PathBuf};

use login_supervisor::read::paths;
use login_supervisor::rules::limits;
use login_supervisor::system::SystemWorld;
use login_supervisor::world::{Supervisor, World as _};

/// Ticket 281's crash loop: abort on reaching this stage (its journal
/// code, `6` for the compositor). Read from the unit's environment, which
/// only root sets.
const ABORT_HOOK: &str = "BETTER_OS_TEST_SUPERVISOR_ABORT";

fn main() {
    if std::env::args().nth(1).is_some() {
        eprintln!("better-os-login-supervisor takes no arguments; it is started by systemd");
        std::process::exit(2);
    }
    let abort_at = std::env::var(ABORT_HOOK).ok().filter(|v| !v.is_empty());
    let mut world = SystemWorld::new();
    let loaded = login_supervisor::state::load(Path::new(paths::STATE));
    let boot_id = world.boot_id();
    let unit = world.unit_start();
    login_supervisor::bus::serve_in_background(
        PathBuf::from(paths::STATE),
        manager_ipc::boot_watch::Records::system(),
    );
    let mut supervisor = Supervisor::start(&mut world, loaded, boot_id, unit);
    loop {
        supervisor.tick(&mut world);
        if let Some(code) = &abort_at
            && supervisor
                .state()
                .current_step()
                .is_some_and(|step| step.code() == code)
        {
            world.log(&format!("{ABORT_HOOK}={code}: aborting as asked"));
            std::process::abort();
        }
        std::thread::sleep(std::time::Duration::from_micros(limits::TICK));
    }
}

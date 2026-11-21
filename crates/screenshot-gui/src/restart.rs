//! What Better Screenshot carries over when an update restarts it (AGENTS.md,
//! ticket 105), with no GPUI in it.
//!
//! The interface shows a frozen picture of the screen that a new program
//! could not have, and a recording, a picture being written, and a pending
//! portal call are work in flight; each of them makes the restart wait
//! until it ends. Between them the process holds nothing but the clipboard
//! and its notifications, which a restart gives up: the program that
//! replaced it starts clean, as a new Print Screen would. The view carried
//! over is only what the state file already keeps, so it is empty.

use better_ui::restart::Blocker;
use screenshot_core::session::Holds;
use serde::{Deserialize, Serialize};

/// Everything the window puts back: nothing beyond the state file.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotView {}

/// Why a restart must wait now, if it must.
pub fn restart_blocked(holds: &Holds) -> Option<Blocker> {
    if holds.recording || holds.saving || holds.portal_pending {
        Some(Blocker::Task)
    } else if holds.interface_open {
        Some(Blocker::Dialog)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{Blocker, Holds, restart_blocked};

    #[test]
    fn work_in_flight_and_the_open_interface_hold_a_restart() {
        assert_eq!(restart_blocked(&Holds::default()), None);
        let recording = Holds {
            recording: true,
            ..Holds::default()
        };
        assert_eq!(restart_blocked(&recording), Some(Blocker::Task));
        let portal = Holds {
            portal_pending: true,
            interface_open: true,
            ..Holds::default()
        };
        assert_eq!(restart_blocked(&portal), Some(Blocker::Task));
        let open = Holds {
            interface_open: true,
            ..Holds::default()
        };
        assert_eq!(restart_blocked(&open), Some(Blocker::Dialog));
        // The clipboard and notifications do not hold a restart.
        let idle = Holds {
            clipboard: true,
            notifications: true,
            ..Holds::default()
        };
        assert_eq!(restart_blocked(&idle), None);
    }
}

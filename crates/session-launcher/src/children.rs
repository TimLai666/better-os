//! The programs the launcher runs as its own children, and how it ends them.
//!
//! Almost everything in the session is a systemd user unit of
//! `better-os-session.target`. A program that finds its logind session by its
//! own process id cannot be one, because a user unit is outside the session's
//! scope: `cosmic-greeter`, the locker, exits at once as a unit
//! (`org.freedesktop.login1.NoSessionForPID`, ticket 201) and answers logind's
//! `Lock` when it runs here. So it is the launcher's child, started after the
//! environment is in place and started again when it ends, so the session
//! never stays without a locker.
//!
//! The launcher ends what it started rather than leaving it to logind: ticket
//! 201 saw a session scope stay in `stop-sigterm` for 34 s once, with
//! `cosmic-settings-daemon` alive, after the compositor ended. Each child gets
//! SIGTERM, then SIGKILL after [`TERM_PATIENCE`]; the compositor last, so
//! every client can still say goodbye to it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// One program the launcher runs inside the session's scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChildSpec {
    /// The name used in the journal and in the resume state.
    pub name: &'static str,
    pub program: &'static str,
    pub arguments: &'static [&'static str],
}

/// The locker: `cosmic-greeter` in a user session (ticket 201).
pub const LOCKER: ChildSpec = ChildSpec {
    name: "locker",
    program: "/usr/bin/cosmic-greeter",
    arguments: &[],
};

/// Every child, in the order it is started; they end in the reverse order.
pub const CHILDREN: [ChildSpec; 1] = [LOCKER];

/// The child with this name.
pub fn by_name(name: &str) -> Option<ChildSpec> {
    CHILDREN.into_iter().find(|child| child.name == name)
}

/// How long a child, and then the compositor, has after SIGTERM.
pub const TERM_PATIENCE: Duration = Duration::from_secs(5);

/// How many ends within [`RESTART_WINDOW`] a child is started again after.
pub const RESTART_LIMIT: usize = 5;

/// The window [`RESTART_LIMIT`] counts in.
pub const RESTART_WINDOW: Duration = Duration::from_secs(60);

/// How long the launcher waits before it starts an ended child again.
pub const RESTART_DELAY: Duration = Duration::from_secs(1);

/// What to do about a child that ended while the session runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AfterExit {
    /// Start it again after this long.
    Restart(Duration),
    /// It ended too often; leave it ended and say so.
    GiveUp,
}

/// The ends of one child, for the restart rule.
#[derive(Clone, Debug, Default)]
pub struct Restarts {
    ends: VecDeque<Instant>,
}

impl Restarts {
    /// Records an end at `now` and decides.
    pub fn ended(&mut self, now: Instant) -> AfterExit {
        while let Some(&oldest) = self.ends.front() {
            if now.saturating_duration_since(oldest) >= RESTART_WINDOW {
                self.ends.pop_front();
            } else {
                break;
            }
        }
        self.ends.push_back(now);
        if self.ends.len() > RESTART_LIMIT {
            AfterExit::GiveUp
        } else {
            AfterExit::Restart(RESTART_DELAY)
        }
    }
}

/// What the end of the session sends next, given how long ago the SIGTERM
/// went.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ending {
    Terminate,
    Wait,
    Kill,
}

/// SIGTERM first; SIGKILL once [`TERM_PATIENCE`] has passed since it.
pub fn ending(terminated_at: Option<Instant>, now: Instant) -> Ending {
    match terminated_at {
        None => Ending::Terminate,
        Some(at) if now.saturating_duration_since(at) >= TERM_PATIENCE => Ending::Kill,
        Some(_) => Ending::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locker_is_a_child_and_not_a_unit() {
        assert_eq!(by_name("locker"), Some(LOCKER));
        assert_eq!(LOCKER.program, "/usr/bin/cosmic-greeter");
        assert_eq!(by_name("nobody"), None);
    }

    #[test]
    fn a_child_is_started_again_until_it_ends_too_often() {
        let start = Instant::now();
        let mut restarts = Restarts::default();
        for second in 0..RESTART_LIMIT as u64 {
            assert_eq!(
                restarts.ended(start + Duration::from_secs(second)),
                AfterExit::Restart(RESTART_DELAY)
            );
        }
        assert_eq!(
            restarts.ended(start + Duration::from_secs(10)),
            AfterExit::GiveUp
        );
    }

    #[test]
    fn ends_older_than_the_window_are_forgotten() {
        let start = Instant::now();
        let mut restarts = Restarts::default();
        for minute in 0..20u64 {
            assert_eq!(
                restarts.ended(start + Duration::from_secs(minute * 61)),
                AfterExit::Restart(RESTART_DELAY)
            );
        }
    }

    #[test]
    fn ending_is_terminate_then_wait_then_kill() {
        let now = Instant::now();
        assert_eq!(ending(None, now), Ending::Terminate);
        assert_eq!(
            ending(Some(now), now + Duration::from_secs(1)),
            Ending::Wait
        );
        assert_eq!(ending(Some(now), now + TERM_PATIENCE), Ending::Kill);
    }
}

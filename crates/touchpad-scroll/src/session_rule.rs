//! Who may change the factors.
//!
//! The factor is a property of the machine's touchpad, so the person sitting
//! at it may change it and nobody else may. The decision is made per calling
//! *session*, not per user: the service asks logind which session the calling
//! process belongs to (`GetSessionByPID`), and that session must be local and
//! the active one on its seat. The same person's ssh login, or a second
//! session switched away from, is refused even though the uid matches.
//!
//! A program started from the desktop's application grid runs under the
//! user's service manager, outside every login session, and logind answers
//! "no session" for it. Only for such a process, the user's display session
//! stands in, which is what polkit does for the same case: it must still be
//! local and active, and the user must have no remote session open at the
//! time of the call.
//!
//! **What this is not:** a boundary between processes of the same uid. A
//! process in no session can be the window, but it can as well be the same
//! person's cron job, a `systemd-run --user` unit, or something an ssh login
//! started under the user's service manager; logind cannot tell them apart.
//! The remote-session refusal closes the ssh case while that login is open,
//! and nothing closes the rest. That is accepted because a process running as
//! the person can already change everything the person's desktop can (their
//! settings, their files, their session's input), of which a scroll factor
//! between 0.2 and 5 is the least; the rule exists to keep *other* accounts
//! and remote logins out, not to tell one of the person's programs from
//! another.
//!
//! polkit itself is not used because the service has dropped to an
//! unprivileged account by the time a call arrives, and polkit 0.105 (Ubuntu
//! 22.04) only answers such a mechanism about its own processes.

/// What logind said about the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFacts {
    pub uid: u32,
    /// The session the calling process is in, when it is in one.
    pub caller_session: Option<SessionState>,
    /// The user's display session, consulted only when the caller is in no
    /// session at all.
    pub display_session: Option<SessionState>,
    /// Whether the user has any remote session open (not closing) right now.
    pub remote_session_open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionState {
    pub active: bool,
    pub remote: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Allowed,
    Refused(&'static str),
}

pub fn decide(facts: &SessionFacts) -> Decision {
    if facts.uid == 0 {
        // root is allowed: it could stop the service and edit its file anyway.
        return Decision::Allowed;
    }
    let session = match (&facts.caller_session, &facts.display_session) {
        (Some(session), _) => session,
        (None, _) if facts.remote_session_open => {
            return Decision::Refused(
                "the caller is in no session and the user has a remote session open",
            );
        }
        (None, Some(session)) => session,
        (None, None) => return Decision::Refused("the caller is in no session"),
    };
    if session.remote {
        Decision::Refused("the caller's session is remote")
    } else if !session.active {
        Decision::Refused("the caller's session is not the active one on this seat")
    } else {
        Decision::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVE_LOCAL: Option<SessionState> = Some(SessionState {
        active: true,
        remote: false,
    });
    const REMOTE: Option<SessionState> = Some(SessionState {
        active: true,
        remote: true,
    });
    const INACTIVE: Option<SessionState> = Some(SessionState {
        active: false,
        remote: false,
    });

    fn facts(caller: Option<SessionState>, display: Option<SessionState>) -> SessionFacts {
        SessionFacts {
            uid: 1000,
            caller_session: caller,
            display_session: display,
            remote_session_open: false,
        }
    }

    #[test]
    fn the_calling_session_decides_not_the_user() {
        assert_eq!(decide(&facts(ACTIVE_LOCAL, None)), Decision::Allowed);
        // The same person over ssh, while their desktop is active: refused.
        assert!(matches!(
            decide(&facts(REMOTE, ACTIVE_LOCAL)),
            Decision::Refused(_)
        ));
        // A session switched away from: refused, whatever the display says.
        assert!(matches!(
            decide(&facts(INACTIVE, ACTIVE_LOCAL)),
            Decision::Refused(_)
        ));
    }

    #[test]
    fn a_process_outside_every_session_falls_back_to_the_display_session() {
        assert_eq!(decide(&facts(None, ACTIVE_LOCAL)), Decision::Allowed);
        assert!(matches!(
            decide(&facts(None, INACTIVE)),
            Decision::Refused(_)
        ));
        assert!(matches!(decide(&facts(None, None)), Decision::Refused(_)));
    }

    #[test]
    fn a_sessionless_caller_is_refused_while_the_user_has_a_remote_login() {
        let mut ssh_started = facts(None, ACTIVE_LOCAL);
        ssh_started.remote_session_open = true;
        assert!(matches!(decide(&ssh_started), Decision::Refused(_)));
        // A caller in the active local session is still itself, ssh or not.
        let mut window_in_session = facts(ACTIVE_LOCAL, ACTIVE_LOCAL);
        window_in_session.remote_session_open = true;
        assert_eq!(decide(&window_in_session), Decision::Allowed);
    }

    #[test]
    fn root_is_allowed() {
        let mut root = facts(None, None);
        root.uid = 0;
        assert_eq!(decide(&root), Decision::Allowed);
    }
}

/// Refusals are logged at most once per [`RefusalLog::INTERVAL`], with how
/// many were swallowed since, so a caller retrying in a loop cannot fill the
/// journal.
#[derive(Debug, Default)]
pub struct RefusalLog {
    last: Option<std::time::Instant>,
    swallowed: u64,
}

impl RefusalLog {
    pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

    /// `Some(n)` when this refusal should be logged, `n` being how many were
    /// not logged before it.
    pub fn note(&mut self, now: std::time::Instant) -> Option<u64> {
        match self.last {
            Some(last) if now.duration_since(last) < Self::INTERVAL => {
                self.swallowed += 1;
                None
            }
            _ => {
                self.last = Some(now);
                Some(std::mem::take(&mut self.swallowed))
            }
        }
    }
}

#[cfg(test)]
mod log_tests {
    use super::RefusalLog;
    use std::time::{Duration, Instant};

    #[test]
    fn a_burst_of_refusals_is_one_line_and_a_count() {
        let mut log = RefusalLog::default();
        let start = Instant::now();
        assert_eq!(log.note(start), Some(0));
        for step in 1..=50 {
            assert_eq!(log.note(start + Duration::from_millis(step)), None);
        }
        assert_eq!(log.note(start + RefusalLog::INTERVAL), Some(50));
    }
}

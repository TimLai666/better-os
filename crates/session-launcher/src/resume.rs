//! The launcher after an update of its own package (AGENTS.md: no Better OS
//! process keeps running a `(deleted)` program).
//!
//! The launcher is the session's leader: GDM ends the session when it exits,
//! so it cannot be restarted the way a unit is. Instead it starts its new
//! program in its own place with `execve(2)` once dpkg has configured the
//! package (`better_core::self_update::UpdateTracker`). The process id stays,
//! so the compositor and the locker stay its children, and the new program
//! finds what it needs in [`VARIABLE`]: their process ids, the compositor's
//! socket, and the import to put back at the end. The session's environment
//! itself travels as the process's environment.

use serde::{Deserialize, Serialize};

use crate::environment::Import;

/// The variable that carries the state across the exec. It is taken out of
/// the environment before anything else starts, so no child ever sees it.
pub const VARIABLE: &str = "BETTER_OS_SESSION_RESUME";

/// What the running session hands to the launcher's new program.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Resume {
    pub compositor: i32,
    /// The launcher's end of the compositor's session socket, kept open so
    /// the compositor does not see it close.
    pub socket: Option<i32>,
    /// Each running child by its [`crate::children::ChildSpec::name`].
    pub children: Vec<(String, i32)>,
    pub import: Import,
    /// Whether the session's record was written ([`crate::record`]).
    #[serde(default)]
    pub recorded: bool,
}

impl Resume {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("the resume state always serializes")
    }

    /// The state, if it is one: process ids are positive, the socket is an
    /// open descriptor's number, and every child is one the launcher knows.
    pub fn decode(text: &str) -> Option<Resume> {
        let resume: Resume = serde_json::from_str(text).ok()?;
        let valid = resume.compositor > 1
            && resume.socket.is_none_or(|fd| fd > 2)
            && resume
                .children
                .iter()
                .all(|(name, pid)| *pid > 1 && crate::children::by_name(name).is_some());
        valid.then_some(resume)
    }
}

/// What is waiting in the launcher's queue when the update is ready.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pending {
    /// A stop signal, a logout, or the compositor's end: the session ends.
    Ending,
    /// A child ended; the new program starts it again (it adopts only the
    /// children that run).
    ChildEnded,
    /// Anything the exec may drop: a restart timer, another look at the file.
    Other,
}

/// Whether the launcher may exec its new program with these waiting. An
/// ending that is already taken would be lost by the exec (its signal was
/// consumed by `sigwait`, its logout call answered), and GDM's SIGKILL later
/// would end the session without its end; so the session ends first, and the
/// new program is not started.
pub fn may_restart(pending: &[Pending]) -> bool {
    !pending.contains(&Pending::Ending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ending_already_taken_is_never_dropped_by_the_exec() {
        assert!(may_restart(&[]));
        assert!(may_restart(&[Pending::Other, Pending::ChildEnded]));
        assert!(!may_restart(&[Pending::Other, Pending::Ending]));
    }

    fn state() -> Resume {
        let mut import = Import::default();
        import.set.insert("DCONF_PROFILE".into(), "betteros".into());
        import.restore_unset.push("DCONF_PROFILE".into());
        Resume {
            compositor: 4242,
            socket: Some(7),
            children: vec![("locker".into(), 4250)],
            import,
            recorded: true,
        }
    }

    #[test]
    fn the_state_survives_the_exec() {
        let text = state().encode();
        assert_eq!(Resume::decode(&text), Some(state()));
    }

    #[test]
    fn a_state_that_cannot_be_the_sessions_is_refused() {
        assert_eq!(Resume::decode("not json"), None);
        let mut bad = state();
        bad.compositor = 1;
        assert_eq!(Resume::decode(&bad.encode()), None);
        let mut bad = state();
        bad.children = vec![("shell".into(), 99)];
        assert_eq!(Resume::decode(&bad.encode()), None);
        let mut bad = state();
        bad.socket = Some(0);
        assert_eq!(Resume::decode(&bad.encode()), None);
    }
}

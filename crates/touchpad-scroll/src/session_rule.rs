//! Who may change the factors.
//!
//! The factor is a property of the machine's touchpad, so the person sitting
//! at it may change it and nobody else may: the caller must belong to a user
//! whose display session is local and active, the same rule polkit's
//! `allow_active` expresses. polkit itself is not used because the service has
//! dropped to an unprivileged account by the time a call arrives, and polkit
//! 0.105 (Ubuntu 22.04) only answers such a mechanism about its own processes.
//! The service asks logind directly instead; this module is the decision.

/// What logind said about the caller's user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFacts {
    pub uid: u32,
    /// The user's display session, if logind names one.
    pub display_session: Option<DisplaySession>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplaySession {
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
    match &facts.display_session {
        None => Decision::Refused("the caller has no display session"),
        Some(session) if session.remote => Decision::Refused("the caller's session is remote"),
        Some(session) if !session.active => {
            Decision::Refused("the caller's session is not the active one on this seat")
        }
        Some(_) => Decision::Allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(uid: u32, session: Option<(bool, bool)>) -> SessionFacts {
        SessionFacts {
            uid,
            display_session: session.map(|(active, remote)| DisplaySession { active, remote }),
        }
    }

    #[test]
    fn only_the_person_at_the_machine_may_change_the_factor() {
        assert_eq!(decide(&facts(1000, Some((true, false)))), Decision::Allowed);
        assert!(matches!(decide(&facts(1000, None)), Decision::Refused(_)));
        assert!(matches!(
            decide(&facts(1000, Some((false, false)))),
            Decision::Refused(_)
        ));
        assert!(matches!(
            decide(&facts(1000, Some((true, true)))),
            Decision::Refused(_)
        ));
        assert_eq!(decide(&facts(0, None)), Decision::Allowed);
    }
}

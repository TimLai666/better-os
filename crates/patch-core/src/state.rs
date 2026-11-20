//! The state an entry is in on this machine, decided from what detection
//! found, whether a fix is recorded, and whether it was just undone.

use crate::entry::CannotCheck;

/// The states the page draws (ticket 90).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryState {
    /// It looked, and the fault is not here.
    NotPresent,
    /// The fault is here and no fix is applied.
    Found,
    /// A fix is applied and the fault is gone.
    FixedAndChecked,
    /// A fix is applied and the fault is still there.
    AppliedButStillPresent,
    /// The fix was just taken back. Until the next check the page says so;
    /// whether the fault came back is what that check shows.
    Undone,
    /// It could not tell.
    CannotCheck(CannotCheck),
}

/// What detection said about one subject.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Seen {
    Present,
    Absent,
    CannotCheck(CannotCheck),
}

/// The state of one subject.
///
/// Not knowing outranks everything: a recorded fix whose fault cannot be
/// checked is not called fixed. A recorded fix is then judged by whether
/// the fault is still there. Only with nothing recorded does a fresh undo
/// show, and otherwise the finding alone decides.
pub fn state(seen: &Seen, recorded: bool, just_undone: bool) -> EntryState {
    match (seen, recorded) {
        (Seen::CannotCheck(reason), _) => EntryState::CannotCheck(reason.clone()),
        (Seen::Present, true) => EntryState::AppliedButStillPresent,
        (Seen::Absent, true) => EntryState::FixedAndChecked,
        (_, false) if just_undone => EntryState::Undone,
        (Seen::Present, false) => EntryState::Found,
        (Seen::Absent, false) => EntryState::NotPresent,
    }
}

impl EntryState {
    /// Whether the fault is there and needs the person: what Home and the
    /// pane's count say.
    pub fn needs_attention(&self) -> bool {
        matches!(self, EntryState::Found | EntryState::AppliedButStillPresent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_combination_has_its_state() {
        let missing = CannotCheck::ProgramMissing("/usr/bin/fc-match");
        let cases = [
            (Seen::Absent, false, false, EntryState::NotPresent),
            (Seen::Present, false, false, EntryState::Found),
            (Seen::Absent, true, false, EntryState::FixedAndChecked),
            (
                Seen::Present,
                true,
                false,
                EntryState::AppliedButStillPresent,
            ),
            (Seen::Present, false, true, EntryState::Undone),
            (Seen::Absent, false, true, EntryState::Undone),
            (
                Seen::CannotCheck(missing.clone()),
                true,
                false,
                EntryState::CannotCheck(missing.clone()),
            ),
            (
                Seen::CannotCheck(missing.clone()),
                false,
                true,
                EntryState::CannotCheck(missing),
            ),
        ];
        for (seen, recorded, undone, expected) in cases {
            assert_eq!(
                state(&seen, recorded, undone),
                expected,
                "{seen:?} {recorded} {undone}"
            );
        }
    }

    #[test]
    fn a_record_outranks_an_old_undo() {
        // An undo, then a fix again: the record decides.
        assert_eq!(
            state(&Seen::Absent, true, true),
            EntryState::FixedAndChecked
        );
    }

    #[test]
    fn only_a_fault_that_is_there_needs_attention() {
        assert!(EntryState::Found.needs_attention());
        assert!(EntryState::AppliedButStillPresent.needs_attention());
        for state in [
            EntryState::NotPresent,
            EntryState::FixedAndChecked,
            EntryState::Undone,
            EntryState::CannotCheck(CannotCheck::SessionTypeUnknown),
        ] {
            assert!(!state.needs_attention(), "{state:?}");
        }
    }
}

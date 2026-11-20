//! Running entries against a host: check, fix (then check the fix), undo.
//!
//! These are the only paths from an entry's decisions to the host's
//! changes, and each reads fresh answers right before it decides.

use crate::entry::{
    Answer, Answers, CannotCheck, Change, ChangeError, CheckOutcome, Detection, Entry, EntryId,
    Finding, Host, PlanError, Read, ReadError, Subject,
};
use crate::record::{AppliedRecord, ChangedElsewhere, Recorded, Records};
use crate::state::{EntryState, Seen, state};

/// Asks the host every read not already answered. A command that would
/// change the machine is never run as a read.
pub fn gather(host: &mut dyn Host, reads: &[Read], answers: &mut Answers) {
    for read in reads {
        if answers.contains(read) {
            continue;
        }
        let answer = match read {
            Read::Command(command) if command.changes_the_machine() => {
                Answer::Output(Err(ReadError::Failed(format!(
                    "{} changes the machine and is not a read",
                    command.program()
                ))))
            }
            _ => host.read(read),
        };
        answers.insert(read.clone(), answer);
    }
}

fn fresh(host: &mut dyn Host, reads: &[Read]) -> Answers {
    let mut answers = Answers::new();
    gather(host, reads, &mut answers);
    answers
}

/// One subject of an entry, as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub subject: Subject,
    /// What detection found, when it found it.
    pub finding: Option<Finding>,
    /// The fix applied to it, when one is.
    pub record: Option<AppliedRecord>,
    /// Keys of that fix someone changed afterwards.
    pub changed_elsewhere: Vec<ChangedElsewhere>,
    pub state: EntryState,
}

/// What one entry is on this machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub entry: EntryId,
    pub detection: Detection,
    /// Every subject that was found, has a fix recorded, or was just undone.
    pub instances: Vec<Instance>,
}

impl Report {
    /// The entry's own state on the "all known problems" list: cannot check
    /// when it could not look, otherwise the most pressing instance's state,
    /// and "not present" when it has none.
    pub fn overall(&self) -> EntryState {
        if let Detection::CannotCheck(reason) = &self.detection {
            return EntryState::CannotCheck(reason.clone());
        }
        let rank = |state: &EntryState| match state {
            EntryState::AppliedButStillPresent => 0,
            EntryState::Found => 1,
            EntryState::Undone => 2,
            EntryState::FixedAndChecked => 3,
            EntryState::CannotCheck(_) => 4,
            EntryState::NotPresent => 5,
        };
        self.instances
            .iter()
            .map(|instance| instance.state.clone())
            .min_by_key(rank)
            .unwrap_or(EntryState::NotPresent)
    }

    /// How many of its faults are here and need the person.
    pub fn needing_attention(&self) -> usize {
        self.instances
            .iter()
            .filter(|instance| instance.state.needs_attention())
            .count()
    }
}

/// Checks one entry: detects, and lines the findings up with the fixes
/// recorded for it and the subjects just undone.
pub fn check(
    entry: &dyn Entry,
    host: &mut dyn Host,
    records: &Records,
    just_undone: &[Subject],
) -> Report {
    let answers = fresh(host, &entry.detection_reads());
    let detection = entry.detect(&answers);
    let findings: &[Finding] = match &detection {
        Detection::Checked(findings) => findings,
        Detection::CannotCheck(_) => &[],
    };
    let mut subjects: Vec<Subject> = findings.iter().map(|f| f.subject.clone()).collect();
    for record in records.of(entry.id()) {
        if !subjects.contains(&record.subject) {
            subjects.push(record.subject.clone());
        }
    }
    for subject in just_undone {
        if !subjects.contains(subject) {
            subjects.push(subject.clone());
        }
    }
    let instances = subjects
        .into_iter()
        .map(|subject| {
            let finding = findings.iter().find(|f| f.subject == subject).cloned();
            let record = records.find(entry.id(), &subject).cloned();
            let seen = match (&detection, &finding) {
                (Detection::CannotCheck(reason), _) => Seen::CannotCheck(reason.clone()),
                (_, Some(_)) => Seen::Present,
                (_, None) => Seen::Absent,
            };
            let changed_elsewhere = record
                .as_ref()
                .map(|record| {
                    let answers = fresh(host, &entry.undo_reads(record));
                    entry
                        .plan_undo(record, &answers, false)
                        .map(|plan| plan.skipped)
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let state = state(&seen, record.is_some(), just_undone.contains(&subject));
            Instance {
                subject,
                finding,
                record,
                changed_elsewhere,
                state,
            }
        })
        .collect();
    Report {
        entry: entry.id(),
        detection,
        instances,
    }
}

/// Why a fix or an undo did not happen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixError {
    Plan(PlanError),
    /// A change failed. `applied` is the record of the changes made before
    /// it, when there were any, so they can still be undone.
    Change {
        error: ChangeError,
        applied: Option<Box<AppliedRecord>>,
    },
}

impl FixError {
    /// The reason, for the error's details.
    pub fn details(&self) -> String {
        match self {
            FixError::Plan(PlanError::NothingToChange) => {
                "everything the fix sets is already set".into()
            }
            FixError::Plan(PlanError::NotThisEntry) => {
                "the finding belongs to another entry".into()
            }
            FixError::Plan(PlanError::NotOffered(why)) => format!("not offered: {why:?}"),
            FixError::Plan(PlanError::CannotRead(reason)) => format!("{reason:?}"),
            FixError::Change { error, .. } => error.to_string(),
        }
    }
}

/// A fix that was made, and how its check came out.
#[derive(Clone, Debug, PartialEq)]
pub struct FixOutcome {
    pub record: AppliedRecord,
    pub check: CheckOutcome,
}

fn run(host: &mut dyn Host, changes: &[Change]) -> Result<(), (ChangeError, usize)> {
    for (index, change) in changes.iter().enumerate() {
        host.change(change).map_err(|error| (error, index))?;
    }
    Ok(())
}

/// Fixes a finding: plans from a fresh read, makes the changes, records
/// them, and checks the fix from another fresh read.
pub fn fix(
    entry: &dyn Entry,
    finding: &Finding,
    host: &mut dyn Host,
    now_unix_seconds: u64,
) -> Result<FixOutcome, FixError> {
    let answers = fresh(host, &entry.plan_reads(finding));
    let plan = entry.plan(finding, &answers).map_err(FixError::Plan)?;
    let record = AppliedRecord {
        entry: entry.id(),
        subject: finding.subject.clone(),
        applied_at: now_unix_seconds,
        change: plan.record,
    };
    if let Err((error, index)) = run(host, &plan.changes) {
        return Err(FixError::Change {
            error,
            applied: (index > 0).then(|| Box::new(record.clone())),
        });
    }
    let answers = fresh(host, &entry.check_reads(&record));
    let check = entry.check(&record, &answers);
    Ok(FixOutcome { record, check })
}

/// An undo that was made.
#[derive(Clone, Debug, PartialEq)]
pub struct UndoOutcome {
    /// What stays recorded, `None` when everything was taken back.
    pub remaining: Option<Recorded>,
    /// Keys left as another program changed them.
    pub skipped: Vec<ChangedElsewhere>,
}

/// Takes a fix back, from a fresh read of what is there now.
pub fn undo(
    entry: &dyn Entry,
    record: &AppliedRecord,
    host: &mut dyn Host,
    confirm_changed: bool,
) -> Result<UndoOutcome, FixError> {
    let answers = fresh(host, &entry.undo_reads(record));
    let plan = entry
        .plan_undo(record, &answers, confirm_changed)
        .map_err(FixError::Plan)?;
    run(host, &plan.changes).map_err(|(error, _)| FixError::Change {
        error,
        applied: Some(Box::new(record.clone())),
    })?;
    Ok(UndoOutcome {
        remaining: plan.remaining,
        skipped: plan.skipped,
    })
}

/// A read that was not answered with the kind of answer it asks for. A
/// fault in the host, reported as "cannot check" rather than guessed past.
pub(crate) fn not_answered() -> CannotCheck {
    CannotCheck::NotAnswered
}

#[cfg(test)]
pub(crate) mod fake {
    //! A host made of canned answers, which records every change.

    use super::*;

    /// What a test makes the machine do after a change.
    pub(crate) type AfterChange = Box<dyn FnMut(&Change, &mut Answers)>;

    #[derive(Default)]
    pub(crate) struct FakeHost {
        pub(crate) answers: Answers,
        pub(crate) changes: Vec<Change>,
        pub(crate) fail_changes: Option<ChangeError>,
        /// Called after each change, so a test can make the machine move.
        pub(crate) after_change: Option<AfterChange>,
        pub(crate) reads: Vec<Read>,
    }

    impl Host for FakeHost {
        fn read(&mut self, read: &Read) -> Answer {
            self.reads.push(read.clone());
            self.answers
                .get(read)
                .cloned()
                .unwrap_or(Answer::Output(Err(ReadError::Failed(
                    "no canned answer".into(),
                ))))
        }

        fn change(&mut self, change: &Change) -> Result<(), ChangeError> {
            if let Some(error) = &self.fail_changes {
                return Err(error.clone());
            }
            self.changes.push(change.clone());
            if let Some(after) = self.after_change.as_mut() {
                after(change, &mut self.answers);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHost;
    use super::*;
    use crate::command::Command;

    #[test]
    fn a_command_that_changes_the_machine_is_never_run_as_a_read() {
        let mut host = FakeHost::default();
        let answers = fresh(&mut host, &[Read::Command(Command::RebuildFontCache)]);
        assert!(host.reads.is_empty());
        assert!(matches!(
            answers.get(&Read::Command(Command::RebuildFontCache)),
            Some(Answer::Output(Err(_)))
        ));
    }

    #[test]
    fn a_read_is_asked_once() {
        let mut host = FakeHost::default();
        fresh(&mut host, &[Read::SessionType, Read::SessionType]);
        assert_eq!(host.reads, [Read::SessionType]);
    }
}

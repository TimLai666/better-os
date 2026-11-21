//! Entries 3 and 4: the USB-C faults of the ASUS Vivobook S 14 M5406WA and
//! S 15 M5506WA (ticket 101, ADR 0046, `engineering/specs/better-patch.md`).
//!
//! - **Entry 3, the USB-C interface.** The firmware's UCSI interface stops
//!   answering (`ucsi_acpi … GET_CONNECTOR_STATUS failed (-110)`, and after
//!   a resume `failed to re-enable notifications (-110)`), and a port stops
//!   charging. Reloading `ucsi_acpi` brings both ports back until the next
//!   sleep. The fix turns that reload on after every sleep and makes it now.
//! - **Entry 4, the USB controller.** The xHCI host of one USB-C side dies
//!   on resume (`HC died`), its hub work then never ends, and every sleep
//!   after it fails. The fix turns on the after-sleep rebind of that one
//!   controller and the overnight guard (notify; power off only when the
//!   person also chose it), and rebinds it now when the hub work is not
//!   stuck. When it is, only a restart helps, and the card says so.
//!
//! Both are found only on a known model; elsewhere they do not apply and
//! offer nothing. The machine keeps the fix (the daemon's choice file), so
//! every account sees the same state and Undo turns the choice off.

use hardware_core::usb_recovery::{self as core, UsbChoice};

use crate::entry::{
    Answer, Answers, CannotCheck, Change, ChangeLine, CheckOutcome, Description, Detection, Entry,
    EntryId, Finding, FindingDetail, Limit, Plan, PlanError, Privileged, Read, Subject, UndoKind,
    UndoPlan, UsbMachineView,
};
use crate::record::{AppliedRecord, Recorded};

pub struct UsbCInterface;
pub struct UsbController;

fn reads() -> Vec<Read> {
    vec![Read::UsbMachine, Read::UsbFindings]
}

/// The machine and this boot's findings, or why not.
fn looked(answers: &Answers) -> Result<(UsbMachineView, core::UsbFindings), CannotCheck> {
    let machine = match answers.get(&Read::UsbMachine) {
        Some(Answer::UsbMachine(Ok(view))) => view.clone(),
        Some(Answer::UsbMachine(Err(error))) => {
            return Err(CannotCheck::FileUnreadable(error.to_string()));
        }
        _ => return Err(crate::engine::not_answered()),
    };
    let findings = match answers.get(&Read::UsbFindings) {
        Some(Answer::UsbFindings(Ok(findings))) => findings.clone(),
        Some(Answer::UsbFindings(Err(error))) => {
            return Err(CannotCheck::KernelLogUnreadable(error.to_string()));
        }
        _ => return Err(crate::engine::not_answered()),
    };
    Ok((machine, findings))
}

/// The model check comes first: on another machine nothing is read into a
/// fault, and a log that cannot be read does not matter.
fn applies(answers: &Answers) -> Option<bool> {
    match answers.get(&Read::UsbMachine) {
        Some(Answer::UsbMachine(Ok(view))) => Some(view.present.model.known().is_some()),
        _ => None,
    }
}

fn ucsi_finding(findings: &core::UsbFindings) -> Option<Finding> {
    findings.ucsi.active.then_some(Finding {
        subject: Subject::Machine,
        detail: FindingDetail::UsbCInterface {
            timeouts: findings.ucsi.timeouts,
            after_sleep: findings.ucsi.notification_failures,
        },
    })
}

fn controller_finding(findings: &core::UsbFindings, stuck: bool) -> Option<Finding> {
    let death = findings.dead_controller()?;
    Some(Finding {
        subject: Subject::Machine,
        detail: FindingDetail::UsbController {
            address: death.address.to_string(),
            sleep_failures: findings.sleep.blocked_by_hub_work,
            streak: findings.sleep.streak,
            restart_needed: findings.hub_work_hung || stuck,
        },
    })
}

fn detect_with(
    answers: &Answers,
    find: impl Fn(&core::UsbFindings) -> Option<Finding>,
) -> Detection {
    match applies(answers) {
        Some(false) => return Detection::DoesNotApply,
        None => {}
        Some(true) => {}
    }
    match looked(answers) {
        Err(reason) => Detection::CannotCheck(reason),
        Ok((_, findings)) => Detection::Checked(find(&findings).into_iter().collect()),
    }
}

fn record(answers: &Answers, choice: UsbChoice) -> Vec<(Subject, Recorded)> {
    match answers.get(&Read::UsbMachine) {
        Some(Answer::UsbMachine(Ok(view))) if view.settings.get(choice) => {
            vec![(Subject::Machine, Recorded::UsbRecovery { choice })]
        }
        _ => Vec::new(),
    }
}

fn set(choice: UsbChoice, on: bool) -> Change {
    Change::Privileged(Privileged::SetChoice { choice, on })
}

fn check_with(answers: &Answers, present: impl Fn(&core::UsbFindings) -> bool) -> CheckOutcome {
    match looked(answers) {
        Err(reason) => CheckOutcome::CannotCheck(reason),
        Ok((_, findings)) if present(&findings) => CheckOutcome::StillPresent,
        Ok(_) => CheckOutcome::Passed,
    }
}

fn undo(record: &AppliedRecord, choice: UsbChoice) -> Result<UndoPlan, PlanError> {
    if record.change != (Recorded::UsbRecovery { choice }) {
        return Err(PlanError::NotThisEntry);
    }
    Ok(UndoPlan {
        changes: vec![set(choice, false)],
        remaining: None,
        skipped: Vec::new(),
    })
}

impl Entry for UsbCInterface {
    fn id(&self) -> EntryId {
        EntryId::UsbCInterface
    }

    fn undo_kind(&self) -> UndoKind {
        UndoKind::Reversible
    }

    fn detection_reads(&self) -> Vec<Read> {
        reads()
    }

    fn detect(&self, answers: &Answers) -> Detection {
        detect_with(answers, ucsi_finding)
    }

    fn describe(&self, _: &Subject) -> Description {
        Description {
            change: vec![
                ChangeLine::Privileged(Privileged::SetChoice {
                    choice: UsbChoice::UcsiAutomatic,
                    on: true,
                }),
                ChangeLine::Privileged(Privileged::ReloadUcsi),
            ],
            undo: UndoKind::Reversible,
            limits: vec![
                Limit::KnownModelsOnly,
                Limit::ResetsNotRepairs,
                Limit::NotMeasured,
            ],
        }
    }

    fn plan_reads(&self, _: &Finding) -> Vec<Read> {
        reads()
    }

    fn plan(&self, finding: &Finding, answers: &Answers) -> Result<Plan, PlanError> {
        if !matches!(finding.detail, FindingDetail::UsbCInterface { .. }) {
            return Err(PlanError::NotThisEntry);
        }
        let (machine, findings) = looked(answers).map_err(PlanError::CannotRead)?;
        let mut changes = Vec::new();
        if !machine.settings.ucsi_automatic {
            changes.push(set(UsbChoice::UcsiAutomatic, true));
        }
        if findings.ucsi.active {
            changes.push(Change::Privileged(Privileged::ReloadUcsi));
        }
        if changes.is_empty() {
            return Err(PlanError::NothingToChange);
        }
        Ok(Plan {
            changes,
            record: Recorded::UsbRecovery {
                choice: UsbChoice::UcsiAutomatic,
            },
        })
    }

    fn check_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        reads()
    }

    fn check(&self, _: &AppliedRecord, answers: &Answers) -> CheckOutcome {
        check_with(answers, |findings| findings.ucsi.active)
    }

    fn undo_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        Vec::new()
    }

    fn plan_undo(
        &self,
        record: &AppliedRecord,
        _: &Answers,
        _: bool,
    ) -> Result<UndoPlan, PlanError> {
        undo(record, UsbChoice::UcsiAutomatic)
    }

    fn kept_by_machine(&self) -> bool {
        true
    }

    fn machine_records(&self, answers: &Answers) -> Vec<(Subject, Recorded)> {
        record(answers, UsbChoice::UcsiAutomatic)
    }
}

impl Entry for UsbController {
    fn id(&self) -> EntryId {
        EntryId::UsbController
    }

    fn undo_kind(&self) -> UndoKind {
        UndoKind::Reversible
    }

    fn detection_reads(&self) -> Vec<Read> {
        reads()
    }

    fn detect(&self, answers: &Answers) -> Detection {
        detect_with(answers, |findings| controller_finding(findings, false))
    }

    fn describe(&self, _: &Subject) -> Description {
        Description {
            change: vec![
                ChangeLine::Privileged(Privileged::SetChoice {
                    choice: UsbChoice::ControllerAutomatic,
                    on: true,
                }),
                ChangeLine::Privileged(Privileged::RebindController),
            ],
            undo: UndoKind::Reversible,
            limits: vec![
                Limit::KnownModelsOnly,
                Limit::RestartWhenStuck,
                Limit::NotMeasured,
            ],
        }
    }

    fn plan_reads(&self, _: &Finding) -> Vec<Read> {
        reads()
    }

    fn plan(&self, finding: &Finding, answers: &Answers) -> Result<Plan, PlanError> {
        let FindingDetail::UsbController { restart_needed, .. } = &finding.detail else {
            return Err(PlanError::NotThisEntry);
        };
        let (machine, findings) = looked(answers).map_err(PlanError::CannotRead)?;
        let mut changes = Vec::new();
        if !machine.settings.controller_automatic {
            changes.push(set(UsbChoice::ControllerAutomatic, true));
        }
        // A stuck hub is a restart: the rebind is not even asked for. The
        // daemon checks again and would refuse it.
        if findings.dead_controller().is_some() && !restart_needed && !findings.hub_work_hung {
            changes.push(Change::Privileged(Privileged::RebindController));
        }
        if changes.is_empty() {
            return Err(PlanError::NothingToChange);
        }
        Ok(Plan {
            changes,
            record: Recorded::UsbRecovery {
                choice: UsbChoice::ControllerAutomatic,
            },
        })
    }

    fn check_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        reads()
    }

    fn check(&self, _: &AppliedRecord, answers: &Answers) -> CheckOutcome {
        check_with(answers, |findings| findings.dead_controller().is_some())
    }

    fn undo_reads(&self, _: &AppliedRecord) -> Vec<Read> {
        Vec::new()
    }

    fn plan_undo(
        &self,
        record: &AppliedRecord,
        _: &Answers,
        _: bool,
    ) -> Result<UndoPlan, PlanError> {
        undo(record, UsbChoice::ControllerAutomatic)
    }

    fn kept_by_machine(&self) -> bool {
        true
    }

    fn machine_records(&self, answers: &Answers) -> Vec<(Subject, Recorded)> {
        record(answers, UsbChoice::ControllerAutomatic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{self, fake::FakeHost};
    use crate::record::Records;
    use crate::state::EntryState;
    use hardware_core::usb_recovery::{MachineModel, UsbPresent, UsbRecoverySettings};

    const UCSI: &str = "\
ucsi_acpi USBC000:00: ucsi_handle_connector_change: GET_CONNECTOR_STATUS failed (-110)
PM: suspend entry (s2idle)
PM: suspend exit
ucsi_acpi USBC000:00: failed to re-enable notifications (-110)
";

    const DIED: &str = "\
xhci_hcd 0000:63:00.4: HC died; cleaning up
PM: suspend entry (s2idle)
Freezing remaining freezable tasks failed after 20.004 seconds (0 tasks refusing to freeze, wq_busy=1):
workqueue usb_hub_wq: flags=0x104
PM: suspend exit
";

    fn host(board: &str, log: &str, settings: UsbRecoverySettings) -> FakeHost {
        let mut host = FakeHost::default();
        answer(&mut host.answers, board, log, settings);
        host
    }

    fn answer(answers: &mut Answers, board: &str, log: &str, settings: UsbRecoverySettings) {
        answers.insert(
            Read::UsbMachine,
            Answer::UsbMachine(Ok(UsbMachineView {
                present: UsbPresent {
                    model: MachineModel {
                        board_name: Some(board.into()),
                        ..MachineModel::default()
                    },
                    ..UsbPresent::default()
                },
                settings,
            })),
        );
        answers.insert(
            Read::UsbFindings,
            Answer::UsbFindings(Ok(core::classify(log))),
        );
    }

    #[test]
    fn another_model_does_not_apply_and_offers_nothing() {
        let mut host = host(
            "UX3405MA",
            &format!("{UCSI}{DIED}"),
            UsbRecoverySettings::default(),
        );
        // Even a log nobody can read does not matter there.
        host.answers.insert(
            Read::UsbFindings,
            Answer::UsbFindings(Err(crate::ReadError::Failed("denied".into()))),
        );
        for entry in [&UsbCInterface as &dyn Entry, &UsbController] {
            let report = engine::check(entry, &mut host, &Records::default(), &[]);
            assert_eq!(report.detection, Detection::DoesNotApply);
            assert_eq!(report.overall(), EntryState::DoesNotApply);
            assert!(report.instances.is_empty());
        }
    }

    #[test]
    fn found_fixed_checked_and_undone_through_the_daemons_choice() {
        let mut host = host("M5406WA", UCSI, UsbRecoverySettings::default());
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(report.instances[0].state, EntryState::Found);
        let finding = report.instances[0].finding.clone().unwrap();

        // The daemon turns the choice on and the reload writes its marker.
        host.after_change = Some(Box::new(|change, answers| match change {
            Change::Privileged(Privileged::SetChoice { choice, on }) => {
                if let Some(Answer::UsbMachine(Ok(mut view))) =
                    answers.get(&Read::UsbMachine).cloned()
                {
                    view.settings = view.settings.with(*choice, *on);
                    answers.insert(Read::UsbMachine, Answer::UsbMachine(Ok(view)));
                }
            }
            Change::Privileged(Privileged::ReloadUcsi) => answers.insert(
                Read::UsbFindings,
                Answer::UsbFindings(Ok(core::classify(&format!(
                    "{UCSI}{}\n",
                    core::UCSI_RELOADED_MARKER
                )))),
            ),
            _ => {}
        }));
        let outcome = engine::fix(&UsbCInterface, &finding, &mut host, 1).unwrap();
        assert_eq!(outcome.check, CheckOutcome::Passed);
        assert_eq!(
            host.changes,
            vec![
                Change::Privileged(Privileged::SetChoice {
                    choice: UsbChoice::UcsiAutomatic,
                    on: true
                }),
                Change::Privileged(Privileged::ReloadUcsi),
            ]
        );
        // The person's own record file keeps nothing.
        let mut records = Records::default();
        records.keep(&UsbCInterface, outcome.record);
        assert!(records.applied.is_empty());

        // Every account sees it fixed, from the machine's choice.
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(report.instances[0].state, EntryState::FixedAndChecked);
        let record = report.instances[0].record.clone().unwrap();

        // The next sleep's timeout, with the choice on: applied, still
        // there (until the after-sleep run resets it).
        let settings = UsbRecoverySettings::default().with(UsbChoice::UcsiAutomatic, true);
        let again = format!(
            "{UCSI}{}\nucsi_acpi USBC000:00: failed to re-enable notifications (-110)\n",
            core::UCSI_RELOADED_MARKER
        );
        answer(&mut host.answers, "M5406WA", &again, settings);
        let report = engine::check(&UsbCInterface, &mut host, &Records::default(), &[]);
        assert_eq!(
            report.instances[0].state,
            EntryState::AppliedButStillPresent
        );

        host.changes.clear();
        let undone = engine::undo(&UsbCInterface, &record, &mut host, false).unwrap();
        assert_eq!(undone.remaining, None);
        assert_eq!(
            host.changes,
            vec![Change::Privileged(Privileged::SetChoice {
                choice: UsbChoice::UcsiAutomatic,
                on: false
            })]
        );
    }

    #[test]
    fn a_stuck_hub_turns_the_guard_on_and_asks_for_no_rebind() {
        let mut host = host("M5406WA", DIED, UsbRecoverySettings::default());
        let report = engine::check(&UsbController, &mut host, &Records::default(), &[]);
        let finding = report.instances[0].finding.clone().unwrap();
        assert_eq!(
            finding.detail,
            FindingDetail::UsbController {
                address: "0000:63:00.4".into(),
                sleep_failures: 1,
                streak: 1,
                restart_needed: true,
            }
        );
        let plan = UsbController.plan(&finding, &host.answers).unwrap();
        assert_eq!(
            plan.changes,
            vec![Change::Privileged(Privileged::SetChoice {
                choice: UsbChoice::ControllerAutomatic,
                on: true
            })]
        );
    }

    #[test]
    fn a_dead_controller_with_free_hub_work_is_rebound() {
        let log = "xhci_hcd 0000:63:00.4: HC died; cleaning up\n";
        let host = host(
            "M5506WA",
            log,
            UsbRecoverySettings::default().with(UsbChoice::ControllerAutomatic, true),
        );
        let finding = match UsbController.detect(&host.answers) {
            Detection::Checked(found) => found[0].clone(),
            other => panic!("{other:?}"),
        };
        let plan = UsbController.plan(&finding, &host.answers).unwrap();
        assert_eq!(
            plan.changes,
            vec![Change::Privileged(Privileged::RebindController)]
        );
        let mut answers = host.answers.clone();
        answer(
            &mut answers,
            "M5506WA",
            &format!(
                "{log}{}\n",
                core::rebound_marker(&core::PciAddress::parse("0000:63:00.4").unwrap())
            ),
            UsbRecoverySettings::default(),
        );
        let record = AppliedRecord {
            entry: EntryId::UsbController,
            subject: Subject::Machine,
            applied_at: 0,
            change: Recorded::UsbRecovery {
                choice: UsbChoice::ControllerAutomatic,
            },
        };
        assert_eq!(UsbController.check(&record, &answers), CheckOutcome::Passed);
    }

    #[test]
    fn an_unreadable_log_on_an_affected_model_cannot_be_checked() {
        let mut answers = Answers::new();
        answer(&mut answers, "M5406WA", "", UsbRecoverySettings::default());
        answers.insert(
            Read::UsbFindings,
            Answer::UsbFindings(Err(crate::ReadError::Failed("no daemon".into()))),
        );
        assert!(matches!(
            UsbCInterface.detect(&answers),
            Detection::CannotCheck(CannotCheck::KernelLogUnreadable(_))
        ));
    }
}

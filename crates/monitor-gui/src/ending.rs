//! Ending processes: whether an End task or a Force stop may be asked at
//! all, and carrying a confirmed one out. GPUI-free, so every refusal and
//! every outcome is tested against a fake controller.
//!
//! The window hands over the targets as the list last saw them; the
//! controller (`monitor-actions-linux` in the window) decides each one
//! again and proves each identity before a signal goes out.

use monitor_core::{
    ActionError, ActionOutcome, ActionRefusal, ProcessAction, ProcessController, ProcessTarget,
};

use crate::app::{ActionReport, EndHow};

/// Why nothing of `targets` may be ended this way, or `None` when it may be
/// asked. One process that may be ended is enough to ask, except that a
/// force stop is refused whole when any of them is something the session
/// stands on, as a tree is: killing one of those ends far more than the
/// person pointed at.
pub(crate) fn refusal(
    controller: &impl ProcessController,
    targets: &[ProcessTarget],
    how: EndHow,
) -> Option<ActionRefusal> {
    let action = how.action();
    let mut first = None;
    let mut allowed = false;
    for target in targets {
        match controller.availability(target, action).refusal() {
            None => allowed = true,
            Some(refusal @ ActionRefusal::SessionProcess { .. }) => return Some(refusal.clone()),
            Some(refusal) => {
                first.get_or_insert_with(|| refusal.clone());
            }
        }
    }
    if allowed {
        None
    } else {
        first.or(Some(ActionRefusal::OwnershipUnknown))
    }
}

/// Carry out a confirmed end. Every target is decided again; one that may
/// not be ended (another user's, a protected one) is skipped, and one that
/// has already gone is not a failure of the whole.
pub(crate) fn carry_out(
    controller: &mut impl ProcessController,
    name: String,
    targets: &[ProcessTarget],
    how: EndHow,
) -> ActionReport {
    if let Some(refusal @ ActionRefusal::SessionProcess { .. }) = refusal(controller, targets, how)
    {
        return ActionReport::Refused { name, refusal };
    }
    let action = how.action();
    let mut sent = 0;
    let mut failure = None;
    for target in targets {
        if controller.availability(target, action).refusal().is_some() {
            continue;
        }
        match controller.perform(target, action) {
            Ok(_) => sent += 1,
            Err(ActionError::ProcessDisappeared { .. }) => {}
            Err(error) => {
                failure.get_or_insert(error);
            }
        }
    }
    match (sent, failure) {
        (0, Some(error)) => ActionReport::Failed { name, error },
        (0, None) => ActionReport::Failed {
            name,
            error: ActionError::ProcessDisappeared {
                pid: targets.first().map(|target| target.pid).unwrap_or_default(),
            },
        },
        (count, _) => ActionReport::Succeeded {
            name,
            outcome: ActionOutcome::SignalAccepted {
                signal: action
                    .signal()
                    .expect("ending a process is always a signal"),
            },
            count,
        },
    }
}

impl EndHow {
    /// The intent the controller is handed.
    pub(crate) fn action(self) -> ProcessAction {
        match self {
            EndHow::Close => ProcessAction::Terminate,
            EndHow::Force => ProcessAction::ForceStop,
        }
    }
}

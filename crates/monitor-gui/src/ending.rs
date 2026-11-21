//! Ending processes: whether an End task or a Force stop may be asked at
//! all, how many processes it will really reach, and carrying a confirmed
//! one out. GPUI-free, so every refusal and every outcome is tested against
//! a fake controller.
//!
//! The window hands over the targets as the list last saw them; the
//! controller (`monitor-actions-linux` in the window) decides each one
//! again and proves each identity before a signal goes out.

use monitor_core::{
    ActionError, ActionOutcome, ActionRefusal, ProcessAction, ProcessController, ProcessTarget,
};

use crate::app::{ActionReport, EndHow, Shortfall};

/// Why nothing of `targets` may be ended this way, or `None` when it may be
/// asked. One process that may be ended is enough to ask, except that a
/// force stop is refused whole when any of them is something the session
/// stands on, as a tree is: killing one of those ends far more than the
/// person pointed at.
///
/// Every place that offers End task or Force stop (the page commands and
/// both row menus) asks this, so a disabled command and the refusal after a
/// click always agree.
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

/// How many of `targets` the end will really be sent to: the confirmation
/// counts these, not the processes it will skip.
pub(crate) fn reachable(
    controller: &impl ProcessController,
    targets: &[ProcessTarget],
    how: EndHow,
) -> usize {
    targets
        .iter()
        .filter(|target| controller.availability(target, how.action()).is_available())
        .count()
}

/// Carry out a confirmed end. Every target is decided again. One that has
/// already gone is not counted either way; one that may not be ended now,
/// or that the kernel refused, is reported beside the ones that were sent,
/// never folded into a plain success.
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
    let signal = action
        .signal()
        .expect("ending a process is always a signal");
    let mut sent = 0;
    let mut missed = 0;
    let mut why: Option<Shortfall> = None;
    for target in targets {
        if let Some(refusal) = controller.availability(target, action).refusal() {
            missed += 1;
            why.get_or_insert_with(|| Shortfall::Refused(refusal.clone()));
            continue;
        }
        match controller.perform(target, action) {
            Ok(_) => sent += 1,
            Err(ActionError::ProcessDisappeared { .. }) => {}
            Err(error) => {
                missed += 1;
                why.get_or_insert(Shortfall::Failed(error));
            }
        }
    }
    match (sent, why) {
        (0, Some(Shortfall::Failed(error))) => ActionReport::Failed { name, error },
        (0, Some(Shortfall::Refused(refusal))) => ActionReport::Refused { name, refusal },
        (0, None) => ActionReport::Failed {
            name,
            error: ActionError::ProcessDisappeared {
                pid: targets.first().map(|target| target.pid).unwrap_or_default(),
            },
        },
        (count, None) => ActionReport::Succeeded {
            name,
            outcome: ActionOutcome::SignalAccepted { signal },
            count,
        },
        (sent, Some(why)) => ActionReport::Partly {
            name,
            signal,
            sent,
            missed,
            why,
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

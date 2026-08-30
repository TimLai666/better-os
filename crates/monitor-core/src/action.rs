//! Process control, as a contract rather than as a syscall.
//!
//! Better Monitor's GUI must never construct a signal number, call `kill`, or
//! run a shell. It describes an intent — stop this process gracefully, lower
//! this process's priority — and a controller behind this trait decides
//! whether the intent is allowed and carries it out. That boundary is what
//! makes the privilege rule testable: a GUI test can drive every action and
//! every refusal through a fake controller without a real process existing.
//!
//! Two rules are encoded here rather than left to each implementation.
//!
//! The first is that refusal is data. An action on another user's process is
//! not an error to be surfaced as a failure after the fact; it is a state the
//! button can be rendered in, with the reason attached, before anyone clicks.
//!
//! The second is that a delivered signal is not a completed outcome. The
//! kernel accepting `SIGTERM` says the process was told to exit, not that it
//! did. The outcome type says exactly that, so no screen can claim more.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The lowest nice value an unprivileged caller may set.
///
/// `setpriority(2)`: raising a process's priority — lowering its nice value —
/// requires `CAP_SYS_NICE`, so an unprivileged Better Monitor can only ever
/// move a process down the queue. The floor is the process's current nice
/// value, not a constant, which is why the check needs the target.
pub const NICE_MAXIMUM: i32 = 19;

/// The most negative nice value the kernel accepts at all.
pub const NICE_MINIMUM: i32 = -20;

/// What the user asked for.
///
/// This is deliberately not a signal. `Terminate` means "ask it to exit", and
/// the mapping to `SIGTERM` belongs to the platform implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAction {
    /// Ask the process to exit and let it clean up.
    Terminate,
    /// Stop the process immediately. Unsaved work is lost.
    ForceStop,
    /// Suspend scheduling without ending the process.
    Pause,
    /// Resume a suspended process.
    Resume,
    /// Move the process down the scheduler queue by raising its nice value.
    SetNice(i32),
}

impl ProcessAction {
    /// A stable key for element identifiers, logs, and test assertions. It is
    /// not a user-facing label; presentation layers own the wording.
    pub fn key(self) -> &'static str {
        match self {
            ProcessAction::Terminate => "terminate",
            ProcessAction::ForceStop => "force-stop",
            ProcessAction::Pause => "pause",
            ProcessAction::Resume => "resume",
            ProcessAction::SetNice(_) => "set-nice",
        }
    }

    /// Whether the user must confirm before this runs.
    ///
    /// Anything that can end a process and lose work asks first. Pausing,
    /// resuming, and renicing are reversible by the same menu that applied
    /// them, so they do not.
    pub fn requires_confirmation(self) -> bool {
        matches!(self, ProcessAction::Terminate | ProcessAction::ForceStop)
    }

    /// Whether the action can destroy unsaved work.
    pub fn is_destructive(self) -> bool {
        matches!(self, ProcessAction::Terminate | ProcessAction::ForceStop)
    }

    /// The signal an implementation delivers for this action, where one
    /// applies. Renicing is not a signal.
    pub fn signal(self) -> Option<SignalKind> {
        match self {
            ProcessAction::Terminate => Some(SignalKind::Terminate),
            ProcessAction::ForceStop => Some(SignalKind::Kill),
            ProcessAction::Pause => Some(SignalKind::Stop),
            ProcessAction::Resume => Some(SignalKind::Continue),
            ProcessAction::SetNice(_) => None,
        }
    }
}

/// The abstract signals Better Monitor is allowed to deliver.
///
/// Naming them here, as a closed set, is what stops an arbitrary signal number
/// from ever reaching a syscall from the presentation layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Terminate,
    Kill,
    Stop,
    Continue,
}

/// The process an action would apply to.
///
/// The start token is the process's start time as the kernel records it. It is
/// carried so a controller can refuse to act on a recycled PID: between the
/// table being drawn and a menu item being clicked, the PID may belong to
/// something else entirely.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessTarget {
    pub pid: u32,
    pub name: String,
    /// `None` when the owning UID could not be read.
    pub owner_uid: Option<u32>,
    /// `None` when the process's start time could not be read.
    pub start_token: Option<u64>,
    /// `None` when the current nice value could not be read.
    pub current_nice: Option<i32>,
}

impl ProcessTarget {
    pub fn new(pid: u32, name: impl Into<String>) -> Self {
        Self {
            pid,
            name: name.into(),
            owner_uid: None,
            start_token: None,
            current_nice: None,
        }
    }

    pub fn owned_by(mut self, uid: u32) -> Self {
        self.owner_uid = Some(uid);
        self
    }

    pub fn started_at(mut self, token: u64) -> Self {
        self.start_token = Some(token);
        self
    }

    pub fn with_nice(mut self, nice: i32) -> Self {
        self.current_nice = Some(nice);
        self
    }
}

/// Why an action is not offered.
///
/// Every variant carries what the user needs in order to understand the
/// refusal without being told to try again as root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRefusal {
    /// The process belongs to another user. Acting on it needs a privileged
    /// boundary that this ticket deliberately does not build, so it is shown
    /// as unavailable rather than attempted and failed.
    NotOwnedByCurrentUser { owner_uid: Option<u32> },
    /// The owning user could not be read, so ownership cannot be proven. An
    /// unprovable claim is not a permission.
    OwnershipUnknown,
    /// A process Better Monitor refuses to signal at all, such as `init` or
    /// the monitor's own window process.
    ProtectedProcess { detail: String },
    /// Lowering a nice value raises priority, which needs `CAP_SYS_NICE`.
    RaisingPriorityNeedsPrivilege { current: i32, requested: i32 },
    /// The requested nice value is outside what the kernel accepts.
    NiceOutOfRange { requested: i32 },
    /// The current nice value is unreadable, so no bounded change can be
    /// proven safe.
    CurrentPriorityUnknown,
}

/// Whether an action can be offered for a target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionAvailability {
    Available,
    Refused(ActionRefusal),
}

impl ActionAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, ActionAvailability::Available)
    }

    pub fn refusal(&self) -> Option<&ActionRefusal> {
        match self {
            ActionAvailability::Available => None,
            ActionAvailability::Refused(reason) => Some(reason),
        }
    }
}

/// What actually happened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    /// The kernel accepted the signal. Whether the process exits, and when, is
    /// its own decision; nothing here claims it has.
    SignalAccepted {
        signal: SignalKind,
    },
    PriorityChanged {
        from: Option<i32>,
        to: i32,
    },
}

/// Why an attempted action failed.
///
/// These are the outcomes a pre-flight check cannot rule out, because they
/// describe a race with the running system rather than a policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionError {
    /// `ESRCH`. The process exited between the table being drawn and the
    /// action being sent. That is a normal outcome, not a fault.
    ProcessDisappeared { pid: u32 },
    /// `EPERM` or `EACCES` from the kernel despite the pre-flight check
    /// allowing it — a process that changed owner, or a security module.
    PermissionDenied { detail: String },
    /// `EINVAL`, or a request the controller rejects as malformed.
    InvalidRequest { detail: String },
    /// The platform has no way to perform this action.
    Unsupported { detail: String },
    /// Anything else the operating system reported.
    Failed { detail: String },
}

impl fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::ProcessDisappeared { pid } => {
                write!(formatter, "process {pid} no longer exists")
            }
            ActionError::PermissionDenied { detail } => write!(formatter, "denied: {detail}"),
            ActionError::InvalidRequest { detail } => write!(formatter, "invalid: {detail}"),
            ActionError::Unsupported { detail } => write!(formatter, "unsupported: {detail}"),
            ActionError::Failed { detail } => write!(formatter, "failed: {detail}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// Something that can inspect and control processes.
///
/// The trait is the only way the GUI reaches a process. An implementation may
/// use syscalls, or may be a fake; nothing above this line can tell.
pub trait ProcessController {
    /// The user this controller acts as. `None` when it cannot be determined,
    /// which makes every ownership check refuse rather than assume.
    fn current_uid(&self) -> Option<u32>;

    /// The PID of the process the controller runs inside, so it can refuse to
    /// signal itself.
    fn own_pid(&self) -> u32;

    /// Whether an action would be offered, decided before anything is
    /// attempted.
    fn availability(&self, target: &ProcessTarget, action: ProcessAction) -> ActionAvailability {
        unprivileged_availability(self.current_uid(), self.own_pid(), target, action)
    }

    /// Carry the action out. Only called after `availability` allowed it.
    fn perform(
        &mut self,
        target: &ProcessTarget,
        action: ProcessAction,
    ) -> Result<ActionOutcome, ActionError>;
}

/// The single implementation of the unprivileged action policy.
///
/// Both the real controller and every fake share it, so a test that proves a
/// refusal is proving the same code production runs. `init` and the monitor's
/// own process are protected because signalling either is never what a user
/// meant by "stop this app".
pub fn unprivileged_availability(
    current_uid: Option<u32>,
    own_pid: u32,
    target: &ProcessTarget,
    action: ProcessAction,
) -> ActionAvailability {
    if target.pid == 1 {
        return ActionAvailability::Refused(ActionRefusal::ProtectedProcess {
            detail: "pid 1 is the init system".into(),
        });
    }
    if target.pid == own_pid {
        return ActionAvailability::Refused(ActionRefusal::ProtectedProcess {
            detail: "this is Better Monitor's own process".into(),
        });
    }

    let Some(current_uid) = current_uid else {
        return ActionAvailability::Refused(ActionRefusal::OwnershipUnknown);
    };
    match target.owner_uid {
        None => return ActionAvailability::Refused(ActionRefusal::OwnershipUnknown),
        Some(owner) if owner != current_uid => {
            return ActionAvailability::Refused(ActionRefusal::NotOwnedByCurrentUser {
                owner_uid: Some(owner),
            });
        }
        Some(_) => {}
    }

    if let ProcessAction::SetNice(requested) = action {
        if !(NICE_MINIMUM..=NICE_MAXIMUM).contains(&requested) {
            return ActionAvailability::Refused(ActionRefusal::NiceOutOfRange { requested });
        }
        let Some(current) = target.current_nice else {
            return ActionAvailability::Refused(ActionRefusal::CurrentPriorityUnknown);
        };
        if requested < current {
            return ActionAvailability::Refused(ActionRefusal::RaisingPriorityNeedsPrivilege {
                current,
                requested,
            });
        }
    }

    ActionAvailability::Available
}

/// Controllers that exist so tests can drive every path without a real
/// process.
pub mod testing {
    use super::*;
    use std::collections::HashMap;

    /// A controller that records what it was asked to do and answers with
    /// whatever the test configured.
    ///
    /// It shares the production availability policy, so a test that asserts a
    /// refusal here is asserting the rule the real controller applies.
    #[derive(Debug, Default)]
    pub struct RecordingController {
        current_uid: Option<u32>,
        own_pid: u32,
        failures: HashMap<(u32, &'static str), ActionError>,
        pub performed: Vec<(u32, ProcessAction)>,
    }

    impl RecordingController {
        pub fn new(current_uid: u32, own_pid: u32) -> Self {
            Self {
                current_uid: Some(current_uid),
                own_pid,
                failures: HashMap::new(),
                performed: Vec::new(),
            }
        }

        /// A controller that cannot tell which user it is.
        pub fn without_identity(own_pid: u32) -> Self {
            Self {
                current_uid: None,
                own_pid,
                failures: HashMap::new(),
                performed: Vec::new(),
            }
        }

        /// Make one action against one PID fail, so the honest surfacing of a
        /// raced `ESRCH` or an `EPERM` can be tested.
        pub fn fail(mut self, pid: u32, action: ProcessAction, error: ActionError) -> Self {
            self.failures.insert((pid, action.key()), error);
            self
        }
    }

    impl ProcessController for RecordingController {
        fn current_uid(&self) -> Option<u32> {
            self.current_uid
        }

        fn own_pid(&self) -> u32 {
            self.own_pid
        }

        fn perform(
            &mut self,
            target: &ProcessTarget,
            action: ProcessAction,
        ) -> Result<ActionOutcome, ActionError> {
            if let Some(error) = self.failures.get(&(target.pid, action.key())) {
                return Err(error.clone());
            }
            self.performed.push((target.pid, action));
            match action {
                ProcessAction::SetNice(value) => Ok(ActionOutcome::PriorityChanged {
                    from: target.current_nice,
                    to: value,
                }),
                other => Ok(ActionOutcome::SignalAccepted {
                    signal: other.signal().expect("every non-nice action has a signal"),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::RecordingController;
    use super::*;

    fn target() -> ProcessTarget {
        ProcessTarget::new(4242, "gedit")
            .owned_by(1000)
            .started_at(9_000)
            .with_nice(0)
    }

    #[test]
    fn only_the_two_destructive_actions_ask_for_confirmation() {
        assert!(ProcessAction::Terminate.requires_confirmation());
        assert!(ProcessAction::ForceStop.requires_confirmation());
        assert!(!ProcessAction::Pause.requires_confirmation());
        assert!(!ProcessAction::Resume.requires_confirmation());
        assert!(!ProcessAction::SetNice(5).requires_confirmation());
    }

    #[test]
    fn every_action_that_is_a_signal_names_one_and_renice_does_not() {
        assert_eq!(
            ProcessAction::Terminate.signal(),
            Some(SignalKind::Terminate)
        );
        assert_eq!(ProcessAction::ForceStop.signal(), Some(SignalKind::Kill));
        assert_eq!(ProcessAction::Pause.signal(), Some(SignalKind::Stop));
        assert_eq!(ProcessAction::Resume.signal(), Some(SignalKind::Continue));
        assert_eq!(ProcessAction::SetNice(1).signal(), None);
    }

    #[test]
    fn an_own_process_allows_every_action() {
        let controller = RecordingController::new(1000, 55);
        for action in [
            ProcessAction::Terminate,
            ProcessAction::ForceStop,
            ProcessAction::Pause,
            ProcessAction::Resume,
            ProcessAction::SetNice(10),
        ] {
            assert_eq!(
                controller.availability(&target(), action),
                ActionAvailability::Available,
                "{action:?}"
            );
        }
    }

    #[test]
    fn another_users_process_is_refused_with_the_owner_named() {
        let controller = RecordingController::new(1000, 55);
        let other = ProcessTarget::new(9, "systemd-resolved")
            .owned_by(101)
            .with_nice(0);
        assert_eq!(
            controller.availability(&other, ProcessAction::Terminate),
            ActionAvailability::Refused(ActionRefusal::NotOwnedByCurrentUser {
                owner_uid: Some(101)
            })
        );
    }

    #[test]
    fn ownership_that_cannot_be_read_is_refused_rather_than_assumed() {
        let controller = RecordingController::new(1000, 55);
        let unreadable = ProcessTarget::new(9, "unknown").with_nice(0);
        assert_eq!(
            controller.availability(&unreadable, ProcessAction::Terminate),
            ActionAvailability::Refused(ActionRefusal::OwnershipUnknown)
        );

        let no_identity = RecordingController::without_identity(55);
        assert_eq!(
            no_identity.availability(&target(), ProcessAction::Terminate),
            ActionAvailability::Refused(ActionRefusal::OwnershipUnknown)
        );
    }

    #[test]
    fn init_and_the_monitors_own_process_are_never_signalled() {
        let controller = RecordingController::new(0, 55);
        let init = ProcessTarget::new(1, "systemd").owned_by(0).with_nice(0);
        assert!(matches!(
            controller.availability(&init, ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::ProtectedProcess { .. })
        ));
        let itself = ProcessTarget::new(55, "monitor-gui")
            .owned_by(0)
            .with_nice(0);
        assert!(matches!(
            controller.availability(&itself, ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::ProtectedProcess { .. })
        ));
    }

    #[test]
    fn lowering_a_nice_value_is_refused_because_it_raises_priority() {
        let controller = RecordingController::new(1000, 55);
        let running = target().with_nice(5);
        assert_eq!(
            controller.availability(&running, ProcessAction::SetNice(0)),
            ActionAvailability::Refused(ActionRefusal::RaisingPriorityNeedsPrivilege {
                current: 5,
                requested: 0
            })
        );
        assert!(
            controller
                .availability(&running, ProcessAction::SetNice(10))
                .is_available()
        );
        assert!(
            controller
                .availability(&running, ProcessAction::SetNice(5))
                .is_available()
        );
    }

    #[test]
    fn a_nice_value_outside_the_kernels_range_is_refused_before_the_syscall() {
        let controller = RecordingController::new(1000, 55);
        assert_eq!(
            controller.availability(&target(), ProcessAction::SetNice(20)),
            ActionAvailability::Refused(ActionRefusal::NiceOutOfRange { requested: 20 })
        );
        assert_eq!(
            controller.availability(&target(), ProcessAction::SetNice(-21)),
            ActionAvailability::Refused(ActionRefusal::NiceOutOfRange { requested: -21 })
        );
    }

    #[test]
    fn an_unreadable_current_priority_blocks_a_bounded_change() {
        let controller = RecordingController::new(1000, 55);
        let unknown = ProcessTarget::new(4242, "gedit").owned_by(1000);
        assert_eq!(
            controller.availability(&unknown, ProcessAction::SetNice(10)),
            ActionAvailability::Refused(ActionRefusal::CurrentPriorityUnknown)
        );
        // Signals do not depend on the priority being readable.
        assert!(
            controller
                .availability(&unknown, ProcessAction::Terminate)
                .is_available()
        );
    }

    #[test]
    fn a_delivered_signal_is_reported_as_accepted_and_not_as_an_exit() {
        let mut controller = RecordingController::new(1000, 55);
        let outcome = controller
            .perform(&target(), ProcessAction::Terminate)
            .expect("an own process accepts a terminate");
        assert_eq!(
            outcome,
            ActionOutcome::SignalAccepted {
                signal: SignalKind::Terminate
            }
        );
        assert_eq!(controller.performed, vec![(4242, ProcessAction::Terminate)]);
    }

    #[test]
    fn a_process_that_exited_first_is_reported_as_a_race_not_a_fault() {
        let mut controller = RecordingController::new(1000, 55).fail(
            4242,
            ProcessAction::Terminate,
            ActionError::ProcessDisappeared { pid: 4242 },
        );
        let error = controller
            .perform(&target(), ProcessAction::Terminate)
            .expect_err("the configured failure must surface");
        assert_eq!(error, ActionError::ProcessDisappeared { pid: 4242 });
        assert!(controller.performed.is_empty());
        assert!(error.to_string().contains("no longer exists"));
    }

    #[test]
    fn renicing_reports_where_it_moved_from_and_to() {
        let mut controller = RecordingController::new(1000, 55);
        let outcome = controller
            .perform(&target(), ProcessAction::SetNice(12))
            .expect("a downward nice change is allowed");
        assert_eq!(
            outcome,
            ActionOutcome::PriorityChanged {
                from: Some(0),
                to: 12
            }
        );
    }
}

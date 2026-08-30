//! The Linux end of process control.
//!
//! This is the only crate in Better Monitor that calls a process-control
//! syscall, and it does exactly four of them: `kill(2)`, `setpriority(2)`,
//! `getpriority(2)`, and `geteuid(2)`. There is no shell, no `kill` binary, no
//! privileged handle, and no path that can be reached with a signal number
//! that did not come from [`monitor_core::ProcessAction`].
//!
//! Everything about *whether* an action is allowed lives in `monitor-core`, so
//! this crate cannot develop a second, more permissive policy. What it adds is
//! the part only the operating system can answer: which user this process is,
//! and what the kernel said when the syscall ran.
//!
//! ## Scope
//!
//! Own-user processes only. An action on another user's process, or one
//! needing elevation, is refused before anything is attempted, with the reason
//! attached. The narrow polkit-reviewed boundary that would allow those is a
//! separate piece of work; nothing here reaches for one, and the GUI never
//! runs as root.

use monitor_core::{
    ActionError, ActionOutcome, ProcessAction, ProcessController, ProcessTarget, SignalKind,
};
use std::io;

/// Maps an abstract signal to the number the kernel expects.
///
/// This function is the whole reason the rest of Better Monitor never sees a
/// signal number.
fn signal_number(signal: SignalKind) -> libc::c_int {
    match signal {
        SignalKind::Terminate => libc::SIGTERM,
        SignalKind::Kill => libc::SIGKILL,
        SignalKind::Stop => libc::SIGSTOP,
        SignalKind::Continue => libc::SIGCONT,
    }
}

fn last_error(pid: u32) -> ActionError {
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => ActionError::ProcessDisappeared { pid },
        Some(libc::EPERM) | Some(libc::EACCES) => ActionError::PermissionDenied {
            detail: error.to_string(),
        },
        Some(libc::EINVAL) => ActionError::InvalidRequest {
            detail: error.to_string(),
        },
        _ => ActionError::Failed {
            detail: error.to_string(),
        },
    }
}

/// Process control against the running kernel.
#[derive(Clone, Copy, Debug)]
pub struct LinuxProcessController {
    current_uid: u32,
    own_pid: u32,
}

impl LinuxProcessController {
    /// A controller acting as whoever this process already is. It never
    /// escalates and never holds a privileged connection.
    pub fn for_current_process() -> Self {
        Self {
            // SAFETY: `geteuid` takes no arguments, touches no memory, and is
            // documented as always succeeding.
            current_uid: unsafe { libc::geteuid() },
            own_pid: std::process::id(),
        }
    }

    /// The kernel's current nice value for a process.
    ///
    /// `getpriority` legitimately returns -1, so `errno` has to be cleared
    /// first to tell that apart from an error.
    pub fn read_nice(&self, pid: u32) -> Result<i32, ActionError> {
        unsafe {
            *libc::__errno_location() = 0;
        }
        // SAFETY: `PRIO_PROCESS` with a PID is the documented form; the call
        // reads no caller memory.
        let value = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid) };
        if value == -1 {
            let errno = unsafe { *libc::__errno_location() };
            if errno != 0 {
                return Err(last_error(pid));
            }
        }
        Ok(value)
    }

    fn deliver(&self, pid: u32, signal: SignalKind) -> Result<ActionOutcome, ActionError> {
        // SAFETY: both arguments are plain integers and the signal number came
        // from the closed `SignalKind` set, so no arbitrary signal can reach
        // here.
        let result = unsafe { libc::kill(pid as libc::pid_t, signal_number(signal)) };
        if result == 0 {
            Ok(ActionOutcome::SignalAccepted { signal })
        } else {
            Err(last_error(pid))
        }
    }
}

impl ProcessController for LinuxProcessController {
    fn current_uid(&self) -> Option<u32> {
        Some(self.current_uid)
    }

    fn own_pid(&self) -> u32 {
        self.own_pid
    }

    fn perform(
        &mut self,
        target: &ProcessTarget,
        action: ProcessAction,
    ) -> Result<ActionOutcome, ActionError> {
        // The policy is asked again here rather than trusted from the caller.
        // A view that forgot to check, or a target that changed owner since it
        // was drawn, must not be able to reach a syscall.
        if let Some(refusal) = self.availability(target, action).refusal() {
            return Err(ActionError::PermissionDenied {
                detail: format!("{refusal:?}"),
            });
        }
        match action {
            ProcessAction::SetNice(value) => {
                let from = self.read_nice(target.pid).ok();
                // SAFETY: plain integer arguments to a documented syscall.
                let result = unsafe {
                    libc::setpriority(libc::PRIO_PROCESS, target.pid, value as libc::c_int)
                };
                if result != 0 {
                    return Err(last_error(target.pid));
                }
                Ok(ActionOutcome::PriorityChanged { from, to: value })
            }
            other => self.deliver(
                target.pid,
                other
                    .signal()
                    .expect("every non-priority action names a signal"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{ActionAvailability, ActionRefusal};

    #[test]
    fn every_signal_maps_to_the_number_the_kernel_documents() {
        assert_eq!(signal_number(SignalKind::Terminate), libc::SIGTERM);
        assert_eq!(signal_number(SignalKind::Kill), libc::SIGKILL);
        assert_eq!(signal_number(SignalKind::Stop), libc::SIGSTOP);
        assert_eq!(signal_number(SignalKind::Continue), libc::SIGCONT);
    }

    #[test]
    fn the_controller_acts_as_the_current_unprivileged_process() {
        let controller = LinuxProcessController::for_current_process();
        assert_eq!(controller.own_pid(), std::process::id());
        assert!(controller.current_uid().is_some());
    }

    #[test]
    fn the_controller_refuses_to_signal_itself() {
        let controller = LinuxProcessController::for_current_process();
        let uid = controller.current_uid().unwrap();
        let target = ProcessTarget::new(std::process::id(), "monitor-actions-linux-test")
            .owned_by(uid)
            .with_nice(0);
        assert!(matches!(
            controller.availability(&target, ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::ProtectedProcess { .. })
        ));
    }

    #[test]
    fn a_refused_action_never_reaches_a_syscall() {
        let mut controller = LinuxProcessController::for_current_process();
        // pid 1 is protected, so this must fail on the policy rather than on
        // the kernel returning EPERM.
        let target = ProcessTarget::new(1, "systemd").owned_by(0).with_nice(0);
        let error = controller
            .perform(&target, ProcessAction::ForceStop)
            .expect_err("init is protected");
        assert!(matches!(error, ActionError::PermissionDenied { .. }));
    }

    #[test]
    fn the_current_processes_own_nice_value_is_readable() {
        let controller = LinuxProcessController::for_current_process();
        let nice = controller
            .read_nice(std::process::id())
            .expect("a process can always read its own priority");
        assert!((-20..=19).contains(&nice));
    }

    #[test]
    fn reading_the_priority_of_a_process_that_does_not_exist_is_reported_honestly() {
        let controller = LinuxProcessController::for_current_process();
        // 0x7FFF_FFFE is above any plausible pid_max, so nothing owns it.
        let error = controller
            .read_nice(0x7FFF_FFFE)
            .expect_err("no such process");
        assert!(matches!(error, ActionError::ProcessDisappeared { .. }));
    }
}

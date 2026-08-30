//! Against a real child process.
//!
//! Every other action test drives a fake controller, which proves the policy
//! but not the syscalls. This one spawns a process the test owns, pauses it,
//! resumes it, renices it, and terminates it, reading `/proc` after each step
//! to check that the kernel actually did what was asked. It is the only place
//! the syscall path is exercised end to end.

use monitor_actions_linux::LinuxProcessController;
use monitor_core::{
    ActionError, ActionOutcome, ProcessAction, ProcessController, ProcessTarget, SignalKind,
};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// A child that is always cleaned up, even when an assertion fails.
struct Sleeper(Option<Child>);

impl Sleeper {
    fn spawn() -> Self {
        let child = Command::new("sleep")
            .arg("120")
            .spawn()
            .expect("the test host must provide `sleep`");
        Self(Some(child))
    }

    fn pid(&self) -> u32 {
        self.0.as_ref().expect("the child is alive").id()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The single-letter scheduler state from `/proc/[pid]/stat`, read the same
/// way the collector does: split at the last `)` so a command name containing
/// spaces cannot shift the fields.
fn state_of(pid: u32) -> Option<char> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = raw.rfind(')')?;
    raw[close + 1..].split_whitespace().next()?.chars().next()
}

/// Wait for a state, because signal delivery is not synchronous.
fn wait_for_state(pid: u32, expected: char) -> char {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = state_of(pid).unwrap_or('?');
    while Instant::now() < deadline {
        if last == expected {
            return last;
        }
        sleep(Duration::from_millis(20));
        last = state_of(pid).unwrap_or('?');
    }
    last
}

fn target(controller: &LinuxProcessController, pid: u32) -> ProcessTarget {
    ProcessTarget::new(pid, "sleep")
        .owned_by(controller.current_uid().expect("a uid"))
        .with_nice(controller.read_nice(pid).unwrap_or(0))
}

#[test]
fn a_child_process_can_be_paused_resumed_and_then_terminated() {
    let sleeper = Sleeper::spawn();
    let pid = sleeper.pid();
    let mut controller = LinuxProcessController::for_current_process();

    // It starts schedulable.
    assert!(
        matches!(wait_for_state(pid, 'S'), 'S' | 'R'),
        "a freshly spawned sleep should be sleeping or running"
    );

    let paused = controller
        .perform(&target(&controller, pid), ProcessAction::Pause)
        .expect("pausing an own process is allowed");
    assert_eq!(
        paused,
        ActionOutcome::SignalAccepted {
            signal: SignalKind::Stop
        }
    );
    assert_eq!(
        wait_for_state(pid, 'T'),
        'T',
        "the kernel must report the process as stopped"
    );

    controller
        .perform(&target(&controller, pid), ProcessAction::Resume)
        .expect("resuming an own process is allowed");
    assert!(
        matches!(wait_for_state(pid, 'S'), 'S' | 'R'),
        "the process must be schedulable again"
    );

    controller
        .perform(&target(&controller, pid), ProcessAction::Terminate)
        .expect("terminating an own process is allowed");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        // A terminated child becomes a zombie until it is reaped, and the
        // `Sleeper` drop does the reaping.
        match state_of(pid) {
            None | Some('Z') => break,
            _ => sleep(Duration::from_millis(20)),
        }
    }
    assert!(
        matches!(state_of(pid), None | Some('Z')),
        "SIGTERM must actually end `sleep`"
    );
}

#[test]
fn a_child_process_can_be_moved_down_the_scheduler_queue() {
    let sleeper = Sleeper::spawn();
    let pid = sleeper.pid();
    let mut controller = LinuxProcessController::for_current_process();
    let before = controller
        .read_nice(pid)
        .expect("an own process's priority");

    let requested = (before + 5).min(19);
    let outcome = controller
        .perform(&target(&controller, pid), ProcessAction::SetNice(requested))
        .expect("raising a nice value needs no privilege");
    assert_eq!(
        outcome,
        ActionOutcome::PriorityChanged {
            from: Some(before),
            to: requested
        }
    );
    assert_eq!(controller.read_nice(pid).unwrap(), requested);

    // And the reverse is refused rather than attempted, because lowering the
    // nice value would need CAP_SYS_NICE.
    let raised = ProcessTarget::new(pid, "sleep")
        .owned_by(controller.current_uid().unwrap())
        .with_nice(requested);
    let error = controller
        .perform(&raised, ProcessAction::SetNice(before))
        .expect_err("raising priority is out of bounds for this ticket");
    assert!(matches!(error, ActionError::PermissionDenied { .. }));
    assert_eq!(controller.read_nice(pid).unwrap(), requested);
}

#[test]
fn terminating_a_process_that_already_exited_reports_the_race_honestly() {
    let mut sleeper = Sleeper::spawn();
    let pid = sleeper.pid();
    let mut controller = LinuxProcessController::for_current_process();
    let target = target(&controller, pid);

    // Reap it so the PID is genuinely gone rather than a zombie, which still
    // accepts signals.
    let mut child = sleeper.0.take().expect("the child is alive");
    let _ = child.kill();
    let _ = child.wait();

    let error = controller
        .perform(&target, ProcessAction::Terminate)
        .expect_err("the process is gone");
    assert_eq!(error, ActionError::ProcessDisappeared { pid });
    assert!(error.to_string().contains("no longer exists"));
}

#[test]
fn another_users_process_is_refused_without_a_syscall_being_attempted() {
    let mut controller = LinuxProcessController::for_current_process();
    let sleeper = Sleeper::spawn();
    let mine = sleeper.pid();
    let uid = controller.current_uid().unwrap();
    // Claim a different owner for a process that really is ours. If the
    // refusal came from the kernel rather than from the policy, the process
    // would die and this test would be the one that noticed.
    let mislabelled = ProcessTarget::new(mine, "sleep")
        .owned_by(uid.wrapping_add(1))
        .with_nice(0);
    let error = controller
        .perform(&mislabelled, ProcessAction::ForceStop)
        .expect_err("another user's process is out of scope");
    assert!(matches!(error, ActionError::PermissionDenied { .. }));
    assert!(
        matches!(state_of(mine), Some('S') | Some('R')),
        "the refusal must not have signalled anything"
    );
}

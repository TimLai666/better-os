//! The Linux end of process control.
//!
//! This is the only crate in Better Monitor that calls a process-control
//! syscall: `pidfd_open(2)` and `pidfd_send_signal(2)` (with `kill(2)` where
//! the kernel predates them), `setpriority(2)`, `getpriority(2)`, and
//! `geteuid(2)`. There is no shell, no `kill` binary, no privileged handle, and
//! no path that can be reached with a signal number that did not come from
//! [`monitor_core::ProcessAction`].
//!
//! ## A PID is not an identity
//!
//! The window acts on a snapshot, and between the snapshot and the click the
//! process may have exited and its PID been given to something else. Before
//! any signal or priority change, the target's start time and owner are read
//! back from `/proc` and compared with the snapshot's; a mismatch is reported
//! as the process having gone. With pidfds the comparison is made after the
//! descriptor is open, so the signal reaches exactly the process that was
//! checked. This crate is also the window's only reader of `/proc` for an
//! action: the identity check, the "end process tree" guard, and the program
//! path behind "Open file location".
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
use std::cell::Cell;
use std::ffi::OsString;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod session;

pub use session::{ProcReader, ScanRequest, SessionEnv, SessionReader};

use monitor_core::session::{LoginSession, SessionFacts, SessionMember};

/// `USER_HZ`, the unit `/proc/[pid]/stat` counts start time in. It is part of
/// the kernel's user-space ABI and is 100 on every Linux architecture.
const USER_HZ: u64 = 100;

/// What `/proc` says about a process now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProcess {
    pub pid: u32,
    /// The command name, as the kernel records it (`comm`).
    pub name: String,
    pub parent: Option<u32>,
    pub session: Option<u32>,
    /// Start time in hundredths of a second since the Unix epoch, the same
    /// token `ProcessFacts::action_target` carries.
    pub start_token: Option<u64>,
    /// The real user id, from `status`.
    pub uid: Option<u32>,
}

/// Parse `/proc/[pid]/stat`. The command name is everything between the
/// first `(` and the last `)`, so a name holding spaces or parentheses
/// cannot shift the fields after it.
/// The fields of `stat` an action needs: name, parent, session, start ticks.
type StatFields = (String, Option<u32>, Option<u32>, Option<u64>);

pub(crate) fn parse_stat(text: &str) -> Option<StatFields> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text.get(open + 1..close)?.to_string();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    // After the name: state(0) ppid(1) pgrp(2) session(3) … starttime(19).
    let parent = fields.get(1).and_then(|v| v.parse().ok());
    let session = fields.get(3).and_then(|v| v.parse().ok());
    let start = fields.get(19).and_then(|v| v.parse().ok());
    Some((name, parent, session, start))
}

fn boot_seconds(root: &Path) -> Option<u64> {
    std::fs::read_to_string(root.join("stat"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|value| value.trim().parse().ok())
}

pub(crate) fn real_uid(root: &Path, pid: u32) -> Option<u32> {
    std::fs::read_to_string(root.join(pid.to_string()).join("status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// Strip the ` (deleted)` the kernel appends to the link of a program whose
/// file was replaced, working on the raw bytes so a path that is not UTF-8
/// survives unchanged.
pub fn without_deleted_suffix(path: OsString) -> OsString {
    const SUFFIX: &[u8] = b" (deleted)";
    let bytes = path.as_bytes();
    if bytes.ends_with(SUFFIX) {
        OsString::from_vec(bytes[..bytes.len() - SUFFIX.len()].to_vec())
    } else {
        path
    }
}

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
///
/// The session guard is answered two ways. The window's interface thread
/// asks [`LinuxProcessController::snapshot_view`], which answers from the
/// [`SessionFacts`] the window read off that thread and handed in
/// ([`LinuxProcessController::set_session`]) and never touches `/proc`.
/// [`ProcessController::availability`] and `perform` on the controller itself
/// read the facts again, fresh, for the processes concerned, right before a
/// signal goes out.
#[derive(Clone, Debug)]
pub struct LinuxProcessController {
    current_uid: u32,
    own_pid: u32,
    proc_root: PathBuf,
    session_env: SessionEnv,
    reader: Arc<dyn SessionReader>,
    /// The facts the window last read, for answers on its interface thread.
    snapshot: Option<Arc<SessionFacts>>,
    /// Set when such an answer met a process the snapshot does not cover,
    /// so the window reads the facts again.
    stale: Cell<bool>,
}

impl LinuxProcessController {
    /// A controller acting as whoever this process already is. It never
    /// escalates and never holds a privileged connection.
    pub fn for_current_process() -> Self {
        // SAFETY: `geteuid` takes no arguments, touches no memory, and is
        // documented as always succeeding.
        let current_uid = unsafe { libc::geteuid() };
        let mut controller = Self {
            current_uid,
            own_pid: std::process::id(),
            proc_root: PathBuf::from("/proc"),
            session_env: SessionEnv::from_process(),
            reader: Arc::new(ProcReader {
                proc_root: PathBuf::new(),
                env: SessionEnv::default(),
                uid: current_uid,
                own_pid: 0,
            }),
            snapshot: None,
            stale: Cell::new(false),
        };
        controller.reader = Arc::new(controller.proc_reader());
        controller
    }

    fn proc_reader(&self) -> ProcReader {
        ProcReader {
            proc_root: self.proc_root.clone(),
            env: self.session_env.clone(),
            uid: self.current_uid,
            own_pid: self.own_pid,
        }
    }

    /// The same controller reading a different `/proc`, for tests that need
    /// a process whose recorded identity differs from the running one.
    pub fn with_proc_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.proc_root = root.into();
        self.reader = Arc::new(self.proc_reader());
        self.snapshot = None;
        self
    }

    /// The same controller finding the session's sockets from `env`
    /// rather than its own environment.
    pub fn with_session_env(mut self, env: SessionEnv) -> Self {
        self.session_env = env;
        self.reader = Arc::new(self.proc_reader());
        self.snapshot = None;
        self
    }

    /// The same controller reading session facts with `reader` (a test's).
    pub fn with_reader(mut self, reader: Arc<dyn SessionReader>) -> Self {
        self.reader = reader;
        self
    }

    /// The reader, for the window to run off its interface thread.
    pub fn reader(&self) -> Arc<dyn SessionReader> {
        self.reader.clone()
    }

    /// Adopt facts the window read off its interface thread.
    pub fn set_session(&mut self, facts: SessionFacts) {
        self.snapshot = Some(Arc::new(facts));
        self.stale.set(false);
    }

    /// The facts the window holds, if it has read any.
    pub fn session_snapshot(&self) -> Option<&SessionFacts> {
        self.snapshot.as_deref()
    }

    /// The person's logind sessions as last handed in.
    pub fn login_sessions(&self) -> Option<Vec<LoginSession>> {
        self.snapshot
            .as_ref()
            .and_then(|facts| facts.sessions.clone())
    }

    /// Whether the window should read the facts again: none yet, an answer
    /// met a process they do not cover, or a process they name as holding a
    /// socket is no longer among `live` (the compositor and the bus do not
    /// change while the window lives, so nothing else triggers a read).
    pub fn needs_session_read(&self, live: &std::collections::BTreeSet<u32>) -> bool {
        match &self.snapshot {
            None => true,
            Some(facts) => {
                self.stale.get() || facts.holders().iter().any(|pid| !live.contains(pid))
            }
        }
    }

    /// The controller as the window's interface thread may ask it: from the
    /// snapshot only, never reading `/proc`, never signalling.
    pub fn snapshot_view(&self) -> SnapshotView<'_> {
        SnapshotView { controller: self }
    }

    /// What the snapshot says about one process, or what the target alone
    /// says when the snapshot does not cover it (which asks for a re-read).
    fn snapshot_member(&self, facts: &SessionFacts, pid: u32, name: &str) -> SessionMember {
        facts.members.get(&pid).cloned().unwrap_or_else(|| {
            self.stale.set(true);
            SessionMember {
                pid,
                name: name.to_string(),
                parent: None,
                cgroup: None,
            }
        })
    }

    fn empty_facts(&self) -> SessionFacts {
        SessionFacts {
            uid: self.current_uid,
            ..SessionFacts::default()
        }
    }

    /// The session refusal from the snapshot, for the interface thread.
    fn snapshot_refusal(&self, target: &ProcessTarget) -> Option<monitor_core::ActionRefusal> {
        let empty;
        let facts = match &self.snapshot {
            Some(facts) => facts.as_ref(),
            None => {
                self.stale.set(true);
                empty = self.empty_facts();
                &empty
            }
        };
        let member = self.snapshot_member(facts, target.pid, &target.name);
        monitor_core::session::refusal(&member, facts)
    }

    /// Whether a tree may be ended, from the snapshot, for the interface
    /// thread.
    pub fn tree_refusal_snapshot(
        &self,
        root: u32,
        pids: &[u32],
        name_of: impl Fn(u32) -> String,
    ) -> Option<monitor_core::ActionRefusal> {
        let empty;
        let facts = match &self.snapshot {
            Some(facts) => facts.as_ref(),
            None => {
                self.stale.set(true);
                empty = self.empty_facts();
                &empty
            }
        };
        let members: Vec<SessionMember> = pids
            .iter()
            .map(|pid| self.snapshot_member(facts, *pid, &name_of(*pid)))
            .collect();
        monitor_core::session::tree_refusal(root, &members, facts)
    }

    /// Facts read now for `pids` and their parents: one look at the sockets
    /// and at those processes' descriptors, which is all a decision about
    /// them needs.
    fn fresh_facts(&self, pids: &[u32]) -> SessionFacts {
        let mut scan: Vec<u32> = pids.to_vec();
        for pid in pids {
            if let Some(parent) = self.live(*pid).and_then(|live| live.parent)
                && !scan.contains(&parent)
            {
                scan.push(parent);
            }
        }
        self.reader.read(&ScanRequest {
            pids: Some(scan),
            sessions: self.login_sessions(),
        })
    }

    /// What `/proc` says about a process now, or `None` when it is gone.
    pub fn live(&self, pid: u32) -> Option<LiveProcess> {
        let text =
            std::fs::read_to_string(self.proc_root.join(pid.to_string()).join("stat")).ok()?;
        let (name, parent, session, start_ticks) = parse_stat(&text)?;
        let start_token = match (boot_seconds(&self.proc_root), start_ticks) {
            (Some(boot), Some(ticks)) => Some(boot * 100 + ticks * 100 / USER_HZ),
            _ => None,
        };
        Some(LiveProcess {
            pid,
            name,
            parent,
            session,
            start_token,
            uid: real_uid(&self.proc_root, pid),
        })
    }

    /// The token and owner a target should carry for a process as it is now.
    pub fn target_for(&self, pid: u32) -> Option<ProcessTarget> {
        let live = self.live(pid)?;
        let mut target = ProcessTarget::new(pid, live.name);
        if let Some(uid) = live.uid {
            target = target.owned_by(uid);
        }
        if let Some(token) = live.start_token {
            target = target.started_at(token);
        }
        if let Ok(nice) = self.read_nice(pid) {
            target = target.with_nice(nice);
        }
        Some(target)
    }

    /// Prove the target is still the process the snapshot saw: same start
    /// time (to the hundredth the snapshot was rounded to) and same owner.
    /// Anything else means the PID now names another process, which is
    /// reported as the original having gone.
    pub fn verify(&self, target: &ProcessTarget) -> Result<(), ActionError> {
        let Some(token) = target.start_token else {
            return Err(ActionError::InvalidRequest {
                detail: "the process's start time is unknown, so it cannot be told apart from a newer process with the same PID".into(),
            });
        };
        let gone = || ActionError::ProcessDisappeared { pid: target.pid };
        let live = self.live(target.pid).ok_or_else(gone)?;
        let same_start = live.start_token.is_some_and(|now| now.abs_diff(token) <= 1);
        let same_owner = target.owner_uid.is_none() || live.uid == target.owner_uid;
        if same_start && same_owner {
            Ok(())
        } else {
            Err(gone())
        }
    }

    /// The processes above this one, from its parent up to the first that
    /// has no readable parent.
    pub fn own_ancestors(&self) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut at = self.own_pid;
        while let Some(parent) = self.live(at).and_then(|live| live.parent) {
            if parent == 0 || chain.contains(&parent) || chain.len() > 4096 {
                break;
            }
            chain.push(parent);
            at = parent;
        }
        chain
    }

    /// Whether the process tree rooted at `root` (every member in `pids`)
    /// may be ended, by the one session rule
    /// (`monitor_core::session::tree_refusal`), read now. A member that is
    /// already gone is skipped.
    pub fn tree_refusal(&self, root: u32, pids: &[u32]) -> Option<monitor_core::ActionRefusal> {
        let facts = self.fresh_facts(pids);
        let members: Vec<SessionMember> = pids
            .iter()
            .filter_map(|pid| facts.members.get(pid).cloned())
            .collect();
        monitor_core::session::tree_refusal(root, &members, &facts)
    }

    /// Whether one process may be ended (asked, or at once) as far as the
    /// session is concerned, read now: `None`, or the refusal naming the
    /// part it plays. A process that is already gone is not refused here;
    /// the identity check reports it.
    pub fn session_refusal(&self, pid: u32) -> Option<monitor_core::ActionRefusal> {
        let facts = self.fresh_facts(&[pid]);
        let member = facts.members.get(&pid)?.clone();
        monitor_core::session::refusal(&member, &facts)
    }

    /// The folder holding a process's program, for "Open file location". It
    /// reads one link, `/proc/<pid>/exe`, when asked; the kernel only lets a
    /// person read it for their own processes, so another user's process has
    /// no folder to open.
    pub fn program_folder(&self, pid: u32) -> Option<PathBuf> {
        let link = std::fs::read_link(self.proc_root.join(pid.to_string()).join("exe")).ok()?;
        let program = PathBuf::from(without_deleted_suffix(link.into_os_string()));
        program.parent().map(Path::to_path_buf)
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

    /// Send a signal to exactly the process the target describes.
    ///
    /// With a pidfd, the descriptor is opened first and the identity checked
    /// after, so the PID cannot be recycled between the check and the
    /// signal. On a kernel without pidfds (before 5.3) the check runs
    /// immediately before `kill(2)`, which narrows the window to the time
    /// between two syscalls.
    fn deliver(
        &self,
        target: &ProcessTarget,
        signal: SignalKind,
    ) -> Result<ActionOutcome, ActionError> {
        let pid = target.pid;
        // SAFETY: `pidfd_open` takes a PID and flags and returns a new file
        // descriptor or -1; no caller memory is touched.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        if raw >= 0 {
            // SAFETY: the syscall returned a fresh descriptor this function
            // now owns; it is closed when `pidfd` drops.
            let pidfd = unsafe { OwnedFd::from_raw_fd(raw as libc::c_int) };
            self.verify(target)?;
            // SAFETY: a valid pidfd, a signal from the closed set, no siginfo,
            // no flags.
            let result = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    std::os::fd::AsRawFd::as_raw_fd(&pidfd),
                    signal_number(signal),
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            return if result == 0 {
                Ok(ActionOutcome::SignalAccepted { signal })
            } else {
                Err(last_error(pid))
            };
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOSYS) {
            return Err(last_error(pid));
        }
        self.verify(target)?;
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

    /// The shared policy, and for every action that ends a process (End
    /// task and Force stop) the session guard on top: the session ends with
    /// these processes whichever signal it is (owner, 2026-11-22). Because
    /// `perform` asks this again, the guard cannot be stepped around by a
    /// view that skipped it.
    fn availability(
        &self,
        target: &ProcessTarget,
        action: ProcessAction,
    ) -> monitor_core::ActionAvailability {
        let policy = monitor_core::unprivileged_availability(
            Some(self.current_uid),
            self.own_pid,
            target,
            action,
        );
        if !policy.is_available() || !monitor_core::session::guards(action) {
            return policy;
        }
        match self.session_refusal(target.pid) {
            Some(refusal) => monitor_core::ActionAvailability::Refused(refusal),
            None => policy,
        }
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
                self.verify(target)?;
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
                target,
                other
                    .signal()
                    .expect("every non-priority action names a signal"),
            ),
        }
    }
}

/// The controller as the window's interface thread asks it
/// ([`LinuxProcessController::snapshot_view`]): every answer comes from the
/// snapshot the window handed in, nothing reads `/proc`, and nothing is
/// signalled.
#[derive(Debug)]
pub struct SnapshotView<'a> {
    controller: &'a LinuxProcessController,
}

impl ProcessController for SnapshotView<'_> {
    fn current_uid(&self) -> Option<u32> {
        Some(self.controller.current_uid)
    }

    fn own_pid(&self) -> u32 {
        self.controller.own_pid
    }

    fn availability(
        &self,
        target: &ProcessTarget,
        action: ProcessAction,
    ) -> monitor_core::ActionAvailability {
        let policy = monitor_core::unprivileged_availability(
            Some(self.controller.current_uid),
            self.controller.own_pid,
            target,
            action,
        );
        if !policy.is_available() || !monitor_core::session::guards(action) {
            return policy;
        }
        match self.controller.snapshot_refusal(target) {
            Some(refusal) => monitor_core::ActionAvailability::Refused(refusal),
            None => policy,
        }
    }

    fn perform(
        &mut self,
        _: &ProcessTarget,
        _: ProcessAction,
    ) -> Result<ActionOutcome, ActionError> {
        Err(ActionError::InvalidRequest {
            detail: "the interface thread's view of the controller never signals".into(),
        })
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

    /// A scratch `/proc` that disagrees with the real one on purpose.
    struct FakeProc(PathBuf);

    impl FakeProc {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "monitor-actions-fake-proc-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("stat"), "cpu  1 2 3\nbtime 1000\n").unwrap();
            Self(root)
        }

        /// One process: start ticks since boot, parent, session, real uid.
        fn process(&self, pid: u32, name: &str, parent: u32, session: u32, start: u64, uid: u32) {
            let dir = self.0.join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            let mut fields = vec!["0".to_string(); 20];
            fields[0] = "S".into();
            fields[1] = parent.to_string();
            fields[3] = session.to_string();
            fields[19] = start.to_string();
            std::fs::write(
                dir.join("stat"),
                format!("{pid} ({name}) {}\n", fields.join(" ")),
            )
            .unwrap();
            std::fs::write(
                dir.join("status"),
                format!("Name:\t{name}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
        }
    }

    impl Drop for FakeProc {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_name_with_spaces_and_parentheses_does_not_shift_the_fields() {
        let mut fields = vec!["0"; 20];
        fields[1] = "77";
        fields[3] = "70";
        fields[19] = "4242";
        let text = format!("12 (Web (Content) 2) {}", fields.join(" "));
        let (name, parent, session, start) = parse_stat(&text).unwrap();
        assert_eq!(name, "Web (Content) 2");
        assert_eq!((parent, session, start), (Some(77), Some(70), Some(4242)));
    }

    #[test]
    fn the_start_token_matches_the_collectors_rounding() {
        let fake = FakeProc::new("token");
        fake.process(500, "sleep", 1, 500, 12_345, 1000);
        let live = LinuxProcessController::for_current_process()
            .with_proc_root(&fake.0)
            .live(500)
            .unwrap();
        // btime 1000 s plus 123.45 s, in hundredths: what
        // `ProcessFacts::action_target` computes from the collector's reading.
        assert_eq!(live.start_token, Some(112_345));
        assert_eq!(live.uid, Some(1000));
        assert_eq!(live.session, Some(500));
    }

    #[test]
    fn a_recycled_pid_is_never_signalled() {
        // A real child the test owns, so a wrongly delivered SIGTERM would
        // show.
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id();
        let real = LinuxProcessController::for_current_process();
        let snapshot = real.target_for(pid).expect("the child is readable");

        // The same PID, but `/proc` now says it started at another time and
        // belongs to the same user: a different process on a recycled PID.
        let fake = FakeProc::new("recycled");
        fake.process(pid, "sleep", 1, pid, 999_999, real.current_uid().unwrap());
        let mut controller = real.clone().with_proc_root(&fake.0);
        let error = controller
            .perform(&snapshot, ProcessAction::Terminate)
            .expect_err("the identity no longer matches");
        assert_eq!(error, ActionError::ProcessDisappeared { pid });
        assert!(
            child.try_wait().unwrap().is_none(),
            "the process on the recycled PID must still be running"
        );
        // A changed owner is refused the same way.
        fake.process(pid, "sleep", 1, pid, 0, real.current_uid().unwrap() + 1);
        let mut moved = snapshot.clone();
        moved.start_token = controller.live(pid).unwrap().start_token;
        assert!(controller.perform(&moved, ProcessAction::Pause).is_err());
        // And a target with no start time at all is refused before anything.
        let mut unknown = snapshot.clone();
        unknown.start_token = None;
        assert!(matches!(
            real.clone().perform(&unknown, ProcessAction::Terminate),
            Err(ActionError::InvalidRequest { .. })
        ));
        assert!(child.try_wait().unwrap().is_none());

        // The real `/proc` agrees with the snapshot, so the signal goes out.
        real.clone()
            .perform(&snapshot, ProcessAction::Terminate)
            .expect("the snapshot still names the child");
        let status = child.wait().unwrap();
        assert!(!status.success(), "SIGTERM ended the child");
    }

    #[test]
    fn a_tree_that_holds_the_session_or_the_monitor_is_refused() {
        let fake = FakeProc::new("tree");
        let own = std::process::id();
        // The monitor runs under a terminal (30), under the user's systemd
        // (20), under init.
        fake.process(own, "monitor-gui", 30, 10, 5, 1000);
        fake.process(30, "gnome-terminal", 20, 10, 4, 1000);
        fake.process(20, "systemd", 1, 20, 3, 1000);
        fake.process(1, "systemd", 0, 1, 1, 0);
        fake.process(40, "bash", 30, 40, 6, 1000);
        fake.process(41, "make", 40, 40, 7, 1000);
        fake.process(42, "cc", 41, 40, 8, 1000);
        let controller = LinuxProcessController::for_current_process()
            .with_proc_root(&fake.0)
            .with_session_env(SessionEnv::default());
        assert_eq!(controller.own_ancestors(), vec![30, 20, 1]);
        assert_eq!(
            controller.tree_refusal(41, &[42, 41]),
            None,
            "a build under a shell may end"
        );
        // Leading a session is not the rule (owner, 2026-11-22): a shell that
        // is not above the window may be ended with everything it started.
        assert_eq!(controller.tree_refusal(40, &[42, 41, 40]), None);
        assert!(
            matches!(
                controller.tree_refusal(30, &[42, 41, 40, own, 30]),
                Some(ActionRefusal::SessionProcess { pid: 30, .. })
            ),
            "the monitor's own terminal"
        );
        assert!(
            controller.tree_refusal(20, &[20]).is_some(),
            "systemd --user"
        );
        assert_eq!(
            controller.tree_refusal(9999, &[9999]),
            None,
            "a gone process is skipped"
        );
    }

    /// A session laid out in a scratch `/proc`: processes with their
    /// cgroups and descriptors, the listening sockets in `net/unix`, and the
    /// logind sessions the window would have read over D-Bus.
    struct FakeSession {
        proc: FakeProc,
        uid: u32,
        sockets: Vec<String>,
        sessions: Option<Vec<LoginSession>>,
    }

    impl FakeSession {
        fn new(name: &str) -> Self {
            let proc = FakeProc::new(name);
            let uid = LinuxProcessController::for_current_process()
                .current_uid()
                .unwrap();
            Self {
                proc,
                uid,
                sockets: Vec::new(),
                sessions: None,
            }
        }

        /// A process of the user's, in a cgroup under the user's slice, whose
        /// descriptors can be read.
        fn process(&self, pid: u32, name: &str, parent: u32, session: u32, cgroup: &str) {
            self.hidden(pid, name, parent, session, cgroup);
            std::fs::create_dir_all(self.proc.0.join(pid.to_string()).join("fd")).unwrap();
        }

        /// The same, but its descriptors cannot be read (a non-dumpable
        /// process such as gpg-agent).
        fn hidden(&self, pid: u32, name: &str, parent: u32, session: u32, cgroup: &str) {
            self.proc
                .process(pid, name, parent, session, pid as u64, self.uid);
            std::fs::write(
                self.proc.0.join(pid.to_string()).join("cgroup"),
                format!("0::/user.slice/user-{}.slice/{cgroup}\n", self.uid),
            )
            .unwrap();
        }

        /// A listening socket at `path`, held by `holders`.
        fn socket(&mut self, path: &str, inode: u64, holders: &[u32]) {
            self.sockets.push(format!(
                "0000000000000000: 00000002 00000000 00010000 0001 01 {inode} {path}"
            ));
            std::fs::create_dir_all(self.proc.0.join("net")).unwrap();
            std::fs::write(
                self.proc.0.join("net/unix"),
                format!(
                    "Num       RefCount Protocol Flags    Type St Inode Path\n{}\n",
                    self.sockets.join("\n")
                ),
            )
            .unwrap();
            for pid in holders {
                let fd = self.proc.0.join(pid.to_string()).join("fd");
                std::fs::create_dir_all(&fd).unwrap();
                let slot = std::fs::read_dir(&fd).unwrap().count() + 3;
                std::os::unix::fs::symlink(format!("socket:[{inode}]"), fd.join(slot.to_string()))
                    .unwrap();
            }
        }

        /// A graphical logind session of the person.
        fn logind(&mut self, id: u32, leader: u32, own: bool) {
            self.sessions
                .get_or_insert_with(Vec::new)
                .push(LoginSession {
                    id: id.to_string(),
                    leader: Some(leader),
                    graphical: true,
                    own,
                });
        }

        /// A controller as the window has it: the facts read once, off the
        /// interface thread, and handed in.
        fn controller(&self, env: SessionEnv) -> LinuxProcessController {
            let mut controller = LinuxProcessController::for_current_process()
                .with_proc_root(&self.proc.0)
                .with_session_env(env);
            let facts = controller.reader().read(&ScanRequest {
                pids: None,
                sessions: self.sessions.clone(),
            });
            controller.set_session(facts);
            controller
        }

        fn user(&self, unit: &str) -> String {
            format!("user@{}.service/{unit}", self.uid)
        }
    }

    fn session_env() -> SessionEnv {
        SessionEnv {
            runtime_dir: Some("/run/user/1000".into()),
            wayland_display: Some("wayland-0".into()),
            bus_address: Some("unix:path=/run/user/1000/bus".into()),
            x_display: Some(":0".into()),
        }
    }

    /// The role a force stop is refused with, or `None` when it is offered.
    /// The role End task and Force stop are both refused with, or `None`
    /// when both are offered; the two never disagree. Pausing is never the
    /// guard's.
    fn force_stop_role(
        controller: &LinuxProcessController,
        pid: u32,
    ) -> Option<monitor_core::session::SessionRole> {
        // The start token matches nothing in the fake `/proc`, so even a
        // broken guard would stop at the identity check and never signal
        // whatever really runs under these PIDs.
        let target = ProcessTarget::new(pid, "fixture")
            .owned_by(controller.current_uid().unwrap())
            .started_at(1)
            .with_nice(0);
        let role = |action| match controller.availability(&target, action) {
            ActionAvailability::Available => None,
            ActionAvailability::Refused(ActionRefusal::SessionProcess { role, .. }) => Some(role),
            other => panic!("pid {pid} {action:?}: {other:?}"),
        };
        let force = role(ProcessAction::ForceStop);
        assert_eq!(role(ProcessAction::Terminate), force, "pid {pid}: End task");
        // The interface thread's answer, from the snapshot the window read,
        // is the same.
        if controller.session_snapshot().is_some() {
            let view = match controller
                .snapshot_view()
                .availability(&target, ProcessAction::ForceStop)
            {
                ActionAvailability::Available => None,
                ActionAvailability::Refused(ActionRefusal::SessionProcess { role, .. }) => {
                    Some(role)
                }
                other => panic!("pid {pid} from the snapshot: {other:?}"),
            };
            assert_eq!(view, force, "pid {pid}: the snapshot agrees");
        }
        assert!(
            controller
                .availability(&target, ProcessAction::Pause)
                .is_available(),
            "pid {pid}: Suspend is not the guard's"
        );
        force
    }

    /// GNOME 46 as Ubuntu 24.04 and Zorin 18 run it, laid out from this
    /// desktop on 2026-11-22: gnome-shell in `org.gnome.Shell@wayland.service`
    /// holding the Wayland and X sockets, Xwayland started on demand, the
    /// bus socket held by systemd and dbus-daemon, GDM's leader (root) with
    /// `gdm-wayland-session` under it, and a terminal whose shell leads its
    /// own session.
    #[test]
    fn a_gnome_session_keeps_its_compositor_bus_and_session_and_lets_the_rest_go() {
        use monitor_core::session::SessionRole as Role;
        let mut s = FakeSession::new("gnome");
        let own = std::process::id();
        s.process(2338, "systemd", 1, 2338, &s.user("init.scope"));
        s.process(
            2370,
            "dbus-daemon",
            2338,
            2370,
            &s.user("session.slice/dbus.service"),
        );
        let shell = s.user("session.slice/org.gnome.Shell@wayland.service");
        s.process(2545, "gnome-shell", 2338, 2545, &shell);
        s.process(2600, "Xwayland", 2545, 2545, &shell);
        s.process(2408, "gdm-wayland-ses", 2321, 2408, "session-2.scope");
        s.process(2412, "gnome-session-b", 2408, 2408, "session-2.scope");
        s.process(
            2480,
            "gnome-session-b",
            2338,
            2480,
            &s.user("session.slice/gnome-session-manager@gnome.service"),
        );
        s.process(
            3336,
            "better-monitor-",
            2338,
            3336,
            &s.user("app.slice/better-monitor.service"),
        );
        // A terminal, its shell, and Better Monitor started from it.
        let ptyxis = s.user("app.slice/app-org.gnome.Ptyxis-4000.scope");
        s.process(4000, "ptyxis", 2338, 4000, &ptyxis);
        s.process(4001, "ptyxis-agent", 4000, 4001, &ptyxis);
        s.process(4002, "bash", 4001, 4002, &ptyxis);
        s.process(own, "monitor-gui", 4002, 4002, &ptyxis);
        // A second terminal shell, not above the window.
        s.process(4010, "bash", 4001, 4010, &ptyxis);
        // User services, each its own session leader.
        s.process(
            4100,
            "tracker-miner-f",
            2338,
            4100,
            &s.user("session.slice/tracker-miner-fs-3.service"),
        );
        s.process(
            4200,
            "gvfsd",
            2338,
            4200,
            &s.user("app.slice/gvfs-daemon.service"),
        );
        s.process(
            4300,
            "xdg-desktop-por",
            2338,
            4300,
            &s.user("app.slice/xdg-desktop-portal.service"),
        );
        // A nested gnome-shell another program runs, which is not the desktop.
        s.process(
            4400,
            "gnome-shell",
            4390,
            4390,
            &s.user("app.slice/run-r1.scope"),
        );
        s.socket("/run/user/1000/bus", 927, &[2338, 2370]);
        s.socket("/run/user/1000/wayland-0", 31750, &[2545]);
        s.socket("/tmp/.X11-unix/X0", 31747, &[2545, 2600]);
        s.socket("@/tmp/.X11-unix/X0", 31746, &[2545, 2600]);
        s.logind(2, 2321, true);
        let controller = s.controller(session_env());

        for (pid, role) in [
            (2338, Some(Role::Monitor)),
            (2370, Some(Role::MessageBus)),
            (2545, Some(Role::Compositor)),
            // Mutter 46 starts Xwayland on demand inside a user unit and
            // carries on without it: only X11 windows close.
            (2600, None),
            (2408, Some(Role::SessionLauncher)),
            (2412, Some(Role::SessionManager)),
            (2480, Some(Role::SessionManager)),
            (3336, Some(Role::MonitorService)),
            // The terminal and the shell Better Monitor runs in.
            (4000, Some(Role::Monitor)),
            (4002, Some(Role::Monitor)),
            // Everything else that leads a session may be force stopped.
            (4010, None),
            (4100, None),
            (4200, None),
            (4300, None),
            (4400, None),
        ] {
            assert_eq!(force_stop_role(&controller, pid), role, "pid {pid}");
        }
        let facts = controller.session_snapshot().unwrap();
        assert_eq!(facts.bus_holders, Some(vec![2338, 2370]));
        assert_eq!(facts.compositor_holders, Some(vec![2545]));
        assert_eq!(facts.on_demand_compositors, vec![2545]);
        // `perform` asks again, so a refused End task or Force stop reaches
        // no syscall.
        let target = ProcessTarget::new(2545, "gnome-shell")
            .owned_by(s.uid)
            .started_at(1)
            .with_nice(0);
        for action in [ProcessAction::Terminate, ProcessAction::ForceStop] {
            assert!(matches!(
                controller.clone().perform(&target, action),
                Err(ActionError::PermissionDenied { .. })
            ));
        }
        // End process tree, by the same rule.
        assert!(matches!(
            controller.tree_refusal(2338, &[2370, 2545, 2600, 4000, 2338]),
            Some(ActionRefusal::SessionProcess {
                pid: 2338,
                role: Role::Monitor,
                ..
            })
        ));
        assert!(matches!(
            controller.tree_refusal(2408, &[2412, 2408]),
            Some(ActionRefusal::SessionProcess {
                role: Role::SessionLauncher,
                ..
            })
        ));
        // The terminal Better Monitor runs in, from above it.
        assert!(matches!(
            controller.tree_refusal(4000, &[own, 4002, 4010, 4001, 4000]),
            Some(ActionRefusal::SessionProcess {
                pid: 4000,
                role: Role::Monitor,
                ..
            })
        ));
        // Another shell in that terminal, a user service, and the nested
        // shell may be ended whole.
        assert_eq!(controller.tree_refusal(4010, &[4010]), None);
        assert_eq!(controller.tree_refusal(4100, &[4100]), None);
        assert_eq!(controller.tree_refusal(4400, &[4400]), None);
        // The window itself is the shared policy's.
        let itself = ProcessTarget::new(own, "monitor-gui")
            .owned_by(s.uid)
            .with_nice(0);
        assert!(matches!(
            controller.availability(&itself, ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::ProtectedProcess { .. })
        ));
        assert_eq!(controller.session_refusal(99_999), None, "gone");
    }

    /// COSMIC from a greeter: the leader starts `start-cosmic`, which starts
    /// cosmic-session, which starts cosmic-comp, which starts Xwayland, all
    /// in the session's scope beside the panel and an application.
    #[test]
    fn a_cosmic_session_keeps_its_compositor_x_server_and_session_manager() {
        use monitor_core::session::SessionRole as Role;
        let mut s = FakeSession::new("cosmic");
        let scope = "session-3.scope";
        s.process(2200, "systemd", 1, 2200, &s.user("init.scope"));
        s.process(
            2210,
            "dbus-broker-lau",
            2200,
            2210,
            &s.user("session.slice/dbus-broker.service"),
        );
        s.process(
            2211,
            "dbus-broker",
            2210,
            2210,
            &s.user("session.slice/dbus-broker.service"),
        );
        s.process(2305, "start-cosmic", 2300, 2305, scope);
        s.process(2310, "cosmic-session", 2305, 2305, scope);
        s.process(2320, "cosmic-comp", 2310, 2305, scope);
        s.process(2330, "Xwayland", 2320, 2305, scope);
        s.process(2340, "cosmic-panel", 2310, 2305, scope);
        s.process(2350, "cosmic-edit", 2310, 2305, scope);
        s.socket("/run/user/1000/bus", 900, &[2200, 2210, 2211]);
        s.socket("/run/user/1000/wayland-1", 901, &[2320]);
        s.socket("/tmp/.X11-unix/X1", 902, &[2320, 2330]);
        s.logind(3, 2300, true);
        let env = SessionEnv {
            wayland_display: Some("wayland-1".into()),
            x_display: Some(":1".into()),
            ..session_env()
        };
        let controller = s.controller(env);
        for (pid, role) in [
            (2200, Some(Role::UserManager)),
            (2210, Some(Role::MessageBus)),
            (2211, Some(Role::MessageBus)),
            (2305, Some(Role::SessionLauncher)),
            (2310, Some(Role::SessionManager)),
            (2320, Some(Role::Compositor)),
            // cosmic-comp's Xwayland policy is not known, so it stays.
            (2330, Some(Role::XServer)),
            (2340, None),
            (2350, None),
        ] {
            assert_eq!(force_stop_role(&controller, pid), role, "pid {pid}");
        }
        // End process tree: the session manager's tree is its own refusal;
        // the panel's and an application's trees may be ended.
        assert!(matches!(
            controller.tree_refusal(2310, &[2330, 2320, 2340, 2350, 2310]),
            Some(ActionRefusal::SessionProcess {
                role: Role::SessionManager,
                ..
            })
        ));
        assert_eq!(controller.tree_refusal(2340, &[2340]), None);
        assert_eq!(controller.tree_refusal(2350, &[2350]), None);
    }

    /// `flatpak run` 1.14.6 adds no `--new-session` to bwrap (its
    /// `common/flatpak-run.c` blocks `TIOCSTI` with seccomp instead), and on
    /// this desktop on 2026-11-22 `flatpak run --command=sleep` left
    /// `bwrap`, the inner `bwrap`, and `sleep` in the caller's session. The
    /// application may be force stopped either way: leading a session is not
    /// the rule.
    #[test]
    fn a_flatpak_application_may_be_force_stopped() {
        let mut s = FakeSession::new("flatpak");
        let scope = s.user("app.slice/app-flatpak-net.nokyan.Resources-5000.scope");
        s.process(2338, "systemd", 1, 2338, &s.user("init.scope"));
        s.process(5000, "bwrap", 2338, 5000, &scope);
        s.process(5001, "bwrap", 5000, 5000, &scope);
        s.process(5002, "resources", 5001, 5000, &scope);
        s.process(5010, "xdg-dbus-proxy", 2338, 5010, &scope);
        s.process(
            2545,
            "gnome-shell",
            2338,
            2545,
            &s.user("session.slice/org.gnome.Shell@wayland.service"),
        );
        s.socket("/run/user/1000/bus", 927, &[2338]);
        s.socket("/run/user/1000/wayland-0", 931, &[2545]);
        let controller = s.controller(session_env());
        for pid in [5000, 5001, 5002, 5010] {
            assert_eq!(force_stop_role(&controller, pid), None, "pid {pid}");
        }
    }

    #[test]
    fn a_session_whose_records_cannot_be_read_keeps_every_process_that_matches_a_name() {
        use monitor_core::session::SessionRole as Role;
        // No net/unix, no logind record, no cgroups: nothing can be proven.
        let fake = FakeProc::new("blind");
        let uid = LinuxProcessController::for_current_process()
            .current_uid()
            .unwrap();
        for (pid, name) in [
            (10, "systemd"),
            (11, "dbus-daemon"),
            (12, "gnome-shell"),
            (13, "Xwayland"),
            (14, "gnome-session-b"),
            (15, "make"),
        ] {
            fake.process(pid, name, 1, pid, pid as u64, uid);
        }
        let controller = LinuxProcessController::for_current_process()
            .with_proc_root(&fake.0)
            .with_session_env(session_env());
        for (pid, role) in [
            (10, Some(Role::UserManager)),
            (11, Some(Role::MessageBus)),
            (12, Some(Role::Compositor)),
            (13, Some(Role::XServer)),
            (14, Some(Role::SessionManager)),
            (15, None),
        ] {
            assert_eq!(force_stop_role(&controller, pid), role, "pid {pid}");
        }
    }

    /// A process whose descriptors cannot be read is not known to hold
    /// nothing: the socket's holder list says nothing about it, and a
    /// matching name protects it. A process that holds nothing and whose
    /// descriptors were read is not protected by its name.
    #[test]
    fn a_process_whose_descriptors_cannot_be_read_is_protected_by_its_name() {
        use monitor_core::session::SessionRole as Role;
        let mut s = FakeSession::new("hidden");
        s.process(2338, "systemd", 1, 2338, &s.user("init.scope"));
        let shell = s.user("session.slice/org.gnome.Shell@wayland.service");
        // The compositor's descriptors cannot be read; the socket is found.
        s.hidden(2545, "gnome-shell", 2338, 2545, &shell);
        s.hidden(
            2700,
            "gpg-agent",
            2338,
            2700,
            &s.user("app.slice/gpg.service"),
        );
        s.process(
            2800,
            "dbus-daemon",
            2338,
            2800,
            &s.user("app.slice/other.scope"),
        );
        s.socket("/run/user/1000/bus", 927, &[2338]);
        s.socket("/run/user/1000/wayland-0", 931, &[]);
        let controller = s.controller(session_env());
        let facts = controller.session_snapshot().unwrap();
        assert!(!facts.fds_read.contains(&2545));
        assert!(facts.fds_read.contains(&2800));
        assert_eq!(force_stop_role(&controller, 2545), Some(Role::Compositor));
        assert_eq!(force_stop_role(&controller, 2700), None);
        assert_eq!(force_stop_role(&controller, 2800), None);
    }

    /// The person is signed in to GNOME and to COSMIC at once, and Better
    /// Monitor runs in GNOME. COSMIC's compositor holds `wayland-1` in the
    /// same runtime directory and runs in COSMIC's session scope, so it is
    /// found by the same facts; a nested compositor in an app scope is not.
    #[test]
    fn a_second_graphical_session_is_protected_by_the_same_facts() {
        use monitor_core::session::SessionRole as Role;
        let mut s = FakeSession::new("two");
        s.process(2338, "systemd", 1, 2338, &s.user("init.scope"));
        s.process(
            2370,
            "dbus-daemon",
            2338,
            2370,
            &s.user("session.slice/dbus.service"),
        );
        let shell = s.user("session.slice/org.gnome.Shell@wayland.service");
        s.process(2545, "gnome-shell", 2338, 2545, &shell);
        s.process(2408, "gdm-wayland-ses", 2321, 2408, "session-2.scope");
        let cosmic = "session-3.scope";
        s.process(3305, "start-cosmic", 3300, 3305, cosmic);
        s.process(3310, "cosmic-session", 3305, 3305, cosmic);
        s.process(3320, "cosmic-comp", 3310, 3305, cosmic);
        s.process(3330, "Xwayland", 3320, 3305, cosmic);
        s.process(3340, "cosmic-panel", 3310, 3305, cosmic);
        s.process(4400, "sway", 4390, 4390, &s.user("app.slice/run-r1.scope"));
        s.socket("/run/user/1000/bus", 927, &[2338, 2370]);
        s.socket("/run/user/1000/wayland-0", 931, &[2545]);
        s.socket("/run/user/1000/wayland-1", 932, &[3320]);
        s.socket("/tmp/.X11-unix/X1", 933, &[3320, 3330]);
        s.socket("/run/user/1000/wayland-9", 934, &[4400]);
        s.logind(2, 2321, true);
        s.logind(3, 3300, false);
        let controller = s.controller(session_env());
        for (pid, role) in [
            (2545, Some(Role::Compositor)),
            (2408, Some(Role::SessionLauncher)),
            (3305, Some(Role::SessionLauncher)),
            (3310, Some(Role::SessionManager)),
            (3320, Some(Role::Compositor)),
            (3330, Some(Role::XServer)),
            (3340, None),
            (4400, None),
        ] {
            assert_eq!(force_stop_role(&controller, pid), role, "pid {pid}");
        }
    }

    /// A reader that must never be called.
    #[derive(Debug)]
    struct PanickingReader;

    impl SessionReader for PanickingReader {
        fn read(&self, _: &ScanRequest) -> SessionFacts {
            panic!("the interface thread read /proc")
        }
    }

    /// A reader that counts its reads and answers with fixed facts.
    #[derive(Debug)]
    struct CountingReader {
        reads: std::sync::atomic::AtomicUsize,
        facts: SessionFacts,
    }

    impl SessionReader for CountingReader {
        fn read(&self, _: &ScanRequest) -> SessionFacts {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.facts.clone()
        }
    }

    /// Everything the window asks while it draws comes from the snapshot:
    /// a reader that panics is never reached, for a process the snapshot
    /// covers, one it does not, and a tree.
    #[test]
    fn the_interface_threads_answers_never_read_proc() {
        let uid = LinuxProcessController::for_current_process()
            .current_uid()
            .unwrap();
        let shell = SessionMember {
            pid: 2545,
            name: "gnome-shell".into(),
            parent: Some(2338),
            cgroup: Some("/user.slice/x/session.slice/org.gnome.Shell@wayland.service".into()),
        };
        let mut facts = SessionFacts {
            uid,
            compositor_holders: Some(vec![2545]),
            ..SessionFacts::default()
        };
        facts.fds_read.insert(2545);
        facts.members.insert(2545, shell);
        let mut controller =
            LinuxProcessController::for_current_process().with_reader(Arc::new(PanickingReader));
        controller.set_session(facts);
        let target = |pid: u32, name: &str| {
            ProcessTarget::new(pid, name)
                .owned_by(uid)
                .started_at(1)
                .with_nice(0)
        };
        let view = controller.snapshot_view();
        assert!(matches!(
            view.availability(&target(2545, "gnome-shell"), ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::SessionProcess { .. })
        ));
        assert!(
            view.availability(&target(7000, "make"), ProcessAction::Terminate)
                .is_available()
        );
        assert!(
            controller
                .tree_refusal_snapshot(2545, &[2545], |_| "gnome-shell".into())
                .is_some()
        );
        // A process the snapshot does not cover asks for a re-read.
        let live: std::collections::BTreeSet<u32> = [2545].into();
        assert!(controller.needs_session_read(&live));
    }

    /// A new holder that appears after the snapshot (an Xwayland a
    /// compositor whose policy is not known just started, under another
    /// name than the one the guard knows): the snapshot cannot know it, but
    /// the window reads again because its PID is not covered, and `perform`
    /// reads fresh facts before it signals, so it is refused there.
    #[test]
    fn a_holder_that_appeared_after_the_snapshot_is_refused_when_it_matters() {
        use monitor_core::session::SessionRole as Role;
        let mut s = FakeSession::new("late");
        s.process(2200, "systemd", 1, 2200, &s.user("init.scope"));
        s.process(2320, "cosmic-comp", 2200, 2320, "session-3.scope");
        s.socket("/run/user/1000/bus", 900, &[2200]);
        s.socket("/run/user/1000/wayland-0", 901, &[2320]);
        s.logind(3, 2300, true);
        let mut controller = s.controller(session_env());
        // After the snapshot, the compositor starts an X server.
        s.process(2330, "xserver-new", 2320, 2320, "session-3.scope");
        s.socket("/tmp/.X11-unix/X0", 902, &[2320, 2330]);
        let target = ProcessTarget::new(2330, "xserver-new")
            .owned_by(s.uid)
            .started_at(1)
            .with_nice(0);
        // The snapshot does not cover it, and says so.
        let live: std::collections::BTreeSet<u32> = [2200, 2320, 2330].into();
        assert!(!controller.needs_session_read(&live));
        let _ = controller
            .snapshot_view()
            .availability(&target, ProcessAction::ForceStop);
        assert!(controller.needs_session_read(&live));
        // Fresh facts refuse it, in `availability` and in `perform`.
        assert!(matches!(
            controller.availability(&target, ProcessAction::ForceStop),
            ActionAvailability::Refused(ActionRefusal::SessionProcess {
                role: Role::XServer,
                ..
            })
        ));
        assert!(matches!(
            controller.perform(&target, ProcessAction::ForceStop),
            Err(ActionError::PermissionDenied { .. })
        ));
    }

    /// `perform` reads the facts itself every time: the window's snapshot
    /// is never what lets a signal through.
    #[test]
    fn perform_reads_fresh_facts_every_time() {
        let uid = LinuxProcessController::for_current_process()
            .current_uid()
            .unwrap();
        let reader = Arc::new(CountingReader {
            reads: Default::default(),
            facts: SessionFacts {
                uid,
                ..SessionFacts::default()
            },
        });
        let mut controller =
            LinuxProcessController::for_current_process().with_reader(reader.clone());
        controller.set_session(SessionFacts {
            uid,
            ..SessionFacts::default()
        });
        // A target whose identity cannot match, so nothing is signalled.
        let target = ProcessTarget::new(0x7FFF_FFF0, "gone")
            .owned_by(uid)
            .started_at(1)
            .with_nice(0);
        for _ in 0..2 {
            let _ = controller.perform(&target, ProcessAction::Terminate);
        }
        assert_eq!(reader.reads.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn the_program_folder_survives_a_replaced_file_and_a_non_utf8_name() {
        let fake = FakeProc::new("exe");
        let dir = fake.0.join("77");
        std::fs::create_dir_all(&dir).unwrap();
        let mut target = b"/opt/caf\xe9/bin/tool (deleted)".to_vec();
        std::os::unix::fs::symlink(OsString::from_vec(target.clone()), dir.join("exe")).unwrap();
        let controller = LinuxProcessController::for_current_process().with_proc_root(&fake.0);
        let folder = controller.program_folder(77).unwrap();
        target.truncate(b"/opt/caf\xe9/bin".len());
        assert_eq!(folder.as_os_str().as_bytes(), &target[..]);
        assert_eq!(
            without_deleted_suffix(OsString::from("/usr/bin/x")),
            OsString::from("/usr/bin/x")
        );
        assert_eq!(controller.program_folder(78), None);
    }
}

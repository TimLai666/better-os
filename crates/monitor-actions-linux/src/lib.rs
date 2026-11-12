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
use std::ffi::OsString;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

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

fn parse_stat(text: &str) -> Option<StatFields> {
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

fn real_uid(root: &Path, pid: u32) -> Option<u32> {
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
#[derive(Clone, Debug)]
pub struct LinuxProcessController {
    current_uid: u32,
    own_pid: u32,
    proc_root: PathBuf,
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
            proc_root: PathBuf::from("/proc"),
        }
    }

    /// The same controller reading a different `/proc`, for tests that need
    /// a process whose recorded identity differs from the running one.
    pub fn with_proc_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.proc_root = root.into();
        self
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

    /// Whether a process tree may be ended: `None`, or the first member that
    /// the session stands on. A member that is already gone is skipped.
    pub fn tree_refusal(&self, pids: &[u32]) -> Option<monitor_core::ActionRefusal> {
        let members: Vec<monitor_core::TreeMember> = pids
            .iter()
            .filter_map(|pid| self.live(*pid))
            .map(|live| monitor_core::TreeMember {
                pid: live.pid,
                session_leader: live.session == Some(live.pid),
                name: live.name,
            })
            .collect();
        monitor_core::tree_refusal(&members, &self.own_ancestors())
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
        let controller = LinuxProcessController::for_current_process().with_proc_root(&fake.0);
        assert_eq!(controller.own_ancestors(), vec![30, 20, 1]);
        assert_eq!(
            controller.tree_refusal(&[42, 41]),
            None,
            "a build under a shell may end"
        );
        assert!(
            matches!(
                controller.tree_refusal(&[42, 41, 40]),
                Some(ActionRefusal::ProtectedTree { pid: 40, .. })
            ),
            "the shell leads its session"
        );
        assert!(
            matches!(
                controller.tree_refusal(&[own, 30]),
                Some(ActionRefusal::ProtectedTree { pid: 30, .. })
            ),
            "the monitor's own terminal"
        );
        assert!(controller.tree_refusal(&[20]).is_some(), "systemd --user");
        assert_eq!(
            controller.tree_refusal(&[9999]),
            None,
            "a gone process is skipped"
        );
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

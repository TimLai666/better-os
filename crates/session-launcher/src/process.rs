//! Starting a program from the launcher.
//!
//! The launcher blocks SIGTERM, SIGINT, and SIGHUP in every thread so that
//! one thread can wait for them. A blocked signal stays blocked across
//! `fork` and `execve`, and Rust's `Command` keeps the parent's mask on
//! purpose (`std` `sys/process/unix/unix.rs`: "Inherit the signal mask from
//! the parent rather than resetting it"). So a child started as it is
//! ignores SIGTERM: in ticket 203's VM `cosmic-comp`, its XWayland, and the
//! locker all showed `SigBlk: 0000000000004003`, every logout waited out the
//! patience and killed them, and every program the compositor starts from a
//! key inherits the mask too. Every child therefore starts with an empty
//! signal mask.

use std::os::unix::process::CommandExt as _;
use std::process::Command;

/// Makes `command` start its program with no signal blocked.
pub fn with_signals_unblocked(command: &mut Command) -> &mut Command {
    // SAFETY: sigemptyset and pthread_sigmask are async-signal-safe and touch
    // only the child's own stack and mask.
    unsafe {
        command.pre_exec(|| {
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            if libc::pthread_sigmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
    }
}

/// Makes `command`'s program receive `signal` when the thread that starts
/// it ends: the launcher dying any way at all (SIGKILL, the OOM killer, a
/// panic) then takes the compositor and the locker with it, where before
/// they stayed in the abandoned session scope (ticket 203's review, P1).
///
/// The kernel ties the signal to the thread that forked, not to the
/// process, so the launcher starts its children from its main thread only,
/// which lives as long as the process. `execve(2)` keeps the setting in the
/// child and does not fire it when the parent execs: the re-executed
/// launcher's main thread is the same thread, so adopted children still die
/// with it. A parent that is already gone by the time the child sets it is
/// caught by comparing the parent's id.
pub fn with_parent_death_signal(command: &mut Command, signal: i32) -> &mut Command {
    // SAFETY: getpid has no preconditions.
    let parent = unsafe { libc::getpid() };
    // SAFETY: prctl, getppid and raise are async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, signal as libc::c_ulong, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::raise(signal);
            }
            Ok(())
        })
    }
}

/// Sets or clears `FD_CLOEXEC` on a descriptor this process owns.
pub fn set_close_on_exec(fd: std::os::fd::RawFd, close: bool) -> std::io::Result<()> {
    // SAFETY: fcntl on a descriptor number; an invalid one is an error.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        let flags = if close {
            flags | libc::FD_CLOEXEC
        } else {
            flags & !libc::FD_CLOEXEC
        };
        if libc::fcntl(fd, libc::F_SETFD, flags) == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd as _;
    use std::time::{Duration, Instant};

    #[test]
    fn a_child_dies_with_the_thread_that_started_it() {
        // The starting thread ends, as the launcher's would: the child gets
        // the signal although this process lives on.
        let pid = std::thread::spawn(|| {
            let mut command = Command::new("sleep");
            command.arg("30");
            with_parent_death_signal(&mut command, libc::SIGKILL);
            command.spawn().unwrap().id() as i32
        })
        .join()
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut status = 0;
        loop {
            // SAFETY: waitpid on our own child, writing only `status`.
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the child outlived its parent thread"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(libc::WIFSIGNALED(status));
        assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
    }

    #[test]
    fn close_on_exec_is_set_and_cleared() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = a.as_raw_fd();
        let flag = || unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC;
        assert_ne!(flag(), 0, "Rust opens descriptors close-on-exec");
        set_close_on_exec(fd, false).unwrap();
        assert_eq!(flag(), 0);
        set_close_on_exec(fd, true).unwrap();
        assert_ne!(flag(), 0);
        assert!(set_close_on_exec(-1, true).is_err());
    }

    /// The `SigBlk` line a child of this thread reports about itself. `grep`
    /// is run directly and not through a shell, which may reset its mask.
    fn child_mask(clean: bool) -> String {
        let mut command = Command::new("grep");
        command.args(["^SigBlk:", "/proc/self/status"]);
        if clean {
            with_signals_unblocked(&mut command);
        }
        let output = command.output().unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn a_child_never_inherits_the_launchers_blocked_signals() {
        // On a thread of its own: the mask is per thread, and the harness's
        // other threads keep theirs.
        std::thread::spawn(|| {
            // SAFETY: an initialized set, this thread's own mask.
            unsafe {
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGTERM);
                assert_eq!(
                    libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()),
                    0
                );
            }
            assert_eq!(
                child_mask(false),
                "SigBlk:\t0000000000004000",
                "without the fix the child keeps SIGTERM blocked"
            );
            assert_eq!(child_mask(true), "SigBlk:\t0000000000000000");
        })
        .join()
        .unwrap();
    }
}

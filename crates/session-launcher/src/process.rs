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

/// The stop signals the launcher handles.
pub const STOP_SIGNALS: [i32; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

/// [`STOP_SIGNALS`] as a set.
pub fn stop_signal_set() -> libc::sigset_t {
    // SAFETY: the set is initialized by sigemptyset before sigaddset.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in STOP_SIGNALS {
            libc::sigaddset(&mut set, signal);
        }
        set
    }
}

/// The first stop signal pending for the calling thread or the process.
///
/// The launcher never takes a stop signal off the pending set: it watches a
/// [`stop_signal_fd`] with `poll` and never reads it. `execve(2)` keeps the
/// mask and the pending set, so a stop signal that arrives while the
/// launcher `exec`s its updated program is still pending in the new one,
/// whose own descriptor is readable at once (ticket 203's second review).
pub fn pending_stop_signal() -> Option<i32> {
    // SAFETY: sigpending writes only to `pending`, which sigemptyset
    // initialized.
    unsafe {
        let mut pending: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut pending);
        if libc::sigpending(&mut pending) != 0 {
            return None;
        }
        STOP_SIGNALS
            .into_iter()
            .find(|signal| libc::sigismember(&pending, *signal) == 1)
    }
}

/// A close-on-exec `signalfd` for the stop signals, which the caller has
/// blocked. It is readable while one is pending.
pub fn stop_signal_fd() -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd as _;
    let set = stop_signal_set();
    // SAFETY: `set` is initialized; -1 asks for a new descriptor.
    let raw = unsafe { libc::signalfd(-1, &set, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a new descriptor nothing else owns.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) })
}

/// Waits until `fd` is readable, without reading it. `None` waits without
/// end. Returns whether it became readable.
pub fn wait_readable(
    fd: std::os::fd::BorrowedFd<'_>,
    timeout: Option<std::time::Duration>,
) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd as _;
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = timeout.map_or(-1, |timeout| {
        i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX)
    });
    loop {
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready >= 0 {
            return Ok(ready > 0);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
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
    use std::os::fd::{AsFd as _, AsRawFd as _};
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

    #[test]
    fn a_watched_stop_signal_stays_pending_until_taken() {
        // On a thread of its own, with the signal sent to that thread only:
        // the harness's other threads do not block SIGTERM.
        std::thread::spawn(|| {
            let set = stop_signal_set();
            // SAFETY: an initialized set, this thread's own mask.
            unsafe {
                assert_eq!(
                    libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()),
                    0
                );
            }
            let fd = stop_signal_fd().unwrap();
            assert_eq!(pending_stop_signal(), None);
            assert!(!wait_readable(fd.as_fd(), Some(Duration::ZERO)).unwrap());
            // SAFETY: a signal to this very thread, which blocks it.
            unsafe {
                libc::pthread_kill(libc::pthread_self(), libc::SIGHUP);
            }
            assert!(wait_readable(fd.as_fd(), Some(Duration::from_secs(5))).unwrap());
            // Looked at twice, still there: nothing took it.
            assert_eq!(pending_stop_signal(), Some(libc::SIGHUP));
            assert!(wait_readable(fd.as_fd(), Some(Duration::ZERO)).unwrap());
            assert_eq!(pending_stop_signal(), Some(libc::SIGHUP));
            // Taken, so the thread ends clean.
            let mut taken = 0;
            // SAFETY: an initialized set.
            assert_eq!(unsafe { libc::sigwait(&set, &mut taken) }, 0);
            assert_eq!(taken, libc::SIGHUP);
            assert_eq!(pending_stop_signal(), None);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn a_pending_stop_signal_crosses_an_exec() {
        // The child blocks SIGTERM, has one sent to itself, and execs `grep`
        // through `Command`'s own exec path, the one the launcher's restart
        // in place takes: the signal is still pending and blocked there.
        let mut command = Command::new("grep");
        command.args(["-E", "^(SigBlk|ShdPnd):", "/proc/self/status"]);
        // SAFETY: sigemptyset, sigaddset, pthread_sigmask, getpid and kill
        // are async-signal-safe.
        unsafe {
            command.pre_exec(|| {
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGTERM);
                if libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::kill(libc::getpid(), libc::SIGTERM);
                Ok(())
            });
        }
        let output = command.output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("ShdPnd:\t0000000000004000"), "{text}");
        assert!(text.contains("SigBlk:\t0000000000004000"), "{text}");
    }
}

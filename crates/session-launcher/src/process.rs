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

#[cfg(test)]
mod tests {
    use super::*;

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

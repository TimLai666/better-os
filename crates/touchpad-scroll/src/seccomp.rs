//! What the service may still ask uinput for, once it has dropped privilege.
//!
//! The uinput descriptor is the one dangerous thing the service keeps: with it
//! a process can declare a device that reports keyboard keys and type into the
//! session. The kernel only stops that at the descriptor's permissions, which
//! the service already passed. So after the drop the service installs its own
//! seccomp filter, which checks the arguments of every `ioctl`:
//!
//! - `UI_SET_KEYBIT` only for codes in the button block `0x100..0x160`, the
//!   block [`crate::caps::classify`] allows a touchpad;
//! - `UI_SET_EVBIT` only for `SYN`, `KEY`, `REL`, `ABS`, `MSC`;
//! - `UI_SET_SWBIT`, `UI_SET_LEDBIT`, `UI_SET_FFBIT`, `UI_SET_SNDBIT` never.
//!
//! A refused call fails with `EPERM`. A system call from another architecture
//! (or the x32 ABI) kills the process. Every other call is left to the unit's
//! own `SystemCallFilter=`. Even a process taken over after this point cannot
//! build a keyboard: it cannot declare one key outside the button block.

#![allow(unsafe_code)]

use std::io;

use crate::codes::{BTN_MISC, EV_MSC, KEY_OK};

const fn uinput_set(number: u8) -> u32 {
    // _IOW('U', number, int)
    (1 << 30) | (4 << 16) | ((b'U' as u32) << 8) | number as u32
}

pub const UI_SET_EVBIT: u32 = uinput_set(100);
pub const UI_SET_KEYBIT: u32 = uinput_set(101);
pub const UI_SET_LEDBIT: u32 = uinput_set(105);
pub const UI_SET_SNDBIT: u32 = uinput_set(106);
pub const UI_SET_FFBIT: u32 = uinput_set(107);
pub const UI_SET_SWBIT: u32 = uinput_set(109);

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xc000_00b7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("the seccomp filter knows x86_64 and aarch64 only");

const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

// Offsets into `struct seccomp_data`, little endian.
const NR: u32 = 0;
const ARCH: u32 = 4;
const ARG1_LOW: u32 = 16 + 8;
const ARG2_LOW: u32 = 16 + 16;
const ARG2_HIGH: u32 = 16 + 16 + 4;

// BPF_LD (0x00) | BPF_W (0x00) | BPF_ABS (0x20).
const BPF_LD_W_ABS: u16 = 0x20;
const BPF_JEQ_K: u16 = 0x05 | 0x10;
const BPF_JGE_K: u16 = 0x05 | 0x30;
const BPF_JGT_K: u16 = 0x05 | 0x20;
const BPF_RET_K: u16 = 0x06;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Label {
    Next,
    Ioctl,
    Keybit,
    Keybit2,
    Keybit3,
    Evbit,
    Evbit2,
    Allow,
    Deny,
    Kill,
}

enum Op {
    Load(u32),
    Jump(u16, u32, Label, Label),
    Return(u32),
    Here(Label),
}

fn program() -> Vec<libc::sock_filter> {
    use Label::*;
    use Op::*;
    let deny = SECCOMP_RET_ERRNO | libc::EPERM as u32;
    let mut ops = vec![
        Load(ARCH),
        Jump(BPF_JEQ_K, AUDIT_ARCH, Next, Kill),
        Here(Next),
        Load(NR),
    ];
    #[cfg(target_arch = "x86_64")]
    ops.push(Jump(BPF_JGE_K, 0x4000_0000, Kill, Next));
    ops.extend([
        Here(Next),
        Jump(BPF_JEQ_K, libc::SYS_ioctl as u32, Ioctl, Allow),
        Here(Ioctl),
        Load(ARG1_LOW),
        Jump(BPF_JEQ_K, UI_SET_KEYBIT, Keybit, Next),
        Here(Next),
        Jump(BPF_JEQ_K, UI_SET_EVBIT, Evbit, Next),
        Here(Next),
        Jump(BPF_JEQ_K, UI_SET_SWBIT, Deny, Next),
        Here(Next),
        Jump(BPF_JEQ_K, UI_SET_LEDBIT, Deny, Next),
        Here(Next),
        Jump(BPF_JEQ_K, UI_SET_FFBIT, Deny, Next),
        Here(Next),
        Jump(BPF_JEQ_K, UI_SET_SNDBIT, Deny, Allow),
        Here(Keybit),
        Load(ARG2_HIGH),
        Jump(BPF_JEQ_K, 0, Keybit2, Deny),
        Here(Keybit2),
        Load(ARG2_LOW),
        Jump(BPF_JGE_K, u32::from(BTN_MISC), Keybit3, Deny),
        Here(Keybit3),
        Jump(BPF_JGE_K, u32::from(KEY_OK), Deny, Allow),
        Here(Evbit),
        Load(ARG2_HIGH),
        Jump(BPF_JEQ_K, 0, Evbit2, Deny),
        Here(Evbit2),
        Load(ARG2_LOW),
        Jump(BPF_JGT_K, u32::from(EV_MSC), Deny, Allow),
        Here(Allow),
        Return(SECCOMP_RET_ALLOW),
        Here(Deny),
        Return(deny),
        Here(Kill),
        Return(SECCOMP_RET_KILL_PROCESS),
    ]);
    assemble(&ops)
}

/// Resolve labels. `Next` always means the next `Here(Next)` after the jump;
/// every other label is unique.
fn assemble(ops: &[Op]) -> Vec<libc::sock_filter> {
    let mut positions = Vec::new();
    let mut index = 0usize;
    for op in ops {
        match op {
            Op::Here(label) => positions.push((index, *label)),
            _ => index += 1,
        }
    }
    let target = |from: usize, label: Label| -> u8 {
        let to = positions
            .iter()
            .find(|(at, l)| *l == label && (label != Label::Next || *at > from))
            .map(|(at, _)| *at)
            .expect("every label is placed");
        u8::try_from(to - from - 1).expect("a jump fits in eight bits")
    };
    let mut out = Vec::new();
    for op in ops {
        let at = out.len();
        match op {
            Op::Here(_) => {}
            Op::Load(offset) => out.push(libc::sock_filter {
                code: BPF_LD_W_ABS,
                jt: 0,
                jf: 0,
                k: *offset,
            }),
            Op::Jump(code, value, yes, no) => out.push(libc::sock_filter {
                code: *code,
                jt: target(at, *yes),
                jf: target(at, *no),
                k: *value,
            }),
            Op::Return(value) => out.push(libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: *value,
            }),
        }
    }
    out
}

/// Install the filter on the calling thread and every thread it starts
/// afterwards. `no_new_privs` must already be set.
pub fn install() -> io::Result<()> {
    let filter = program();
    let program = libc::sock_fprog {
        len: u16::try_from(filter.len()).expect("a short program"),
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: `program` points at `filter`, which outlives the call; the
    // kernel copies it.
    let result = unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &program as *const libc::sock_fprog,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The errno an `ioctl` on a closed descriptor gives under the filter:
    /// `EPERM` when the filter refused it, `EBADF` when it reached the kernel.
    fn errno_of(request: u32, argument: u64) -> i32 {
        // SAFETY: descriptor -1 is never open; the call cannot act on anything.
        let result = unsafe { libc::ioctl(-1, request as _, argument) };
        assert_eq!(result, -1);
        io::Error::last_os_error().raw_os_error().unwrap()
    }

    #[test]
    fn the_filter_refuses_every_keyboard_capability_in_a_child_process() {
        // SAFETY: the child only makes system calls and exits; it touches no
        // lock another test thread could hold.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let code = (|| {
                // SAFETY: plain prctl.
                if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
                    return 10;
                }
                if install().is_err() {
                    return 11;
                }
                let cases: [(u32, u64, i32); 12] = [
                    (UI_SET_KEYBIT, 30, libc::EPERM),            // KEY_A
                    (UI_SET_KEYBIT, 0xff, libc::EPERM),          // last key below buttons
                    (UI_SET_KEYBIT, 0x160, libc::EPERM),         // KEY_OK
                    (UI_SET_KEYBIT, 0x1_0000_0110, libc::EPERM), // high word set
                    (UI_SET_KEYBIT, 0x110, libc::EBADF),         // BTN_LEFT reaches the kernel
                    (UI_SET_KEYBIT, 0x14d, libc::EBADF),         // BTN_TOOL_DOUBLETAP
                    (UI_SET_EVBIT, 0x11, libc::EPERM),           // EV_LED
                    (UI_SET_EVBIT, 0x14, libc::EPERM),           // EV_REP
                    (UI_SET_EVBIT, 3, libc::EBADF),              // EV_ABS
                    (UI_SET_LEDBIT, 0, libc::EPERM),
                    (UI_SET_SWBIT, 0, libc::EPERM),
                    (UI_SET_FFBIT, 0, libc::EPERM),
                ];
                for (index, (request, argument, expected)) in cases.iter().enumerate() {
                    if errno_of(*request, *argument) != *expected {
                        return 20 + index as i32;
                    }
                }
                if errno_of(UI_SET_SNDBIT, 0) != libc::EPERM {
                    return 40;
                }
                0
            })();
            // SAFETY: leave the child without running the parent's destructors.
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        // SAFETY: waiting for the child this test forked.
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(libc::WIFEXITED(status), "the child was killed: {status:#x}");
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "case code {}",
            libc::WEXITSTATUS(status)
        );
    }

    #[test]
    fn every_jump_lands_inside_the_program() {
        let program = program();
        for (index, op) in program.iter().enumerate() {
            if op.code & 0x07 == 0x05 {
                assert!(index + 1 + usize::from(op.jt) < program.len());
                assert!(index + 1 + usize::from(op.jf) < program.len());
            }
        }
        assert_eq!(program.last().unwrap().code, BPF_RET_K);
    }
}

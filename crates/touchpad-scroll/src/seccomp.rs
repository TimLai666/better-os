//! What the service may still ask uinput for, once it has dropped privilege.
//!
//! The uinput descriptor is the one dangerous thing the service keeps: with it
//! a process can declare a device that reports keyboard keys and type into the
//! session, or an absolute pointer (a touchscreen, a tablet) that could click
//! any key of an on-screen keyboard. The kernel only stops that at the
//! descriptor's permissions, which the service already passed. So the service
//! filters itself, in two steps:
//!
//! 1. [`install`], right after the privilege drop and before the clone exists,
//!    checks the arguments of every uinput capability call against what a
//!    touchpad is ([`crate::caps::classify`] accepts the same sets):
//!    `UI_SET_KEYBIT` only for [`TOUCHPAD_KEYS`] (mouse buttons and finger
//!    tools: no key, no pen, no stylus), `UI_SET_RELBIT` only for the four
//!    wheel codes (no `REL_X`/`REL_Y`, so no relative mouse), `UI_SET_PROPBIT`
//!    only for [`TOUCHPAD_PROPS`] (never `INPUT_PROP_DIRECT`, a touchscreen),
//!    `UI_SET_EVBIT` only for `SYN`, `KEY`, `REL`, `ABS`, `MSC`, and never
//!    `UI_SET_SWBIT`, `UI_SET_LEDBIT`, `UI_SET_FFBIT`, `UI_SET_SNDBIT`.
//! 2. [`lock`], right after the clone is created, forbids every uinput setup
//!    call at all (`UI_DEV_SETUP`, `UI_ABS_SETUP`, `UI_SET_*`, `UI_DEV_CREATE`,
//!    and the rest of the `'U'` requests) except `UI_DEV_DESTROY`. The clone
//!    is the one device this process can ever have declared, and it is built
//!    before a single input event has been read.
//!
//! `UI_SET_ABSBIT` has no range of its own in step 1: whether a set of
//! absolute axes is a touchpad or an absolute pointer depends on the other
//! capabilities beside it (a finger tool, no `INPUT_PROP_DIRECT`), which a
//! filter on one call cannot see. Step 2 is what closes that: after the lock
//! no combination can be declared any more, and before it the only code that
//! runs is the service's own setup, judged by `classify`.
//!
//! A refused call fails with `EPERM`. A system call from another architecture
//! (or the x32 ABI) kills the process. Every other call is left to the unit's
//! own `SystemCallFilter=`.

#![allow(unsafe_code)]

use std::io;

use crate::codes::{EV_MSC, TOUCHPAD_KEYS, TOUCHPAD_PROPS, WHEEL_RELS};

const fn uinput_set(number: u8) -> u32 {
    // _IOW('U', number, int)
    (1 << 30) | (4 << 16) | ((b'U' as u32) << 8) | number as u32
}

pub const UI_SET_EVBIT: u32 = uinput_set(100);
pub const UI_SET_KEYBIT: u32 = uinput_set(101);
pub const UI_SET_RELBIT: u32 = uinput_set(102);
pub const UI_SET_ABSBIT: u32 = uinput_set(103);
pub const UI_SET_LEDBIT: u32 = uinput_set(105);
pub const UI_SET_SNDBIT: u32 = uinput_set(106);
pub const UI_SET_FFBIT: u32 = uinput_set(107);
pub const UI_SET_SWBIT: u32 = uinput_set(109);
pub const UI_SET_PROPBIT: u32 = uinput_set(110);
/// `_IO('U', 1)` and `_IO('U', 2)`.
pub const UI_DEV_CREATE: u32 = 0x5501;
pub const UI_DEV_DESTROY: u32 = 0x5502;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xc000_00b7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("the seccomp filter knows x86_64 and aarch64 only");

const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const DENY: u32 = SECCOMP_RET_ERRNO | libc::EPERM as u32;

// Offsets into `struct seccomp_data`, little endian.
const NR: u32 = 0;
const ARCH: u32 = 4;
const ARG1_LOW: u32 = 16 + 8;
const ARG2_LOW: u32 = 16 + 16;
const ARG2_HIGH: u32 = 16 + 16 + 4;

// BPF_LD (0x00) | BPF_W (0x00) | BPF_ABS (0x20).
const BPF_LD_W_ABS: u16 = 0x20;
const BPF_ALU_AND_K: u16 = 0x04 | 0x50;
const BPF_JEQ_K: u16 = 0x05 | 0x10;
const BPF_JGE_K: u16 = 0x05 | 0x30;
const BPF_JGT_K: u16 = 0x05 | 0x20;
const BPF_RET_K: u16 = 0x06;

/// A jump target. `Next` is the instruction right after the jump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Label {
    Next,
    Named(u32),
}

enum Op {
    Load(u32),
    And(u32),
    Jump(u16, u32, Label, Label),
    Return(u32),
    Here(u32),
}

const ALLOW: Label = Label::Named(0);
const REFUSE: Label = Label::Named(1);
const KILL: Label = Label::Named(2);
const IOCTL: Label = Label::Named(3);

/// The prologue both programs share: right architecture, and on to `IOCTL`
/// for `ioctl`, `ALLOW` for anything else.
fn prologue() -> Vec<Op> {
    let mut ops = vec![
        Op::Load(ARCH),
        Op::Jump(BPF_JEQ_K, AUDIT_ARCH, Label::Next, KILL),
        Op::Load(NR),
    ];
    #[cfg(target_arch = "x86_64")]
    ops.push(Op::Jump(BPF_JGE_K, 0x4000_0000, KILL, Label::Next));
    ops.push(Op::Jump(BPF_JEQ_K, libc::SYS_ioctl as u32, IOCTL, ALLOW));
    ops
}

fn epilogue(ops: &mut Vec<Op>) {
    ops.extend([
        Op::Here(0),
        Op::Return(SECCOMP_RET_ALLOW),
        Op::Here(1),
        Op::Return(DENY),
        Op::Here(2),
        Op::Return(SECCOMP_RET_KILL_PROCESS),
    ]);
}

/// The argument must have a zero high word and be one of `allowed`.
fn argument_in(ops: &mut Vec<Op>, label: u32, allowed: &[u16]) {
    ops.push(Op::Here(label));
    ops.push(Op::Load(ARG2_HIGH));
    ops.push(Op::Jump(BPF_JEQ_K, 0, Label::Next, REFUSE));
    ops.push(Op::Load(ARG2_LOW));
    for code in allowed {
        ops.push(Op::Jump(BPF_JEQ_K, u32::from(*code), ALLOW, Label::Next));
    }
    ops.push(Op::Return(DENY));
}

/// Step 1: a touchpad's capabilities and nothing else.
fn capability_program() -> Vec<libc::sock_filter> {
    const KEYS: u32 = 10;
    const RELS: u32 = 11;
    const PROPS: u32 = 12;
    const EVENTS: u32 = 13;
    let mut ops = prologue();
    ops.push(Op::Here(3));
    ops.push(Op::Load(ARG1_LOW));
    for (request, target) in [
        (UI_SET_KEYBIT, Label::Named(KEYS)),
        (UI_SET_RELBIT, Label::Named(RELS)),
        (UI_SET_PROPBIT, Label::Named(PROPS)),
        (UI_SET_EVBIT, Label::Named(EVENTS)),
        (UI_SET_SWBIT, REFUSE),
        (UI_SET_LEDBIT, REFUSE),
        (UI_SET_FFBIT, REFUSE),
        (UI_SET_SNDBIT, REFUSE),
    ] {
        ops.push(Op::Jump(BPF_JEQ_K, request, target, Label::Next));
    }
    ops.push(Op::Return(SECCOMP_RET_ALLOW));
    argument_in(&mut ops, KEYS, &TOUCHPAD_KEYS);
    argument_in(&mut ops, RELS, &WHEEL_RELS);
    argument_in(&mut ops, PROPS, &TOUCHPAD_PROPS);
    ops.push(Op::Here(EVENTS));
    ops.push(Op::Load(ARG2_HIGH));
    ops.push(Op::Jump(BPF_JEQ_K, 0, Label::Next, REFUSE));
    ops.push(Op::Load(ARG2_LOW));
    ops.push(Op::Jump(BPF_JGT_K, u32::from(EV_MSC), REFUSE, ALLOW));
    epilogue(&mut ops);
    assemble(&ops)
}

/// Step 2: no uinput setup call at all, except destroying the clone.
fn lock_program() -> Vec<libc::sock_filter> {
    let mut ops = prologue();
    ops.push(Op::Here(3));
    ops.push(Op::Load(ARG1_LOW));
    ops.push(Op::Jump(BPF_JEQ_K, UI_DEV_DESTROY, ALLOW, Label::Next));
    // The request's type byte: every uinput request is type 'U'.
    ops.push(Op::And(0xff00));
    ops.push(Op::Jump(BPF_JEQ_K, u32::from(b'U') << 8, REFUSE, ALLOW));
    epilogue(&mut ops);
    assemble(&ops)
}

fn assemble(ops: &[Op]) -> Vec<libc::sock_filter> {
    let mut positions = Vec::new();
    let mut index = 0usize;
    for op in ops {
        match op {
            Op::Here(label) => positions.push((Label::Named(*label), index)),
            _ => index += 1,
        }
    }
    let target = |from: usize, label: Label| -> u8 {
        let to = match label {
            Label::Next => from + 1,
            named => positions
                .iter()
                .find(|(l, _)| *l == named)
                .map(|(_, at)| *at)
                .expect("every label is placed"),
        };
        u8::try_from(to - from - 1).expect("a forward jump that fits in eight bits")
    };
    let mut out = Vec::new();
    for op in ops {
        let at = out.len();
        let instruction = match op {
            Op::Here(_) => continue,
            Op::Load(offset) => (BPF_LD_W_ABS, 0, 0, *offset),
            Op::And(mask) => (BPF_ALU_AND_K, 0, 0, *mask),
            Op::Jump(code, value, yes, no) => (*code, target(at, *yes), target(at, *no), *value),
            Op::Return(value) => (BPF_RET_K, 0, 0, *value),
        };
        out.push(libc::sock_filter {
            code: instruction.0,
            jt: instruction.1,
            jf: instruction.2,
            k: instruction.3,
        });
    }
    out
}

fn load(filter: Vec<libc::sock_filter>) -> io::Result<()> {
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

/// Step 1, on the calling thread and every thread it starts afterwards.
/// `no_new_privs` must already be set.
pub fn install() -> io::Result<()> {
    load(capability_program())
}

/// Step 2. Filters stack: both stay in force.
pub fn lock() -> io::Result<()> {
    load(lock_program())
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

    /// Run `body` in a forked child with `no_new_privs` set, and return its
    /// exit status. The child makes system calls and exits; it touches no lock
    /// another test thread could hold.
    fn in_child(body: fn() -> i32) -> i32 {
        // SAFETY: see above.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            // SAFETY: plain prctl, then leave without the parent's destructors.
            unsafe {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    libc::_exit(100);
                }
                libc::_exit(body());
            }
        }
        let mut status = 0;
        // SAFETY: waiting for the child this test forked.
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(libc::WIFEXITED(status), "the child was killed: {status:#x}");
        libc::WEXITSTATUS(status)
    }

    fn check(cases: &[(u32, u64, i32)]) -> i32 {
        for (index, (request, argument, expected)) in cases.iter().enumerate() {
            if errno_of(*request, *argument) != *expected {
                return 1 + index as i32;
            }
        }
        0
    }

    #[test]
    fn step_one_allows_a_touchpad_and_nothing_that_types_or_points_absolutely() {
        let status = in_child(|| {
            if install().is_err() {
                return 99;
            }
            check(&[
                (UI_SET_KEYBIT, 30, libc::EPERM),            // KEY_A
                (UI_SET_KEYBIT, 0xff, libc::EPERM),          // last key below buttons
                (UI_SET_KEYBIT, 0x160, libc::EPERM),         // KEY_OK
                (UI_SET_KEYBIT, 0x140, libc::EPERM),         // BTN_TOOL_PEN
                (UI_SET_KEYBIT, 0x14b, libc::EPERM),         // BTN_STYLUS
                (UI_SET_KEYBIT, 0x1_0000_0110, libc::EPERM), // high word set
                (UI_SET_KEYBIT, 0x110, libc::EBADF),         // BTN_LEFT reaches the kernel
                (UI_SET_KEYBIT, 0x14d, libc::EBADF),         // BTN_TOOL_DOUBLETAP
                (UI_SET_RELBIT, 0, libc::EPERM),             // REL_X
                (UI_SET_RELBIT, 1, libc::EPERM),             // REL_Y
                (UI_SET_RELBIT, 8, libc::EBADF),             // REL_WHEEL
                (UI_SET_RELBIT, 0x0b, libc::EBADF),          // REL_WHEEL_HI_RES
                (UI_SET_PROPBIT, 1, libc::EPERM),            // INPUT_PROP_DIRECT
                (UI_SET_PROPBIT, 5, libc::EPERM),            // INPUT_PROP_POINTING_STICK
                (UI_SET_PROPBIT, 0, libc::EBADF),            // INPUT_PROP_POINTER
                (UI_SET_PROPBIT, 2, libc::EBADF),            // INPUT_PROP_BUTTONPAD
                (UI_SET_EVBIT, 0x11, libc::EPERM),           // EV_LED
                (UI_SET_EVBIT, 0x14, libc::EPERM),           // EV_REP
                (UI_SET_EVBIT, 3, libc::EBADF),              // EV_ABS
                (UI_SET_LEDBIT, 0, libc::EPERM),
                (UI_SET_SWBIT, 0, libc::EPERM),
                (UI_SET_FFBIT, 0, libc::EPERM),
                (UI_SET_SNDBIT, 0, libc::EPERM),
                (UI_DEV_CREATE, 0, libc::EBADF), // still allowed before the lock
            ])
        });
        assert_eq!(status, 0, "case {status} failed");
    }

    #[test]
    fn after_the_lock_no_uinput_device_can_be_declared_at_all() {
        let status = in_child(|| {
            if install().is_err() || lock().is_err() {
                return 99;
            }
            check(&[
                (UI_DEV_CREATE, 0, libc::EPERM),
                (UI_SET_KEYBIT, 0x110, libc::EPERM),
                (UI_SET_ABSBIT, 0, libc::EPERM),
                (UI_SET_EVBIT, 3, libc::EPERM),
                (0x405c_5503, 0, libc::EPERM), // UI_DEV_SETUP
                (0x401c_5504, 0, libc::EPERM), // UI_ABS_SETUP
                (UI_DEV_DESTROY, 0, libc::EBADF),
                (0x4004_4590, 0, libc::EBADF), // EVIOCGRAB is not uinput's
            ])
        });
        assert_eq!(status, 0, "case {status} failed");
    }

    #[test]
    fn the_filter_and_classify_accept_the_same_touchpad() {
        let touchpad = crate::caps::fixtures::touchpad_with_wheel();
        assert!(
            touchpad
                .keys
                .iter()
                .all(|code| TOUCHPAD_KEYS.contains(&code))
        );
        assert!(
            touchpad
                .relative
                .iter()
                .all(|code| WHEEL_RELS.contains(&code))
        );
        assert!(
            touchpad
                .properties
                .iter()
                .all(|code| TOUCHPAD_PROPS.contains(&code))
        );
        assert_eq!(crate::caps::classify(&touchpad), Ok(()));
    }

    #[test]
    fn every_jump_lands_inside_the_program() {
        for program in [capability_program(), lock_program()] {
            for (index, op) in program.iter().enumerate() {
                if op.code & 0x07 == 0x05 {
                    assert!(index + 1 + usize::from(op.jt) < program.len());
                    assert!(index + 1 + usize::from(op.jf) < program.len());
                }
            }
            assert_eq!(program.last().unwrap().code, BPF_RET_K);
        }
    }
}

//! One evdev event, without the kernel's timestamp.
//!
//! The kernel stamps an event when it is written to a uinput device, and
//! libinput takes a touchpad's timing from `MSC_TIMESTAMP`, which is forwarded
//! untouched. So the pass-through carries type, code, and value only.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl InputEvent {
    pub const fn new(kind: u16, code: u16, value: i32) -> Self {
        Self { kind, code, value }
    }

    pub const fn syn() -> Self {
        Self::new(crate::codes::EV_SYN, crate::codes::SYN_REPORT, 0)
    }

    pub fn is_report(&self) -> bool {
        self.kind == crate::codes::EV_SYN && self.code == crate::codes::SYN_REPORT
    }

    pub fn is_dropped(&self) -> bool {
        self.kind == crate::codes::EV_SYN && self.code == crate::codes::SYN_DROPPED
    }
}

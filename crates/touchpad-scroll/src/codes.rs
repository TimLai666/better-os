//! The evdev codes this crate reads, from `linux/input-event-codes.h`.

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const EV_MSC: u16 = 0x04;
pub const EV_SW: u16 = 0x05;
pub const EV_LED: u16 = 0x11;
pub const EV_SND: u16 = 0x12;
pub const EV_REP: u16 = 0x14;
pub const EV_FF: u16 = 0x15;
pub const EV_MAX: u16 = 0x1f;

pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;

pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_HWHEEL: u16 = 0x06;
pub const REL_WHEEL: u16 = 0x08;
pub const REL_WHEEL_HI_RES: u16 = 0x0b;
pub const REL_HWHEEL_HI_RES: u16 = 0x0c;
pub const REL_MAX: u16 = 0x0f;

pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
pub const ABS_MT_SLOT: u16 = 0x2f;
pub const ABS_MT_POSITION_X: u16 = 0x35;
pub const ABS_MT_POSITION_Y: u16 = 0x36;
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const ABS_MAX: u16 = 0x3f;

pub const MSC_MAX: u16 = 0x07;

/// The first code that is a button rather than a key.
pub const BTN_MISC: u16 = 0x100;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_TOOL_PEN: u16 = 0x140;
pub const BTN_TOOL_FINGER: u16 = 0x145;
pub const BTN_TOOL_QUINTTAP: u16 = 0x148;
pub const BTN_TOUCH: u16 = 0x14a;
pub const BTN_STYLUS: u16 = 0x14b;
pub const BTN_TOOL_DOUBLETAP: u16 = 0x14d;
pub const BTN_TOOL_TRIPLETAP: u16 = 0x14e;
pub const BTN_TOOL_QUADTAP: u16 = 0x14f;
/// `KEY_OK`: the first code past the button block. Everything from here up is
/// a key again (remote-control and media keys), and so is everything below
/// [`BTN_MISC`].
pub const KEY_OK: u16 = 0x160;
pub const KEY_MAX: u16 = 0x2ff;

pub const INPUT_PROP_POINTER: u16 = 0x00;
pub const INPUT_PROP_DIRECT: u16 = 0x01;
pub const INPUT_PROP_BUTTONPAD: u16 = 0x02;
pub const INPUT_PROP_SEMI_MT: u16 = 0x03;
pub const INPUT_PROP_MAX: u16 = 0x1f;

pub const BUS_USB: u16 = 0x03;
pub const BUS_BLUETOOTH: u16 = 0x05;

/// Every finger-count tool a touchpad reports.
pub const FINGER_TOOLS: [u16; 5] = [
    BTN_TOOL_FINGER,
    BTN_TOOL_DOUBLETAP,
    BTN_TOOL_TRIPLETAP,
    BTN_TOOL_QUADTAP,
    BTN_TOOL_QUINTTAP,
];

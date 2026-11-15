//! Better Touchpad's scroll-speed pass-through (ADR 0030).
//!
//! GNOME 46 has no scroll-speed setting and a GNOME Shell extension cannot see
//! a client's scroll events (ADR 0029's experiment), so the factor is applied
//! below the compositor: one service per touchpad grabs that touchpad's evdev
//! node, scales two-finger motion and wheel motion, and re-emits every event on
//! a uinput clone that libinput uses instead.
//!
//! Everything that decides something lives here, behind the [`passthrough`]
//! seam, and is tested without a real device:
//!
//! - [`caps`] is the only rule for which node may be opened: a touchpad,
//!   matched by its capability bits, and never anything that can report a key.
//! - [`transform`] is the scaling of one frame.
//! - [`passthrough`] decides when the grab is taken and released, and releases
//!   it on every way out, a panic included.
//!
//! [`linux`] is the thin ioctl layer over the real nodes, and the only module
//! that calls into the kernel. `engineering/specs/touchpad-input-passthrough-security.md`
//! is the security review every claim here is tested against.

pub mod caps;
pub mod codes;
pub mod event;
pub mod factors;
pub mod ipc;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod notify;
pub mod passthrough;
pub mod session_rule;
pub mod sysfs;
pub mod transform;

pub use caps::{DeviceDescriptor, Refusal, classify, classify_capabilities, clone_descriptor};
pub use event::InputEvent;
pub use factors::{Factors, SharedFactors};
pub use passthrough::{Passthrough, Sink, SinkFactory, Source};
pub use transform::Scaler;

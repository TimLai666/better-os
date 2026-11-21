//! What the Better OS login screen and lock screen share, with no GPUI
//! (design/login-screen-spec.md section 5.1, ticket 260).
//!
//! `better-greeter` (ticket 264, under greetd) and `better-lock` (ticket 262,
//! in the Better OS session) draw the same screen; every rule they share is
//! here, so the two GPUI programs stay thin:
//!
//! - [`view`]: the states (quiet, entry, chooser, session menu, password
//!   change, starting, failed to start, power overlay), every transition, and
//!   when the password field is wiped;
//! - [`layout`]: the wide and narrow layouts, large text and right-to-left,
//!   as numbers;
//! - [`secret`] and [`greetd`]: the password buffer and the one message that
//!   carries it;
//! - [`wait`]: the wait after failures;
//! - [`accounts`], [`sessions`], [`keyboard`], [`pam`], [`power`]: the
//!   chooser, the session menu, the layout indicator, PAM's words, the power
//!   menu;
//! - [`lock_content`]: the lock screen's notifications and media row;
//! - [`status`]: the status row's battery, from the one battery rule
//!   (`hardware_core::charge_state::battery_look`);
//! - [`clock`]: the date and time;
//! - [`copy`]: every sentence of the spec's tables in zh-TW and English.

pub mod accounts;
pub mod clock;
pub mod copy;
pub mod greetd;
pub mod keyboard;
pub mod layout;
pub mod lock_content;
pub mod pam;
pub mod power;
pub mod secret;
pub mod sessions;
pub mod status;
pub mod view;
pub mod wait;

pub use copy::Language;
pub use secret::{SECRET_CAPACITY, Secret, SecretFull};
pub use view::{Effects, Event, Request, Screen, ScreenKind, View};

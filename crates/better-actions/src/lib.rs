//! The typed desktop actions Better OS components share.
//!
//! One crate answers one question: what is it that a gesture, a shortcut, or a
//! menu item can ask the desktop to do? The answer is a closed enum
//! ([`DesktopAction`]), a validated custom shortcut ([`KeyboardShortcut`]), and
//! a per-action capability report ([`ActionCapabilities`]).
//!
//! Two rules make this worth its own crate.
//!
//! **Arbitrary execution is unrepresentable, not merely forbidden.** There is
//! no variant carrying a command, a path, or free text. The only user text in
//! the catalog is a keyboard shortcut, and its key comes from a fixed table
//! behind a private field, so a stored file, a D-Bus message, or a GUI field
//! cannot smuggle one in. Issue #3 puts shell execution out of scope; this is
//! how that survives the next feature request.
//!
//! **Being in the catalog is not a promise.** Whether any given desktop can
//! perform an action is an adapter's answer, reported per action through
//! [`ActionSupport`], including whether it can follow a gesture's progress.
//! A component renders the reason it cannot, never a control that does nothing.
//!
//! There is no desktop code here at all: no D-Bus, no compositor, no process.
//! `touchpad-session` owns the boundary that actually invokes one of these.

pub mod catalog;
pub mod key;
pub mod support;

pub use catalog::DesktopAction;
pub use key::{Key, KeyboardShortcut, Modifier, ShortcutError};
pub use support::{ActionCapabilities, ActionSupport};

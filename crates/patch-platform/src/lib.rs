//! The I/O Better Patch's entries need, behind seams tests replace
//! (ticket 90, `engineering/specs/better-patch.md`).
//!
//! [`host::LinuxHost`] answers `patch-core`'s reads and makes its changes:
//! it reads the installed Flatpak applications and their override files,
//! edits the person's own override file only while it still holds what was
//! read, asks Mutter for the display scale, and runs the closed list of
//! `patch_core::Command`s through [`command::CommandRunner`]. Nothing here
//! starts a shell, runs a script, or takes a program or an argument from
//! data, and nothing needs root.
//!
//! [`fixture`] is a machine made of files, which the demo window and the
//! sandbox screenshots use, so neither ever touches the person's real font
//! cache or Flatpak overrides.

pub mod command;
pub mod display;
pub mod fixture;
pub mod flatpak;
pub mod host;
pub mod paths;
pub mod removal;
pub mod store;

pub use host::LinuxHost;
pub use paths::Paths;
pub use store::RecordStore;

//! Better Patch: known Linux desktop faults, found, fixed, checked, and
//! undone (ticket 90, ADR 0017, `engineering/specs/better-patch.md`).
//!
//! No GPUI, no I/O, and no words. Every entry is typed Rust in this crate:
//! it says what it needs to read, decides from the answers whether its fault
//! is on this machine, plans the exact change that fixes it, checks the fix
//! from a fresh reading, and plans the undo from the [`record`] of what was
//! changed, so an undo takes back exactly that and nothing else.
//!
//! `patch-platform` answers the reads and makes the changes through the
//! [`entry::Host`] seam. What it may run is the closed [`command::Command`]
//! list, each a fixed program with fixed arguments; nothing here can name a
//! program, a shell, or a script, and nothing can be fetched or supplied at
//! run time.

pub mod command;
pub mod engine;
pub mod entries;
pub mod entry;
pub mod flatpak;
pub mod keyfile;
pub mod record;
pub mod state;

pub use command::{Command, FontQuery};
pub use engine::{FixError, FixOutcome, Instance, Report, UndoOutcome};
pub use entry::{
    Answer, Answers, CannotCheck, Change, ChangeError, CheckOutcome, Detection, Entry, EntryId,
    Finding, Host, Read, ReadError, Subject, UndoKind,
};
pub use record::{AppliedRecord, Records};
pub use state::EntryState;

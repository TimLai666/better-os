//! Which drives mount automatically (ticket 127, ADR 0032): the drives a
//! person would call one, Better OS's block in `/etc/fstab` and its udev
//! rules file, the choice each drive has, and the windows' calls to
//! `manager-daemon`. Better Files, Better Manager and the daemon all read
//! the system's files through here, so the three never disagree.

pub mod choice;
pub mod client;
pub mod fstab;
pub mod ids;
pub mod listing;
pub mod rules;

//! Better Awake's trigger providers: the only crate that reads the machine.
//!
//! `awake-core` decides what a rule means; this crate finds out what is actually
//! true. Everything here goes through a [`Roots`] seam, so the code that runs in
//! production against `/proc` and `/sys` is the same code the tests run against
//! a captured tree — a parser proved against a fixture is a parser proved.
//!
//! # Three rules every provider follows
//!
//! **It reports its own capability.** A provider that cannot read its source
//! says so with a stable key naming the path, and the conditions that needed it
//! evaluate to unknown. Unknown never becomes true, so an unreadable provider
//! never keeps the machine awake, and it never becomes a silent false either, so
//! the rule editor can explain a control instead of showing an inert one.
//!
//! **It states its cadence.** Nothing here spins. Six providers poll, at
//! intervals documented in [`provider`]; one is event-driven through `inotify`;
//! two answer with no I/O at all. A machine with no rules samples nothing.
//!
//! **It reads no more than the question needs.** `/proc/<pid>/cmdline` is never
//! opened, so a password or a document name on a command line cannot reach a
//! history file by way of this crate. The watched-path provider records one
//! timestamp per path and never what changed.
//!
//! # Availability on a real machine
//!
//! Nine of the eleven providers Issue #13 lists work here. Fullscreen state
//! cannot be read without a compositor adapter and reports itself unavailable
//! with that explanation; see [`fullscreen`] for why, and ticket 26's deferred
//! decisions for the ADR that owns it. Audio playback works through ALSA and has
//! documented limits — Bluetooth sinks are not visible to it — recorded in
//! [`audio`] rather than glossed over.

pub mod audio;
pub mod cpu;
pub mod display;
pub mod fullscreen;
pub mod network;
pub mod power;
pub mod process;
pub mod provider;
pub mod roots;
pub mod sampler;
pub mod schedule;
pub mod watch;

pub use audio::AudioProvider;
pub use cpu::{CpuProvider, CpuTimes};
pub use display::{Connector, DisplayProvider};
pub use fullscreen::{FULLSCREEN_UNAVAILABLE, FullscreenProvider};
pub use network::{InterfaceBytes, NetworkProvider, parse_net_dev};
pub use power::{PowerProvider, PowerReading};
pub use process::{ProcessProvider, ProcessScan, desktop_id_from_cgroup};
pub use provider::{
    AUDIO_POLL_SECONDS, CPU_POLL_SECONDS, Cadence, DISPLAY_POLL_SECONDS, NETWORK_POLL_SECONDS,
    POWER_POLL_SECONDS, PROCESS_POLL_SECONDS, ProviderReport, TriggerProvider,
};
pub use roots::{ReadError, Roots};
pub use sampler::ProviderSet;
pub use schedule::{ScheduleProvider, local_time};
pub use watch::{WatchLog, WatchProvider};

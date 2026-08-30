//! What a trigger provider is, and what it must be honest about.
//!
//! Two rules hold for every provider in this crate.
//!
//! A provider reports its own capability. If it cannot read its source it says
//! so with a stable key naming the path, and the condition that needed it
//! evaluates to unknown rather than to false. Issue #13 requires an unavailable
//! provider to show an explanation instead of an inert control, and this is
//! where that explanation comes from.
//!
//! A provider states its cadence, and the sampler honours it. Nothing here
//! spins: a poll interval is a documented number of seconds, an event-driven
//! provider does no work between events, and a provider that needs no I/O at all
//! says that too, so the idle cost of a rule set is the sum of a small list of
//! numbers rather than a mystery.

use awake_core::{Observations, ProviderKind};

/// How often a provider must be re-read, and what that costs when idle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cadence {
    /// Re-read every `seconds`. The only shape that costs anything when nothing
    /// is happening.
    Poll { seconds: u64 },
    /// Updated by kernel events. Costs nothing between them.
    EventDriven,
    /// Answered by arithmetic with no I/O at all.
    Free,
}

impl Cadence {
    pub fn poll_seconds(self) -> Option<u64> {
        match self {
            Cadence::Poll { seconds } => Some(seconds),
            Cadence::EventDriven | Cadence::Free => None,
        }
    }

    pub fn as_key(self) -> &'static str {
        match self {
            Cadence::Poll { .. } => "poll",
            Cadence::EventDriven => "event_driven",
            Cadence::Free => "free",
        }
    }
}

/// Something that can answer one kind of condition.
pub trait TriggerProvider: Send {
    fn kind(&self) -> ProviderKind;

    /// How often this provider needs re-reading, and what it costs when idle.
    fn cadence(&self) -> Cadence;

    /// Reads the source once and records the answer, or records why it could
    /// not. A provider must always write either a reading or an unavailability
    /// into `into`; leaving both out would present a gap with no explanation.
    fn sample(&mut self, now_unix_seconds: u64, into: &mut Observations);
}

/// The poll intervals every polling provider in this crate uses.
///
/// They are collected here rather than scattered through the files so the idle
/// cost of Better Awake is one table a reviewer can read, and so a change to one
/// of them is a change to a documented number rather than a magic constant
/// buried in a loop.
///
/// | Provider | Cadence | Why |
/// | --- | --- | --- |
/// | Process running | 5 s | A build starting or ending must be noticed within a few seconds to feel like it works. One `readdir` of `/proc` plus one small read per process. |
/// | AC power / battery | 10 s | Charger state and battery percentage both move slowly. Two small `/sys` reads. |
/// | External display | 10 s | A monitor being plugged in is not urgent to the second, and `/sys/class/drm` has one small read per connector. |
/// | Audio playback | 5 s | Matches the process cadence because both answer "is something still going". One small read per ALSA substream. |
/// | CPU utilization | 5 s | A utilization figure is a delta between two samples, so the interval is also the averaging window. |
/// | Network throughput | 5 s | Same reason as CPU: the interval is the window the rate is measured over. |
/// | Time schedule | free | Arithmetic on the clock. No I/O. |
/// | Watched path | event-driven | `inotify` through the `notify` crate. No work between events. |
/// | Fullscreen | free | Unavailable on this platform; answering costs nothing. |
pub const PROCESS_POLL_SECONDS: u64 = 5;
pub const POWER_POLL_SECONDS: u64 = 10;
pub const DISPLAY_POLL_SECONDS: u64 = 10;
pub const AUDIO_POLL_SECONDS: u64 = 5;
pub const CPU_POLL_SECONDS: u64 = 5;
pub const NETWORK_POLL_SECONDS: u64 = 5;

/// What a provider says about itself, for the Diagnostics view and for the rule
/// editor's "this control does nothing here, and here is why" state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReport {
    pub kind: ProviderKind,
    pub cadence: Cadence,
    pub available: bool,
    /// A stable key naming what is missing, when anything is.
    pub explanation: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_polling_cadence_has_an_interval_to_report() {
        assert_eq!(Cadence::Poll { seconds: 5 }.poll_seconds(), Some(5));
        assert_eq!(Cadence::EventDriven.poll_seconds(), None);
        assert_eq!(Cadence::Free.poll_seconds(), None);
    }

    #[test]
    fn no_polling_provider_reads_more_often_than_once_a_second() {
        // A provider that polled faster than this would be a busy loop wearing a
        // cadence, which Issue #13's performance requirements forbid.
        for seconds in [
            PROCESS_POLL_SECONDS,
            POWER_POLL_SECONDS,
            DISPLAY_POLL_SECONDS,
            AUDIO_POLL_SECONDS,
            CPU_POLL_SECONDS,
            NETWORK_POLL_SECONDS,
        ] {
            assert!(seconds >= 1, "a sub-second poll interval is a busy loop");
            assert!(seconds <= 60, "a minute-plus interval would feel broken");
        }
    }
}

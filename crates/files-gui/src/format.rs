//! Turning a fact into the text a cell shows.
//!
//! Every one of these is locale-independent on purpose. A size, a date, and a
//! transfer rate mean the same thing in both shipped languages, and the
//! overflow tests compare the same string in each — a translated thousands
//! separator would make those tests compare two different things.
//!
//! Times are on the local wall clock, through `better_core::local_time`
//! (`engineering/specs/files-gui-policy.md`, "Times are shown in local time").

use std::time::Duration;

use better_core::local_time::TimeZone;
use files_core::{EntryKind, EntrySize, FileTime};

/// A byte count, in the units a file manager uses.
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if value < 1024 {
        return format!("{value} B");
    }
    let mut amount = value as f64;
    let mut unit = 0usize;
    while amount >= 1024.0 && unit + 1 < UNITS.len() {
        amount /= 1024.0;
        unit += 1;
    }
    if amount >= 100.0 {
        format!("{amount:.0} {}", UNITS[unit])
    } else if amount >= 10.0 {
        format!("{amount:.1} {}", UNITS[unit])
    } else {
        format!("{amount:.2} {}", UNITS[unit])
    }
}

/// A transfer rate.
pub fn bytes_per_second(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "—".to_string();
    }
    format!("{}/s", bytes(value.round() as u64))
}

/// A duration, rounded the way a remaining-time readout should be: never more
/// precise than it can justify.
pub fn duration(value: Duration) -> String {
    let seconds = value.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// The size cell. A directory has no byte count of its own and says so rather
/// than showing zero, which is Issue #6's rule about a missing value never
/// rendering as a number.
pub fn entry_size(size: EntrySize) -> String {
    match size {
        EntrySize::Bytes(value) => bytes(value),
        _ => "—".to_string(),
    }
}

/// A timestamp as `YYYY-MM-DD HH:MM` on the local wall clock, through the
/// one conversion every Better OS window shares (`better_core::local_time`).
pub fn file_time(time: Option<FileTime>) -> String {
    file_time_in(time, TimeZone::System)
}

/// [`file_time`] in a given zone. A missing time is a dash, never a date.
pub fn file_time_in(time: Option<FileTime>, zone: TimeZone) -> String {
    match time {
        Some(time) => zone.at(time.seconds).calendar(),
        None => "—".to_string(),
    }
}

/// The type column: the MIME type when the platform detected one, and the kind
/// otherwise. Both are stable machine facts; the caller localizes the kind.
pub fn type_key(kind: EntryKind, mime: Option<&str>) -> Option<String> {
    match kind {
        EntryKind::Directory => None,
        _ => mime.map(|value| value.to_string()),
    }
}

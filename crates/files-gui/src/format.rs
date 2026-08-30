//! Turning a fact into the text a cell shows.
//!
//! Every one of these is locale-independent on purpose. A size, a date, and a
//! transfer rate mean the same thing in both shipped languages, and the
//! overflow tests compare the same string in each — a translated thousands
//! separator would make those tests compare two different things.
//!
//! Times are formatted in UTC. There is no time-zone database in this
//! workspace and inventing one from `TZ` would be wrong for half the year in
//! most of the world; `docs/files-gui-policy.md` records this as the one
//! display value that is not in the user's local time yet.

use std::time::Duration;

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

/// A timestamp as `YYYY-MM-DD HH:MM` in UTC.
pub fn file_time(time: Option<FileTime>) -> String {
    let Some(time) = time else {
        return "—".to_string();
    };
    let seconds = time.seconds;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60
    )
}

/// Days since the Unix epoch to a civil date. Howard Hinnant's `civil_from_days`,
/// which is exact for every day this program can be handed and needs no table.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The type column: the MIME type when the platform detected one, and the kind
/// otherwise. Both are stable machine facts; the caller localizes the kind.
pub fn type_key(kind: EntryKind, mime: Option<&str>) -> Option<String> {
    match kind {
        EntryKind::Directory => None,
        _ => mime.map(|value| value.to_string()),
    }
}

//! Enough of the local timezone to print `Started: 22:18`.
//!
//! The standard library only knows about UTC, and this window is forbidden from
//! running a command, so the offset is read out of `/etc/localtime` in the
//! version 1 TZif block every such file still carries. A missing or unreadable
//! file means an offset of zero and a time shown in UTC, which is a visible,
//! explainable fallback rather than a wrong local time invented from a guess.
//!
//! `awake-tray` carries the same reader for its menu. Merging the two belongs
//! in a shared crate, which ticket 26 does not own.

use std::path::Path;

/// Seconds to add to a UTC timestamp to get local wall-clock time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UtcOffset(pub(crate) i64);

impl UtcOffset {
    pub(crate) const UTC: Self = UtcOffset(0);

    /// The offset in force at `unix_seconds` according to the system timezone.
    pub(crate) fn for_system(unix_seconds: u64) -> Self {
        Self::from_tzif_file(Path::new("/etc/localtime"), unix_seconds).unwrap_or(Self::UTC)
    }

    pub(crate) fn from_tzif_file(path: &Path, unix_seconds: u64) -> Option<Self> {
        Self::from_tzif(&std::fs::read(path).ok()?, unix_seconds)
    }

    /// Parses the version 1 block of a TZif file. Version 1 stores 32-bit
    /// transition times, which run out in 2038; a window that prints a session
    /// start time is never asked about 2039.
    pub(crate) fn from_tzif(bytes: &[u8], unix_seconds: u64) -> Option<Self> {
        if bytes.len() < 44 || &bytes[0..4] != b"TZif" {
            return None;
        }
        let count = |offset: usize| -> usize {
            u32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]) as usize
        };
        let transition_count = count(32);
        let type_count = count(36);
        if type_count == 0 {
            return None;
        }

        let transitions_at = 44;
        let indices_at = transitions_at + transition_count * 4;
        let types_at = indices_at + transition_count;
        if bytes.len() < types_at + type_count * 6 {
            return None;
        }

        let now = i64::try_from(unix_seconds).ok()?;
        // Before the first recorded transition the first type is the one in
        // force, which is what every TZif reader does.
        let mut chosen = 0usize;
        for index in 0..transition_count {
            let at = transitions_at + index * 4;
            let transition = i64::from(i32::from_be_bytes([
                bytes[at],
                bytes[at + 1],
                bytes[at + 2],
                bytes[at + 3],
            ]));
            if transition > now {
                break;
            }
            chosen = usize::from(bytes[indices_at + index]);
        }
        if chosen >= type_count {
            return None;
        }
        let at = types_at + chosen * 6;
        Some(UtcOffset(i64::from(i32::from_be_bytes([
            bytes[at],
            bytes[at + 1],
            bytes[at + 2],
            bytes[at + 3],
        ]))))
    }
}

/// `HH:MM` in local time.
pub(crate) fn clock_time(unix_seconds: u64, offset: UtcOffset) -> String {
    let local = i64::try_from(unix_seconds).unwrap_or(0) + offset.0;
    // Rust's `%` keeps the sign of the dividend, and a negative timestamp must
    // still land on a positive time of day.
    let seconds_into_day = local.rem_euclid(86_400);
    format!(
        "{:02}:{:02}",
        seconds_into_day / 3_600,
        (seconds_into_day % 3_600) / 60
    )
}

/// `YYYY-MM-DD HH:MM` in local time, which is what a history row needs: two
/// sessions a day apart must not both read `09:12`.
pub(crate) fn calendar_time(unix_seconds: u64, offset: UtcOffset) -> String {
    let local = i64::try_from(unix_seconds).unwrap_or(0) + offset.0;
    let days = local.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {}",
        clock_time(unix_seconds, offset)
    )
}

/// Howard Hinnant's `civil_from_days`, which is exact for every date this
/// window can be handed and needs no table.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

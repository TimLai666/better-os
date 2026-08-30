//! Enough of the local timezone to print `Started: 22:18`.
//!
//! The menu needs a wall-clock start time, and the standard library only knows
//! about UTC. Rather than take a dependency or shell out to `date` — which the
//! tray is forbidden to do — this reads the offset out of the system's own
//! `/etc/localtime`, in the version 1 block every TZif file still carries for
//! backward compatibility.
//!
//! If the file is missing or unreadable, the offset is zero and the time shown
//! is UTC. That is a visible, explainable fallback rather than a wrong local
//! time invented from a guess.

use std::path::Path;

/// Seconds to add to a UTC timestamp to get local wall-clock time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UtcOffset(pub i64);

impl UtcOffset {
    pub const UTC: Self = UtcOffset(0);

    /// Reads the offset in force at `unix_seconds` from the system timezone.
    pub fn for_system(unix_seconds: u64) -> Self {
        Self::from_tzif_file(Path::new("/etc/localtime"), unix_seconds).unwrap_or(Self::UTC)
    }

    pub fn from_tzif_file(path: &Path, unix_seconds: u64) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Self::from_tzif(&bytes, unix_seconds)
    }

    /// Parses the version 1 block of a TZif file.
    ///
    /// Version 1 stores 32-bit transition times, which run out in 2038. That is
    /// the right trade here: it is present in every version of the format, and
    /// a menu that prints a start time is never asked about 2039.
    pub fn from_tzif(bytes: &[u8], unix_seconds: u64) -> Option<Self> {
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

        let utc_offset_of = |type_index: usize| -> Option<i64> {
            let at = types_at + type_index * 6;
            let raw = i32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            Some(i64::from(raw))
        };

        let now = i64::try_from(unix_seconds).ok()?;
        let mut chosen = 0usize;
        let mut found = false;
        for index in 0..transition_count {
            let at = transitions_at + index * 4;
            let transition = i64::from(i32::from_be_bytes([
                bytes[at],
                bytes[at + 1],
                bytes[at + 2],
                bytes[at + 3],
            ]));
            if transition <= now {
                chosen = usize::from(bytes[indices_at + index]);
                found = true;
            } else {
                break;
            }
        }
        if !found {
            // Before the first recorded transition, the first type is the one
            // in force, which is what every TZif reader does.
            chosen = 0;
        }
        if chosen >= type_count {
            return None;
        }
        utc_offset_of(chosen).map(UtcOffset)
    }
}

/// `HH:MM` in local time. Pure arithmetic on a timestamp and an offset, so it
/// is provable without a timezone database.
pub fn clock_time(unix_seconds: u64, offset: UtcOffset) -> String {
    let local = i64::try_from(unix_seconds).unwrap_or(0) + offset.0;
    // Rust's `%` keeps the sign of the dividend, and a negative timestamp must
    // still land on a positive time of day.
    let seconds_into_day = local.rem_euclid(86_400);
    let hours = seconds_into_day / 3_600;
    let minutes = (seconds_into_day % 3_600) / 60;
    format!("{hours:02}:{minutes:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midnight_utc_is_midnight_when_there_is_no_offset() {
        assert_eq!(clock_time(0, UtcOffset::UTC), "00:00");
    }

    #[test]
    fn a_positive_offset_moves_the_wall_clock_forward() {
        // 2023-11-14T22:13:20Z, which is 06:13 the next morning in Taipei.
        assert_eq!(clock_time(1_700_000_000, UtcOffset(8 * 3_600)), "06:13");
        assert_eq!(clock_time(1_700_000_000, UtcOffset::UTC), "22:13");
    }

    #[test]
    fn a_negative_offset_can_roll_back_across_midnight() {
        assert_eq!(clock_time(3_600, UtcOffset(-5 * 3_600)), "20:00");
    }

    #[test]
    fn something_that_is_not_a_timezone_file_yields_no_offset_rather_than_a_wrong_one() {
        assert_eq!(UtcOffset::from_tzif(b"not a tzif file at all", 0), None);
        assert_eq!(UtcOffset::from_tzif(&[], 0), None);
    }

    /// A hand-built TZif with one type at +08:00 and no transitions.
    fn fixed_offset_tzif(offset_seconds: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TZif");
        bytes.push(b'2');
        bytes.extend_from_slice(&[0u8; 15]);
        for value in [0u32, 0, 0, 0, 1, 4] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&offset_seconds.to_be_bytes());
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(b"CST\0");
        bytes
    }

    #[test]
    fn a_file_with_a_single_fixed_offset_is_read() {
        let bytes = fixed_offset_tzif(8 * 3_600);
        assert_eq!(
            UtcOffset::from_tzif(&bytes, 1_700_000_000),
            Some(UtcOffset(8 * 3_600))
        );
    }

    #[test]
    fn a_truncated_file_is_refused_rather_than_read_past_its_end() {
        let mut bytes = fixed_offset_tzif(8 * 3_600);
        bytes.truncate(46);
        assert_eq!(UtcOffset::from_tzif(&bytes, 0), None);
    }

    #[test]
    fn the_transition_in_force_is_the_last_one_that_has_already_happened() {
        // Two types: +00:00 and +01:00, with one transition at t = 100.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TZif");
        bytes.push(b'2');
        bytes.extend_from_slice(&[0u8; 15]);
        for value in [0u32, 0, 0, 1, 2, 8] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&100i32.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0i32.to_be_bytes());
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&3_600i32.to_be_bytes());
        bytes.push(1);
        bytes.push(4);
        bytes.extend_from_slice(b"GMT\0BST\0");

        assert_eq!(UtcOffset::from_tzif(&bytes, 99), Some(UtcOffset(0)));
        assert_eq!(UtcOffset::from_tzif(&bytes, 100), Some(UtcOffset(3_600)));
        assert_eq!(UtcOffset::from_tzif(&bytes, 10_000), Some(UtcOffset(3_600)));
    }
}

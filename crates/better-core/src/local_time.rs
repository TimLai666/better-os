//! The one place a Unix timestamp becomes a wall-clock time, and back.
//!
//! Every Better OS window, tray, service, and command line that shows or reads
//! a local time goes through here (ticket 146). The conversion is the C
//! library's `localtime_r`, which already knows `TZ`, `/etc/localtime`, every
//! version of the TZif format, and the daylight-saving rules of the zone; a
//! second reader would be a second, worse timezone database. Each timestamp is
//! converted on its own, so two times on either side of a daylight-saving
//! change each get the offset that was in force at them.
//!
//! The way back, a wall-clock time to an instant, is [`TimeZone::resolve`]. It
//! is built on `localtime_r` alone, not on `mktime`, whose answer for a
//! repeated hour depends on the conversion the process made before it. A wall
//! time a change repeats has two instants and says so; one a change skips has
//! a defined instant just past the gap.
//!
//! # Threads and `TZ`
//!
//! The C library reads `TZ` with `getenv`, which races with a `setenv` on
//! another thread. The invariant every Better OS program keeps is that nothing
//! sets an environment variable after it starts; only tests do, and a test
//! binary that sets `TZ` holds one lock around every conversion it makes and
//! calls [`reload_zone`] after the change.
//!
//! When the system has no timezone data — no `TZ`, no readable
//! `/etc/localtime`, or a `TZ` the library could not load — the C library
//! falls back to UTC. That is reported rather than hidden: the time carries
//! [`LocalTime::utc_fallback`], and the formatted forms end in ` UTC`. No
//! offset is ever guessed.
//!
//! Nothing here runs a command, so the windows and the tray, which may not, can
//! use it.

use std::ffi::CStr;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// POSIX does not require `localtime_r` to consult `TZ`, and glibc's does not
// re-read it once the zone has been initialized, so `tzset` is what makes a
// zone change while a program runs take effect. It is not in the `libc`
// crate's exported surface. It costs far more than the conversion itself (926
// ns against 73 ns, measured in the review of ticket 146, because it checks
// `/etc/localtime`), so it runs at most once per [`ZONE_RECHECK`], never per
// timestamp: a list of a thousand file rows asks once.
unsafe extern "C" {
    fn tzset();
}

/// How long a zone the C library loaded is used before it is asked to look
/// again. A timezone changed while a program runs (Settings, `timedatectl`)
/// shows in that program's next conversion at most this long afterwards.
pub const ZONE_RECHECK: Duration = Duration::from_secs(1);

/// Milliseconds after [`clock_start`] of the last `tzset`, or `u64::MAX` for
/// never.
static ZONE_CHECKED_AT: AtomicU64 = AtomicU64::new(u64::MAX);

fn clock_start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn elapsed_millis() -> u64 {
    u64::try_from(clock_start().elapsed().as_millis()).unwrap_or(u64::MAX - 1)
}

/// Calls `tzset` when the last call is older than [`ZONE_RECHECK`].
fn refresh_zone() {
    let now = elapsed_millis();
    let last = ZONE_CHECKED_AT.load(Ordering::Relaxed);
    let recheck = u64::try_from(ZONE_RECHECK.as_millis()).unwrap_or(u64::MAX);
    if last == u64::MAX || now.saturating_sub(last) >= recheck {
        reload_zone_at(now);
    }
}

/// Makes the next conversion read the zone again, now. A program never needs
/// it; a test that has just changed `TZ` does.
pub fn reload_zone() {
    reload_zone_at(elapsed_millis());
}

fn reload_zone_at(now: u64) {
    // SAFETY: `tzset` takes no arguments, returns nothing, and synchronizes
    // internally.
    unsafe { tzset() };
    ZONE_CHECKED_AT.store(now, Ordering::Relaxed);
}

/// The system's own zone file when `TZ` does not name one.
const SYSTEM_ZONE_FILE: &str = "/etc/localtime";

/// Zone abbreviations a real zone uses at an offset of zero. Anything else at
/// zero is the C library's fallback, which names the zone it failed to load
/// (`TZ=Nowhere/Invented` reads as `Nowhere`) or nothing at all.
const ZERO_OFFSET_NAMES: &[&str] = &[
    "UTC",
    "UCT",
    "GMT",
    "Universal",
    "Zulu",
    "Z",
    "WET",
    "+00",
    "-00",
];

/// Which zone a timestamp is shown in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeZone {
    /// The system's zone as the C library reads it, per timestamp.
    System,
    /// A fixed offset in seconds east of UTC. Tests use it for a time that
    /// does not depend on the machine; [`TimeZone::UTC`] is the zero one.
    Fixed(i32),
}

/// A calendar date and time of day, in no particular zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CivilTime {
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// A timestamp as a wall clock in some zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTime {
    pub civil: CivilTime,
    /// Days since Monday: 0 for Monday, 6 for Sunday (ISO 8601's order).
    pub days_from_monday: u8,
    /// Seconds east of UTC in force at this instant.
    pub offset_seconds: i32,
    /// The system zone could not be read and this is UTC instead.
    pub utc_fallback: bool,
}

impl TimeZone {
    pub const UTC: Self = TimeZone::Fixed(0);

    /// The wall clock at `unix_seconds`. A timestamp the C library cannot
    /// place is given in UTC and marked as such.
    pub fn at(self, unix_seconds: i64) -> LocalTime {
        match self {
            TimeZone::Fixed(offset) => fixed(unix_seconds, offset, false),
            TimeZone::System => {
                system_at(unix_seconds).unwrap_or_else(|| fixed(unix_seconds, 0, true))
            }
        }
    }

    /// Every instant at which this zone's wall clock reads `civil`, which must
    /// be a real calendar date and time of day.
    ///
    /// The answer is found by trying each offset in force within a day of the
    /// time and keeping the instants that read back as `civil`, so it does not
    /// depend on anything converted before it.
    pub fn resolve(self, civil: CivilTime) -> WallTime {
        let as_utc = civil.unix_utc();
        let offset = match self {
            TimeZone::Fixed(offset) => return WallTime::Once(as_utc.saturating_sub(offset.into())),
            TimeZone::System => |at: i64| i64::from(self.at(at).offset_seconds),
        };
        let mut offsets = [
            offset(as_utc.saturating_sub(86_400)),
            offset(as_utc),
            offset(as_utc.saturating_add(86_400)),
        ];
        offsets.sort_unstable();
        let candidates = offsets.map(|offset| as_utc.saturating_sub(offset));
        let mut hits: Vec<i64> = candidates
            .iter()
            .copied()
            .filter(|instant| self.at(*instant).civil == civil)
            .collect();
        hits.sort_unstable();
        hits.dedup();
        match hits.as_slice() {
            [] => WallTime::Skipped {
                // The largest offset gives the earliest candidate and the
                // smallest the latest; the latest reads as the skipped time
                // pushed forward by the gap's length.
                after_gap: candidates.into_iter().max().unwrap_or(as_utc),
            },
            [once] => WallTime::Once(*once),
            [earlier, .., later] => WallTime::Twice {
                earlier: *earlier,
                later: *later,
            },
        }
    }

    /// The earliest instant at which the wall clock reads `civil`, by the
    /// rules of [`resolve`](Self::resolve).
    pub fn unix_at(self, civil: CivilTime) -> i64 {
        self.resolve(civil).earliest()
    }

    /// The first instant after `after` at which the wall clock reads
    /// `hour:minute`: today when it is still to come (in a repeated hour, the
    /// first reading of it still to come), otherwise tomorrow.
    pub fn next_wall_time(self, after: i64, hour: u8, minute: u8) -> NextWallTime {
        let today = CivilTime {
            hour,
            minute,
            second: 0,
            ..self.at(after).civil
        };
        if let Some(unix_seconds) = self.resolve(today).first_after(after) {
            return NextWallTime {
                unix_seconds,
                tomorrow: false,
            };
        }
        NextWallTime {
            unix_seconds: self.resolve(today.add_days(1)).earliest(),
            tomorrow: true,
        }
    }
}

/// The instants at which a wall clock reads one date and time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WallTime {
    /// The usual case: one instant.
    Once(i64),
    /// A daylight-saving change repeats this time: it happens twice.
    Twice { earlier: i64, later: i64 },
    /// A daylight-saving change skips this time. `after_gap` is the instant
    /// it would have been without the change, which the wall clock reads as
    /// that time pushed forward by the gap (02:30 becomes 03:30).
    Skipped { after_gap: i64 },
}

impl WallTime {
    pub fn earliest(self) -> i64 {
        match self {
            WallTime::Once(at) | WallTime::Skipped { after_gap: at } => at,
            WallTime::Twice { earlier, .. } => earlier,
        }
    }

    /// The first of the instants that is later than `after`.
    pub fn first_after(self, after: i64) -> Option<i64> {
        match self {
            WallTime::Once(at) | WallTime::Skipped { after_gap: at } => (at > after).then_some(at),
            WallTime::Twice { earlier, later } => {
                [earlier, later].into_iter().find(|at| *at > after)
            }
        }
    }
}

/// When a wall-clock time next comes round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NextWallTime {
    pub unix_seconds: i64,
    /// It is not still to come today.
    pub tomorrow: bool,
}

impl LocalTime {
    /// `HH:MM`, with ` UTC` after it when the zone could not be read.
    pub fn clock(&self) -> String {
        format!(
            "{:02}:{:02}{}",
            self.civil.hour,
            self.civil.minute,
            self.fallback_suffix()
        )
    }

    /// `YYYY-MM-DD HH:MM`, with ` UTC` after it when the zone could not be
    /// read. Two times a day apart never read the same.
    pub fn calendar(&self) -> String {
        format!(
            "{} {:02}:{:02}{}",
            self.civil.date_text(),
            self.civil.hour,
            self.civil.minute,
            self.fallback_suffix()
        )
    }

    /// Minutes since local midnight.
    pub fn minute_of_day(&self) -> u16 {
        u16::from(self.civil.hour) * 60 + u16::from(self.civil.minute)
    }

    fn fallback_suffix(&self) -> &'static str {
        if self.utc_fallback { " UTC" } else { "" }
    }
}

/// `HH:MM` in `zone`, or `HH:MM UTC` when the system zone is missing.
pub fn clock_time(unix_seconds: u64, zone: TimeZone) -> String {
    zone.at(clamp(unix_seconds)).clock()
}

/// `YYYY-MM-DD HH:MM` in `zone`, or with ` UTC` when the system zone is
/// missing.
pub fn calendar_time(unix_seconds: u64, zone: TimeZone) -> String {
    zone.at(clamp(unix_seconds)).calendar()
}

fn clamp(unix_seconds: u64) -> i64 {
    i64::try_from(unix_seconds).unwrap_or(i64::MAX)
}

impl CivilTime {
    /// The date and time `unix_seconds` names in UTC.
    pub fn from_unix_utc(unix_seconds: i64) -> Self {
        let days = unix_seconds.div_euclid(86_400);
        let of_day = unix_seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        CivilTime {
            year,
            month,
            day,
            hour: (of_day / 3_600) as u8,
            minute: (of_day % 3_600 / 60) as u8,
            second: (of_day % 60) as u8,
        }
    }

    /// The Unix time this date and time names when read as UTC.
    pub fn unix_utc(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour) * 3_600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// The same time of day `days` calendar days later (or earlier).
    pub fn add_days(&self, days: i64) -> Self {
        let (year, month, day) =
            civil_from_days(days_from_civil(self.year, self.month, self.day) + days);
        CivilTime {
            year,
            month,
            day,
            ..*self
        }
    }

    /// `YYYY-MM-DD`.
    pub fn date_text(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `YYYY-MM-DDThh:mm:ss`, RFC 3339's form without a zone, which is what a
    /// freedesktop `.trashinfo` `DeletionDate` holds.
    pub fn iso_text(&self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}",
            self.date_text(),
            self.hour,
            self.minute,
            self.second
        )
    }

    /// Whether this is a real calendar date and time of day.
    pub fn is_valid(&self) -> bool {
        (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour < 24
            && self.minute < 60
            && self.second < 61
    }

    /// Reads [`iso_text`](Self::iso_text)'s form back. Seconds may be left
    /// out; anything out of range is refused rather than normalized.
    pub fn parse_iso(text: &str) -> Option<Self> {
        let (date, time) = text.trim().split_once('T')?;
        let mut date = date.split('-');
        let year: i64 = date.next()?.parse().ok()?;
        let month: u8 = date.next()?.parse().ok()?;
        let day: u8 = date.next()?.parse().ok()?;
        if date.next().is_some() {
            return None;
        }
        let mut time = time.split(':');
        let hour: u8 = time.next()?.parse().ok()?;
        let minute: u8 = time.next()?.parse().ok()?;
        let second: u8 = match time.next() {
            // A fraction of a second is allowed by RFC 3339 and dropped here.
            Some(second) => second.split('.').next()?.parse().ok()?,
            None => 0,
        };
        let civil = CivilTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        };
        civil.is_valid().then_some(civil)
    }
}

fn days_in_month(year: i64, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn fixed(unix_seconds: i64, offset: i32, utc_fallback: bool) -> LocalTime {
    let local = unix_seconds.saturating_add(i64::from(offset));
    let days = local.div_euclid(86_400);
    LocalTime {
        civil: CivilTime::from_unix_utc(local),
        // 1970-01-01 was a Thursday, three days after a Monday.
        days_from_monday: (days + 3).rem_euclid(7) as u8,
        offset_seconds: offset,
        utc_fallback,
    }
}

fn system_at(unix_seconds: i64) -> Option<LocalTime> {
    let time = libc::time_t::try_from(unix_seconds).ok()?;
    // SAFETY: `tm` is a plain C struct for which all-zero is a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    refresh_zone();
    // SAFETY: `localtime_r` writes only into the `tm` this frame owns and
    // reads only the `time_t` passed by reference.
    let result = unsafe { libc::localtime_r(&time, &mut tm) };
    if result.is_null() {
        return None;
    }
    let abbreviation = if tm.tm_zone.is_null() {
        String::new()
    } else {
        // SAFETY: a non-null `tm_zone` points at a NUL-terminated name the C
        // library keeps alive for the life of the process.
        unsafe { CStr::from_ptr(tm.tm_zone) }
            .to_string_lossy()
            .into_owned()
    };
    let offset = i32::try_from(tm.tm_gmtoff).ok()?;
    let tz = std::env::var_os("TZ");
    let utc_fallback = zone_is_missing(
        offset,
        &abbreviation,
        tz.as_ref().map(|value| value.to_string_lossy()).as_deref(),
        || Path::new(SYSTEM_ZONE_FILE).exists(),
    );
    Some(LocalTime {
        civil: CivilTime {
            year: i64::from(tm.tm_year) + 1900,
            month: u8::try_from(tm.tm_mon + 1).ok()?,
            day: u8::try_from(tm.tm_mday).ok()?,
            hour: u8::try_from(tm.tm_hour).ok()?,
            minute: u8::try_from(tm.tm_min).ok()?,
            // A leap second reads as :60 from some zone files.
            second: u8::try_from(tm.tm_sec).ok()?.min(59),
        },
        // `tm_wday` is 0 for Sunday.
        days_from_monday: u8::try_from((tm.tm_wday + 6).rem_euclid(7)).ok()?,
        offset_seconds: offset,
        utc_fallback,
    })
}

/// Whether the C library's answer is its UTC fallback rather than a zone.
///
/// A non-zero offset always came from zone data. At zero, the zone is real
/// only when it has a zero-offset name and there was a zone to read: a `TZ`
/// value, or, with `TZ` unset, the system zone file. An empty `TZ` is glibc's
/// UTC-without-a-zone and counts as missing.
pub(crate) fn zone_is_missing(
    offset_seconds: i32,
    abbreviation: &str,
    tz: Option<&str>,
    system_zone_file_exists: impl FnOnce() -> bool,
) -> bool {
    if offset_seconds != 0 {
        return false;
    }
    let named = ZERO_OFFSET_NAMES.contains(&abbreviation.trim());
    let source = match tz {
        Some(value) => !value.trim_start_matches(':').trim().is_empty(),
        None => system_zone_file_exists(),
    };
    !(named && source)
}

/// Howard Hinnant's `civil_from_days`: exact for every day an `i64` of
/// seconds can name, and needs no table.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u8;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u8;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Howard Hinnant's `days_from_civil`, the inverse of [`civil_from_days`].
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_zone_is_pure_arithmetic() {
        // 2023-11-14T22:13:20Z, which is 06:13 the next morning in Taipei.
        assert_eq!(clock_time(1_700_000_000, TimeZone::UTC), "22:13");
        assert_eq!(
            clock_time(1_700_000_000, TimeZone::Fixed(8 * 3_600)),
            "06:13"
        );
        assert_eq!(
            calendar_time(1_700_000_000, TimeZone::Fixed(8 * 3_600)),
            "2023-11-15 06:13"
        );
        assert_eq!(clock_time(0, TimeZone::UTC), "00:00");
        // A negative offset rolls back across midnight.
        assert_eq!(clock_time(3_600, TimeZone::Fixed(-5 * 3_600)), "20:00");
        assert_eq!(
            calendar_time(3_600, TimeZone::Fixed(-5 * 3_600)),
            "1969-12-31 20:00"
        );
    }

    #[test]
    fn calendar_dates_are_exact_across_leap_days_and_centuries() {
        assert_eq!(
            calendar_time(951_782_400, TimeZone::UTC),
            "2000-02-29 00:00"
        );
        assert_eq!(
            calendar_time(1_794_832_980, TimeZone::UTC),
            "2026-11-16 12:43"
        );
        assert_eq!(
            calendar_time(4_107_542_400, TimeZone::UTC),
            "2100-03-01 00:00"
        );
        for seconds in [-86_401, -1, 0, 951_782_400, 1_709_633_730, 4_107_542_399] {
            let civil = CivilTime::from_unix_utc(seconds);
            assert_eq!(civil.unix_utc(), seconds, "{civil:?}");
        }
    }

    #[test]
    fn the_weekday_is_monday_first() {
        // 2023-11-14 was a Tuesday, and 2023-11-19 a Sunday.
        assert_eq!(TimeZone::UTC.at(1_700_000_000).days_from_monday, 1);
        assert_eq!(TimeZone::UTC.at(1_700_395_200).days_from_monday, 6);
        assert_eq!(TimeZone::UTC.at(0).days_from_monday, 3);
        assert_eq!(TimeZone::UTC.at(-1).days_from_monday, 2);
    }

    #[test]
    fn a_fixed_zone_reads_a_wall_clock_back() {
        let taipei = TimeZone::Fixed(8 * 3_600);
        let civil = taipei.at(1_700_000_000).civil;
        assert_eq!(taipei.unix_at(civil), 1_700_000_000);
        assert_eq!(taipei.unix_at(civil.add_days(1)), 1_700_000_000 + 86_400);
        assert_eq!(taipei.resolve(civil), WallTime::Once(1_700_000_000));
    }

    #[test]
    fn the_next_wall_time_is_today_when_still_to_come_and_tomorrow_otherwise() {
        // 2023-11-14 22:13:20 UTC.
        let now = 1_700_000_000;
        let midnight = now - (22 * 3_600 + 13 * 60 + 20);
        let later = TimeZone::UTC.next_wall_time(now, 23, 0);
        assert_eq!(
            later,
            NextWallTime {
                unix_seconds: midnight + 23 * 3_600,
                tomorrow: false
            }
        );
        let morning = TimeZone::UTC.next_wall_time(now, 7, 30);
        assert!(morning.tomorrow);
        assert_eq!(morning.unix_seconds, midnight + 86_400 + 7 * 3_600 + 1_800);
        // The minute it is now is not still to come.
        assert!(TimeZone::UTC.next_wall_time(now - 20, 22, 13).tomorrow);
    }

    #[test]
    fn the_instants_of_a_wall_time_choose_the_first_still_to_come() {
        let twice = WallTime::Twice {
            earlier: 100,
            later: 3_700,
        };
        assert_eq!(twice.earliest(), 100);
        assert_eq!(twice.first_after(99), Some(100));
        assert_eq!(twice.first_after(100), Some(3_700));
        assert_eq!(twice.first_after(3_700), None);
        let skipped = WallTime::Skipped { after_gap: 500 };
        assert_eq!(skipped.earliest(), 500);
        assert_eq!(skipped.first_after(499), Some(500));
        assert_eq!(WallTime::Once(7).first_after(7), None);
    }

    #[test]
    fn the_trash_stamp_round_trips_and_refuses_what_is_not_a_date() {
        let civil = CivilTime::from_unix_utc(1_709_633_730);
        assert_eq!(civil.iso_text(), "2024-03-05T10:15:30");
        assert_eq!(CivilTime::parse_iso("2024-03-05T10:15:30"), Some(civil));
        assert_eq!(
            CivilTime::parse_iso("2024-03-05T10:15"),
            Some(CivilTime { second: 0, ..civil })
        );
        assert_eq!(CivilTime::parse_iso("2024-03-05T10:15:30.25"), Some(civil));
        for bad in [
            "",
            "2024-03-05",
            "2024-13-05T10:15:30",
            "2023-02-29T10:15:30",
            "2024-03-05T24:00:00",
            "2024-03-05-01T10:15:30",
            "yesterday",
        ] {
            assert_eq!(CivilTime::parse_iso(bad), None, "{bad}");
        }
        assert!(CivilTime::parse_iso("2024-02-29T00:00:00").is_some());
    }

    #[test]
    fn a_missing_zone_is_told_apart_from_a_real_zero_offset_zone() {
        let file = || true;
        let no_file = || false;
        // Real zones at zero: the machine set to UTC, London in winter, the
        // Azores in summer, and a TZ naming UTC.
        assert!(!zone_is_missing(0, "UTC", None, file));
        assert!(!zone_is_missing(0, "GMT", None, file));
        assert!(!zone_is_missing(0, "+00", None, file));
        assert!(!zone_is_missing(0, "UTC", Some("UTC0"), no_file));
        assert!(!zone_is_missing(0, "GMT", Some(":Europe/London"), no_file));
        // Any offset is zone data.
        assert!(!zone_is_missing(28_800, "CST", None, no_file));
        // No TZ and no /etc/localtime: glibc's UTC is a fallback.
        assert!(zone_is_missing(0, "UTC", None, no_file));
        // A TZ glibc could not load, or an empty one.
        assert!(zone_is_missing(
            0,
            "Nowhere",
            Some("Nowhere/Invented"),
            file
        ));
        assert!(zone_is_missing(0, "", Some(":/nonexistent"), file));
        assert!(zone_is_missing(0, "Universal", Some(""), file));
    }

    #[test]
    fn a_fallback_says_utc_and_a_fixed_zone_does_not() {
        let fallback = fixed(1_700_000_000, 0, true);
        assert_eq!(fallback.clock(), "22:13 UTC");
        assert_eq!(fallback.calendar(), "2023-11-14 22:13 UTC");
        assert_eq!(TimeZone::UTC.at(1_700_000_000).clock(), "22:13");
        assert!(!TimeZone::UTC.at(1_700_000_000).utc_fallback);
    }
}

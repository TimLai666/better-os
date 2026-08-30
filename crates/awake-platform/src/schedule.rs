//! The local wall clock, for time-schedule conditions.
//!
//! `awake-core` refuses to know about timezones, so something has to turn a Unix
//! timestamp into "Tuesday, 14:30 local". That is this file. It calls
//! `localtime_r`, which is the libc function that already knows about `TZ`,
//! `/etc/localtime`, and the daylight-saving rules for the zone — reimplementing
//! any of that would be a second, worse timezone database.
//!
//! This provider does no I/O of its own and can never be unavailable, which is
//! why its cadence is free: a schedule condition costs nothing to answer no
//! matter how many rules use one.

use awake_core::{LocalTime, Observations, ProviderKind, Weekday};

use crate::provider::{Cadence, TriggerProvider};

// POSIX does not require `localtime_r` to consult `TZ`, and glibc's does not
// re-read it once the zone has been initialized. A user who changes their
// timezone while the service is running would otherwise keep getting schedules
// evaluated against the old one until the next restart, so `tzset` is called
// first. It is not in the `libc` crate's exported surface, and it takes a lock
// internally, so calling it on every conversion is safe and cheap.
unsafe extern "C" {
    fn tzset();
}

/// Converts a Unix timestamp to the local weekday and minute of the day.
///
/// Returns `None` only when libc refuses the conversion, which happens for a
/// timestamp outside the range the platform's `time_t` can express.
pub fn local_time(unix_seconds: u64) -> Option<LocalTime> {
    let timestamp = i64::try_from(unix_seconds).ok()?;
    let mut parts = unsafe { std::mem::zeroed::<libc::tm>() };
    // SAFETY: `tzset` takes no arguments, returns nothing, and synchronizes
    // internally.
    unsafe { tzset() };
    // SAFETY: `localtime_r` writes into the `tm` we own and reads only the
    // `time_t` we pass by reference. It is the reentrant form precisely so it
    // does not hand back a pointer into shared state.
    let result =
        unsafe { libc::localtime_r(&timestamp as *const i64 as *const libc::time_t, &mut parts) };
    if result.is_null() {
        return None;
    }
    // `tm_wday` is 0 for Sunday; `Weekday` is Monday-first to match ISO 8601.
    let weekday = match parts.tm_wday {
        0 => Weekday::Sunday,
        1 => Weekday::Monday,
        2 => Weekday::Tuesday,
        3 => Weekday::Wednesday,
        4 => Weekday::Thursday,
        5 => Weekday::Friday,
        6 => Weekday::Saturday,
        _ => return None,
    };
    let minute_of_day = u16::try_from(parts.tm_hour.max(0) * 60 + parts.tm_min.max(0)).ok()?;
    Some(LocalTime {
        weekday,
        minute_of_day,
    })
}

/// Answers time-schedule conditions.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScheduleProvider;

impl TriggerProvider for ScheduleProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::TimeSchedule
    }

    fn cadence(&self) -> Cadence {
        Cadence::Free
    }

    fn sample(&mut self, now_unix_seconds: u64, into: &mut Observations) {
        match local_time(now_unix_seconds) {
            Some(local) => {
                into.local_time = Some(local);
                into.mark_available(ProviderKind::TimeSchedule);
            }
            None => into.mark_unavailable(
                ProviderKind::TimeSchedule,
                "awake.provider.clock_out_of_range",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TZ` is process-global, and the test harness runs tests on many threads,
    /// so two tests each setting it would read each other's zone. This lock is
    /// what makes each of them see the zone it asked for.
    static TIMEZONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Runs a closure with `TZ` set, so the conversion is checked against a zone
    /// the test names rather than against whatever the machine happens to be in.
    fn with_timezone<T>(zone: &str, body: impl FnOnce() -> T) -> T {
        // A poisoned lock here means another timezone test panicked; the zone is
        // restored either way, so continuing is correct.
        let _guard = TIMEZONE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: the lock above makes this the only thread touching `TZ`, and
        // `tzset` is called immediately so libc picks the new value up rather
        // than a cached one.
        let previous = std::env::var("TZ").ok();
        unsafe {
            std::env::set_var("TZ", zone);
            tzset();
        }
        let result = body();
        unsafe {
            match previous {
                Some(previous) => std::env::set_var("TZ", previous),
                None => std::env::remove_var("TZ"),
            }
            tzset();
        }
        result
    }

    #[test]
    fn a_timestamp_becomes_the_local_weekday_and_minute() {
        // 2023-11-14T22:13:20Z is a Tuesday.
        with_timezone("UTC", || {
            assert_eq!(
                local_time(1_700_000_000),
                Some(LocalTime {
                    weekday: Weekday::Tuesday,
                    minute_of_day: 22 * 60 + 13,
                })
            );
        });
    }

    #[test]
    fn the_local_zone_is_what_a_schedule_is_measured_in_not_utc() {
        // The same instant is Wednesday morning in Taipei, which is the whole
        // reason a schedule cannot be evaluated against a Unix timestamp.
        with_timezone("Asia/Taipei", || {
            assert_eq!(
                local_time(1_700_000_000),
                Some(LocalTime {
                    weekday: Weekday::Wednesday,
                    minute_of_day: 6 * 60 + 13,
                })
            );
        });
    }

    #[test]
    fn sunday_is_the_last_day_of_the_week_and_not_the_first() {
        // 2023-11-19T12:00:00Z is a Sunday.
        with_timezone("UTC", || {
            assert_eq!(local_time(1_700_395_200).unwrap().weekday, Weekday::Sunday);
        });
        assert_eq!(Weekday::Sunday.index(), 6);
        assert_eq!(Weekday::Monday.previous(), Weekday::Sunday);
    }

    #[test]
    fn the_schedule_provider_costs_nothing_when_idle() {
        assert_eq!(ScheduleProvider.cadence(), Cadence::Free);
    }

    #[test]
    fn sampling_records_a_time_and_marks_the_provider_available() {
        let mut observations = Observations::at(1_700_000_000);
        ScheduleProvider.sample(1_700_000_000, &mut observations);
        assert!(observations.local_time.is_some());
        assert!(
            observations
                .availability_of(ProviderKind::TimeSchedule)
                .is_available()
        );
    }

    #[test]
    fn a_timestamp_beyond_what_the_platform_can_express_is_refused_not_wrapped() {
        assert_eq!(local_time(u64::MAX), None);
    }
}

//! The local wall clock, for time-schedule conditions.
//!
//! `awake-core` refuses to know about timezones, so something has to turn a Unix
//! timestamp into "Tuesday, 14:30 local". That is `better_core::local_time`,
//! the one conversion every Better OS program shares (ticket 146); this file
//! only reshapes its answer into the evaluator's vocabulary.
//!
//! This provider does no I/O of its own, which is why its cadence is free: a
//! schedule condition costs nothing to answer no matter how many rules use
//! one. It is unavailable when the machine has no timezone data: the shared
//! conversion then reads UTC, and a rule's "09:00" measured in UTC is a guess
//! at what the person meant, so the provider says why it cannot answer
//! (`awake.provider.no_time_zone`) instead.

use awake_core::{LocalTime, Observations, ProviderKind, Weekday};
use better_core::local_time::TimeZone;

/// Why the schedule provider cannot answer: no timezone data.
pub const NO_TIME_ZONE: &str = "awake.provider.no_time_zone";
/// Why it cannot answer: a timestamp no signed second count holds.
pub const CLOCK_OUT_OF_RANGE: &str = "awake.provider.clock_out_of_range";

use crate::provider::{Cadence, TriggerProvider};

/// Converts a Unix timestamp to the local weekday and minute of the day.
///
/// `None` when the local time is not known: a timestamp outside what a signed
/// 64-bit second count can hold, or a machine with no timezone data.
pub fn local_time(unix_seconds: u64) -> Option<LocalTime> {
    read(unix_seconds).ok()
}

/// The local time, or the key saying why it is not known.
fn read(unix_seconds: u64) -> Result<LocalTime, &'static str> {
    let instant = i64::try_from(unix_seconds).map_err(|_| CLOCK_OUT_OF_RANGE)?;
    let local = TimeZone::System.at(instant);
    if local.utc_fallback {
        return Err(NO_TIME_ZONE);
    }
    let weekday = match local.days_from_monday {
        0 => Weekday::Monday,
        1 => Weekday::Tuesday,
        2 => Weekday::Wednesday,
        3 => Weekday::Thursday,
        4 => Weekday::Friday,
        5 => Weekday::Saturday,
        _ => Weekday::Sunday,
    };
    Ok(LocalTime {
        weekday,
        minute_of_day: local.minute_of_day(),
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
        match read(now_unix_seconds) {
            Ok(local) => {
                into.local_time = Some(local);
                into.mark_available(ProviderKind::TimeSchedule);
            }
            Err(reason) => {
                into.local_time = None;
                into.mark_unavailable(ProviderKind::TimeSchedule, reason);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TZ` is process-global, and the test harness runs tests on many threads,
    /// so two tests each setting it would read each other's zone, and a
    /// conversion on one thread would race the `setenv` on another. Every test
    /// in this binary that converts a time holds this lock (`with_timezone`).
    static TIMEZONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Runs a closure with `TZ` set, so the conversion is checked against a zone
    /// the test names rather than against whatever the machine happens to be in.
    fn with_timezone<T>(zone: &str, body: impl FnOnce() -> T) -> T {
        // A poisoned lock here means another timezone test panicked; the zone is
        // restored either way, so continuing is correct.
        let _guard = TIMEZONE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: the lock above makes this the only thread touching `TZ` or
        // converting a time; `reload_zone` makes the new value the one the
        // next conversion reads.
        let previous = std::env::var("TZ").ok();
        unsafe {
            std::env::set_var("TZ", zone);
        }
        better_core::local_time::reload_zone();
        let result = body();
        unsafe {
            match previous {
                Some(previous) => std::env::set_var("TZ", previous),
                None => std::env::remove_var("TZ"),
            }
        }
        better_core::local_time::reload_zone();
        result
    }

    #[test]
    fn a_timestamp_becomes_the_local_weekday_and_minute() {
        // 2023-11-14T22:13:20Z is a Tuesday.
        with_timezone("UTC0", || {
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
        with_timezone("UTC0", || {
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
        let observations = with_timezone("CST-8", || {
            let mut observations = Observations::at(1_700_000_000);
            ScheduleProvider.sample(1_700_000_000, &mut observations);
            observations
        });
        assert!(observations.local_time.is_some());
        assert!(
            observations
                .availability_of(ProviderKind::TimeSchedule)
                .is_available()
        );
    }

    /// Ticket 146: with no timezone data the shared conversion reads UTC, and
    /// a schedule rule measured in UTC is a guess, so the provider says it
    /// cannot answer, with the reason the rule editor shows.
    #[test]
    fn with_no_time_zone_the_schedule_says_so_rather_than_running_on_utc() {
        for zone in [":/nonexistent/better-os/zone", "Nowhere/Invented"] {
            let observations = with_timezone(zone, || {
                let mut observations = Observations::at(1_700_000_000);
                ScheduleProvider.sample(1_700_000_000, &mut observations);
                observations
            });
            assert_eq!(observations.local_time, None, "TZ={zone}");
            assert_eq!(
                observations
                    .availability_of(ProviderKind::TimeSchedule)
                    .explanation(),
                Some(NO_TIME_ZONE),
                "TZ={zone}"
            );
            assert_eq!(with_timezone(zone, || local_time(1_700_000_000)), None);
        }
    }

    #[test]
    fn a_timestamp_beyond_a_signed_second_count_is_refused_not_wrapped() {
        assert_eq!(local_time(u64::MAX), None);
    }
}

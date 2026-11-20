//! The system-zone path of `better_core::local_time`, against zones the tests
//! name.
//!
//! `TZ` is process-global and the harness runs tests on many threads, so every
//! test here takes one lock before it sets `TZ`. This file is its own test
//! binary, and nothing else in it reads the environment. The zones are POSIX
//! rule strings, which the C library reads without any zone file, so the
//! daylight-saving cases hold on a machine with no tzdata at all.

use std::sync::Mutex;

use better_core::local_time::{
    CivilTime, NextWallTime, TimeZone, WallTime, calendar_time, clock_time, reload_zone,
};

static TIMEZONE: Mutex<()> = Mutex::new(());

/// New York's rules: EST, EDT from the second Sunday of March to the first
/// Sunday of November.
const NEW_YORK: &str = "EST5EDT,M3.2.0,M11.1.0";
/// Sydney's: AEDT from the first Sunday of October to the first Sunday of
/// April, at 03:00 local.
const SYDNEY: &str = "AEST-10AEDT,M10.1.0,M4.1.0/3";

fn with_timezone<T>(zone: Option<&str>, body: impl FnOnce() -> T) -> T {
    let _guard = TIMEZONE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var("TZ").ok();
    // SAFETY: the lock makes this the only thread in this binary touching the
    // environment or converting a time; `reload_zone` makes the change the
    // one the next conversion reads.
    unsafe {
        match zone {
            Some(zone) => std::env::set_var("TZ", zone),
            None => std::env::remove_var("TZ"),
        }
    }
    reload_zone();
    let result = body();
    unsafe {
        match previous {
            Some(previous) => std::env::set_var("TZ", previous),
            None => std::env::remove_var("TZ"),
        }
    }
    reload_zone();
    result
}

#[test]
fn spring_forward_skips_an_hour_on_the_wall_clock() {
    with_timezone(Some(NEW_YORK), || {
        // 2024-03-10T06:59:59Z is 01:59:59 EST; one second later is 03:00 EDT.
        let before = TimeZone::System.at(1_710_053_999);
        let after = TimeZone::System.at(1_710_054_000);
        assert_eq!(before.clock(), "01:59");
        assert_eq!(before.offset_seconds, -5 * 3_600);
        assert_eq!(after.clock(), "03:00");
        assert_eq!(after.offset_seconds, -4 * 3_600);
        assert!(!before.utc_fallback && !after.utc_fallback);
        assert_eq!(
            calendar_time(1_710_054_000, TimeZone::System),
            "2024-03-10 03:00"
        );
    });
}

#[test]
fn fall_back_repeats_an_hour_on_the_wall_clock() {
    with_timezone(Some(NEW_YORK), || {
        // 2024-11-03T05:59:59Z is 01:59:59 EDT; one second later is 01:00 EST.
        assert_eq!(clock_time(1_730_613_599, TimeZone::System), "01:59");
        assert_eq!(clock_time(1_730_613_600, TimeZone::System), "01:00");
        assert_eq!(
            TimeZone::System.at(1_730_613_600).offset_seconds,
            -5 * 3_600
        );
    });
}

#[test]
fn each_timestamp_gets_the_offset_in_force_at_it_not_the_one_in_force_now() {
    with_timezone(Some(NEW_YORK), || {
        // Two history rows a day apart across the March change: an offset
        // taken once at "now" would show one of them an hour wrong.
        assert_eq!(
            calendar_time(1_709_989_200, TimeZone::System),
            "2024-03-09 08:00"
        );
        assert_eq!(
            calendar_time(1_710_072_000, TimeZone::System),
            "2024-03-10 08:00"
        );
    });
}

#[test]
fn a_wall_clock_time_reads_back_to_the_instant_on_both_sides_of_a_change() {
    with_timezone(Some(NEW_YORK), || {
        let eight = |day| CivilTime {
            year: 2024,
            month: 3,
            day,
            hour: 8,
            minute: 0,
            second: 0,
        };
        // 08:00 the day before the change is 13:00Z; the day of it, 12:00Z.
        assert_eq!(TimeZone::System.unix_at(eight(9)), 1_709_989_200);
        assert_eq!(TimeZone::System.unix_at(eight(10)), 1_710_072_000);
        assert_eq!(
            TimeZone::System.unix_at(eight(9).add_days(1)),
            1_710_072_000
        );
    });
}

fn civil(month: u8, day: u8, hour: u8, minute: u8) -> CivilTime {
    CivilTime {
        year: 2024,
        month,
        day,
        hour,
        minute,
        second: 0,
    }
}

#[test]
fn a_time_the_change_skips_resolves_to_just_past_the_gap() {
    with_timezone(Some(NEW_YORK), || {
        // 02:30 on 2024-03-10 never happens in New York; without the change it
        // would have been 07:30Z, which the clock reads as 03:30 EDT.
        let resolved = TimeZone::System.resolve(civil(3, 10, 2, 30));
        assert_eq!(
            resolved,
            WallTime::Skipped {
                after_gap: 1_710_055_800
            }
        );
        assert_eq!(clock_time(1_710_055_800, TimeZone::System), "03:30");
        assert_eq!(
            TimeZone::System.resolve(civil(3, 10, 3, 30)),
            WallTime::Once(1_710_055_800)
        );
    });
}

#[test]
fn a_time_the_change_repeats_has_both_instants_whatever_was_converted_before() {
    with_timezone(Some(NEW_YORK), || {
        let repeated = WallTime::Twice {
            earlier: 1_730_611_800,
            later: 1_730_615_400,
        };
        // The C library's own reverse conversion answers this from the
        // previous conversion's daylight-saving state; this does not.
        let _summer = TimeZone::System.at(1_720_000_000);
        assert_eq!(TimeZone::System.resolve(civil(11, 3, 1, 30)), repeated);
        let _winter = TimeZone::System.at(1_735_000_000);
        assert_eq!(TimeZone::System.resolve(civil(11, 3, 1, 30)), repeated);
    });
}

#[test]
fn until_a_time_across_spring_forward_is_on_the_wall_clock() {
    with_timezone(Some(NEW_YORK), || {
        // 22:00 EST on 2024-03-09: 08:00 is tomorrow, after the change, so it
        // is 12:00Z and not the 13:00Z a day of seconds would give.
        assert_eq!(
            TimeZone::System.next_wall_time(1_710_039_600, 8, 0),
            NextWallTime {
                unix_seconds: 1_710_072_000,
                tomorrow: true
            }
        );
        // 01:00 EST on the day: 02:30 is skipped, so it ends at 03:30 EDT.
        assert_eq!(
            TimeZone::System.next_wall_time(1_710_050_400, 2, 30),
            NextWallTime {
                unix_seconds: 1_710_055_800,
                tomorrow: false
            }
        );
    });
}

#[test]
fn until_a_time_across_fall_back_takes_the_first_reading_still_to_come() {
    with_timezone(Some(NEW_YORK), || {
        // 01:30 EDT: 01:45 is fifteen minutes away, not a day and a half.
        assert_eq!(
            TimeZone::System.next_wall_time(1_730_611_800, 1, 45),
            NextWallTime {
                unix_seconds: 1_730_612_700,
                tomorrow: false
            }
        );
        // 01:50 EDT: 01:45 EDT has passed, 01:45 EST is 55 minutes away.
        assert_eq!(
            TimeZone::System.next_wall_time(1_730_613_000, 1, 45),
            NextWallTime {
                unix_seconds: 1_730_616_300,
                tomorrow: false
            }
        );
        // 01:50 EST: both have passed, so it is tomorrow's 01:45 EST.
        assert_eq!(
            TimeZone::System.next_wall_time(1_730_616_600, 1, 45),
            NextWallTime {
                unix_seconds: 1_730_702_700,
                tomorrow: true
            }
        );
    });
}

#[test]
fn a_zone_change_while_running_shows_after_a_reload() {
    with_timezone(Some("UTC0"), || {
        assert_eq!(clock_time(1_700_000_000, TimeZone::System), "22:13");
    });
    with_timezone(Some("CST-8"), || {
        assert_eq!(clock_time(1_700_000_000, TimeZone::System), "06:13");
    });
}

#[test]
fn the_southern_hemisphere_changes_the_other_way() {
    with_timezone(Some(SYDNEY), || {
        // 2024-04-06T15:59:59Z is 02:59:59 AEDT; then 02:00 AEST.
        assert_eq!(clock_time(1_712_419_199, TimeZone::System), "02:59");
        assert_eq!(clock_time(1_712_419_200, TimeZone::System), "02:00");
        assert_eq!(
            TimeZone::System.at(1_712_419_200).offset_seconds,
            10 * 3_600
        );
    });
}

#[test]
fn a_zone_that_cannot_be_loaded_is_utc_and_says_so() {
    for zone in [":/nonexistent/better-os/zone", "Nowhere/Invented", ""] {
        with_timezone(Some(zone), || {
            let time = TimeZone::System.at(1_700_000_000);
            assert!(time.utc_fallback, "TZ={zone:?}");
            assert_eq!(time.offset_seconds, 0, "TZ={zone:?}");
            assert_eq!(clock_time(1_700_000_000, TimeZone::System), "22:13 UTC");
            assert_eq!(
                calendar_time(1_700_000_000, TimeZone::System),
                "2023-11-14 22:13 UTC"
            );
        });
    }
}

#[test]
fn a_zone_that_is_utc_on_purpose_is_not_called_a_fallback() {
    for zone in ["UTC0", "GMT0"] {
        with_timezone(Some(zone), || {
            let time = TimeZone::System.at(1_700_000_000);
            assert!(!time.utc_fallback, "TZ={zone:?}");
            assert_eq!(time.clock(), "22:13");
        });
    }
}

#[test]
fn the_weekday_follows_the_local_date() {
    with_timezone(Some("CST-8"), || {
        // Tuesday 22:13 UTC is Wednesday 06:13 at UTC+8.
        let time = TimeZone::System.at(1_700_000_000);
        assert_eq!(time.days_from_monday, 2);
        assert_eq!(time.minute_of_day(), 6 * 60 + 13);
    });
}

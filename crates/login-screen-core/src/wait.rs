//! The wait after failed passwords (design/login-screen-spec.md section 3.9).
//!
//! PAM's own delay (3 s from `pam_faildelay` on Ubuntu) runs inside the check
//! itself, while "Checking…" shows. From the second failure in a row the
//! screen adds Luma's doubling wait before the field takes another password:
//! 1, 2, 4, 8, 16, then 30 seconds every time after.

use std::time::Duration;

/// The longest wait, reached at the seventh failure in a row and kept after.
pub const LONGEST_WAIT: Duration = Duration::from_secs(30);

/// How long the field stays disabled after `consecutive_failures` failures in
/// a row (counting the one that just happened). No wait after the first.
pub fn wait_after_failures(consecutive_failures: u32) -> Duration {
    match consecutive_failures {
        0 | 1 => Duration::ZERO,
        // 2 → 1 s, 3 → 2 s, … 6 → 16 s.
        2..=6 => Duration::from_secs(1 << (consecutive_failures - 2)),
        _ => LONGEST_WAIT,
    }
}

/// The whole seconds the countdown shows while `remaining` is left: a wait
/// with 0.2 s left still says 1.
pub fn seconds_shown(remaining: Duration) -> u64 {
    let millis = remaining.as_millis();
    u64::try_from(millis.div_ceil(1000)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_doubling_wait_starts_at_the_second_failure() {
        let waits: Vec<u64> = (0..=9)
            .map(|failures| wait_after_failures(failures).as_secs())
            .collect();
        assert_eq!(waits, [0, 0, 1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(wait_after_failures(u32::MAX), LONGEST_WAIT);
    }

    #[test]
    fn the_countdown_rounds_up() {
        assert_eq!(seconds_shown(Duration::from_millis(200)), 1);
        assert_eq!(seconds_shown(Duration::from_millis(1000)), 1);
        assert_eq!(seconds_shown(Duration::from_millis(1001)), 2);
        assert_eq!(seconds_shown(Duration::ZERO), 0);
    }
}

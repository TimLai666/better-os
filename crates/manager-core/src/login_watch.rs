//! What Better Manager's window and command line say about the login screen
//! watch (ADR 0033, ticket 140), read from its records with no privilege.
//!
//! The records are the privileged service's ([`manager_ipc::boot_watch`]);
//! this module decides which of them a person is told about, so Home, Health,
//! and `better-manager-cli doctor` never disagree.

pub use manager_ipc::boot_watch::{
    Event, Evidence, LastBoot, LastBootOutcome, Records, RestartOutcome, Setting,
};

/// How long after a late login screen Home keeps saying so. Health and the
/// command line list every kept record.
pub const HOME_DAYS: u64 = 7;

/// Everything read from the records directory at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub setting: Setting,
    pub last_boot: Option<LastBoot>,
    /// Newest first.
    pub events: Vec<Event>,
}

impl Report {
    pub fn read(records: &Records) -> Self {
        Self {
            setting: records.setting(),
            last_boot: records.last_boot(),
            events: records.events(),
        }
    }

    /// The event Home tells the person about: the newest, while it is less
    /// than [`HOME_DAYS`] old. A record from the future (a clock that was
    /// wrong at boot) counts as recent rather than being hidden.
    pub fn recent(&self, now_unix_seconds: u64) -> Option<&Event> {
        self.events.first().filter(|event| {
            now_unix_seconds.saturating_sub(event.detected_at_unix_seconds) < HOME_DAYS * 86_400
        })
    }

    /// Whether the newest event ended with the login screen back.
    pub fn severity(event: &Event) -> Severity {
        match event.restart {
            RestartOutcome::Recovered { .. }
            | RestartOutcome::AppearedBeforeRestart
            | RestartOutcome::PersonAppeared => Severity::Warning,
            RestartOutcome::Pending
            | RestartOutcome::StillMissing { .. }
            | RestartOutcome::RestartFailed { .. } => Severity::Error,
        }
    }
}

/// How much an event needs the person.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    /// It happened and the login screen came back.
    Warning,
    /// The restart did not bring it back, or its end was never recorded.
    Error,
}

/// A record's journal sections, each with its own heading, for a details
/// view or a copy. Empty sections are left out.
pub fn evidence_sections(evidence: &Evidence) -> Vec<(&'static str, &str)> {
    [
        (
            "display manager journal",
            evidence.display_manager_journal.as_str(),
        ),
        ("systemd-logind journal", evidence.logind_journal.as_str()),
        ("kernel ucsi lines", evidence.kernel_ucsi.as_str()),
        ("failed units", evidence.failed_units.as_str()),
    ]
    .into_iter()
    .filter(|(_, text)| !text.trim().is_empty())
    .collect()
}

/// `YYYY-MM-DD HH:MM UTC`, for the command line, which has no time zone
/// reader of its own and says which zone it prints.
pub fn utc_time(unix_seconds: u64) -> String {
    let seconds = i64::try_from(unix_seconds).unwrap_or(i64::MAX);
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        of_day / 3_600,
        (of_day % 3_600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_ipc::boot_watch::RECORD_VERSION;

    fn event(at: u64, restart: RestartOutcome) -> Event {
        Event {
            version: RECORD_VERSION,
            boot_id: "0123456789abcdef0123456789abcdef".to_string(),
            detected_at_unix_seconds: at,
            display_manager: "gdm.service".to_string(),
            waited_seconds: 30,
            evidence: Evidence::default(),
            restart,
        }
    }

    #[test]
    fn home_tells_of_the_newest_event_for_a_week() {
        let report = Report {
            setting: Setting::OnByDefault,
            last_boot: None,
            events: vec![
                event(1_000_000, RestartOutcome::Recovered { after_seconds: 5 }),
                event(500_000, RestartOutcome::StillMissing { waited_seconds: 30 }),
            ],
        };
        assert_eq!(
            report
                .recent(1_000_000 + 86_400)
                .map(|e| e.detected_at_unix_seconds),
            Some(1_000_000)
        );
        assert!(report.recent(1_000_000 + 7 * 86_400).is_none());
        assert!(report.recent(10).is_some());
        let empty = Report {
            events: Vec::new(),
            ..report
        };
        assert!(empty.recent(0).is_none());
    }

    #[test]
    fn a_login_screen_that_came_back_is_a_warning_and_one_that_did_not_an_error() {
        let back = event(0, RestartOutcome::Recovered { after_seconds: 5 });
        assert_eq!(Report::severity(&back), Severity::Warning);
        for outcome in [
            RestartOutcome::Pending,
            RestartOutcome::StillMissing { waited_seconds: 30 },
            RestartOutcome::RestartFailed {
                details: "x".to_string(),
            },
        ] {
            assert_eq!(Report::severity(&event(0, outcome)), Severity::Error);
        }
    }

    #[test]
    fn the_command_line_time_is_utc_and_says_so() {
        // 2026-11-16 12:43:00 UTC, the owner's stuck boot at 20:43 in Taipei.
        assert_eq!(utc_time(1_794_832_980), "2026-11-16 12:43 UTC");
        assert_eq!(utc_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_time(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn evidence_sections_leave_out_what_is_empty() {
        let evidence = Evidence {
            display_manager_journal: "gdm: started\n".to_string(),
            failed_units: "  \n".to_string(),
            ..Evidence::default()
        };
        assert_eq!(
            evidence_sections(&evidence),
            [("display manager journal", "gdm: started\n")]
        );
    }
}

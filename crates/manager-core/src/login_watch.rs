//! What Better Manager's window and command line say about the login screen
//! watch (ADR 0033, ticket 140), read from its records with no privilege.
//!
//! The records are the privileged service's ([`manager_ipc::boot_watch`]);
//! this module decides which of them a person is told about, so Home, Health,
//! and `better-manager-cli doctor` never disagree.

pub use manager_ipc::boot_watch::{
    Event, Evidence, LOGIN_SUPERVISOR_UNIT, LastBoot, LastBootOutcome, LoginStage, Records,
    RestartOutcome, Setting, current_boot_id, interrupted,
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
    /// False when this reader may not open the evidence: the events then
    /// carry no journal excerpts, and the details need an administrator.
    pub evidence_readable: bool,
    /// This boot's id, to tell a record cut off on an earlier boot.
    pub current_boot: Option<String>,
}

impl Report {
    pub fn read(records: &Records) -> Self {
        let events = records.read_events();
        Self {
            setting: records.setting(),
            last_boot: records.last_boot(),
            events: events.events,
            evidence_readable: events.evidence_readable,
            current_boot: current_boot_id(),
        }
    }

    /// Whether `event` asked for a restart on an earlier boot and never wrote
    /// how it ended: the watch was cut off.
    pub fn interrupted(&self, event: &Event) -> bool {
        interrupted(event, self.current_boot.as_deref())
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
            | RestartOutcome::PersonAppeared
            | RestartOutcome::LoginInProgress
            | RestartOutcome::LoginScreenRestarted { .. }
            // A newer version's outcome: something happened, its weight unknown.
            | RestartOutcome::Unknown => Severity::Warning,
            RestartOutcome::Pending
            | RestartOutcome::StillMissing { .. }
            | RestartOutcome::RestartFailed { .. }
            | RestartOutcome::TextLoginUsed { .. } => Severity::Error,
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
            evidence_readable: true,
            current_boot: None,
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
    fn a_restarted_login_screen_is_a_warning_and_the_text_login_an_error() {
        use manager_ipc::boot_watch::LoginStage;
        let restarted = event(
            0,
            RestartOutcome::LoginScreenRestarted {
                stage: LoginStage::DisplayServer,
                restarts: 1,
                after_seconds: 30,
            },
        );
        assert_eq!(Report::severity(&restarted), Severity::Warning);
        let text = event(
            0,
            RestartOutcome::TextLoginUsed {
                stage: LoginStage::LoginManager,
                tty: "tty1".to_string(),
                after_seconds: 20,
            },
        );
        assert_eq!(Report::severity(&text), Severity::Error);
    }

    #[test]
    fn a_report_calls_an_earlier_boots_pending_record_interrupted() {
        let mut pending = event(0, RestartOutcome::Pending);
        let report = Report {
            setting: Setting::On,
            last_boot: None,
            events: Vec::new(),
            evidence_readable: true,
            current_boot: Some("f".repeat(32)),
        };
        assert!(report.interrupted(&pending));
        pending.boot_id = "f".repeat(32);
        assert!(!report.interrupted(&pending));
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

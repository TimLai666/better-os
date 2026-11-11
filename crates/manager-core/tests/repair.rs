//! Repairing an interrupted dpkg run through `manager-core` (ticket 68, ADR
//! 0014). The privileged service is a fake: what is tested is what the shared
//! API asks it, what it reports back, and what it writes into the record.

use std::sync::Mutex;

use manager_core::repair::{RepairResult, package_system_state, repair_package_system};
use manager_core::{ActivityKind, ManagerState};
use manager_ipc::{
    DpkgPackageStatus, InterruptedPackage, LogEntry, PROTOCOL_VERSION, PackageSystemState,
    RepairOutcome, RepairStatus,
};
use manager_platform::{PackageSystemRepair, PlatformError};

/// Answers from a script and records which of the two methods were called.
struct FakeService {
    state: Result<PackageSystemState, fn() -> PlatformError>,
    repair: Result<RepairOutcome, fn() -> PlatformError>,
    calls: Mutex<Vec<&'static str>>,
}

impl FakeService {
    fn new(
        state: Result<PackageSystemState, fn() -> PlatformError>,
        repair: Result<RepairOutcome, fn() -> PlatformError>,
    ) -> Self {
        Self {
            state,
            repair,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}

impl PackageSystemRepair for FakeService {
    fn package_system_state(&self) -> Result<PackageSystemState, PlatformError> {
        self.calls.lock().unwrap().push("state");
        self.state.clone().map_err(|error| error())
    }

    fn repair_package_system(&self) -> Result<RepairOutcome, PlatformError> {
        self.calls.lock().unwrap().push("repair");
        self.repair.clone().map_err(|error| error())
    }
}

fn interrupted() -> PackageSystemState {
    PackageSystemState {
        protocol_version: PROTOCOL_VERSION,
        journal_pending: true,
        packages: vec![InterruptedPackage {
            package: "better-monitor".to_string(),
            status: DpkgPackageStatus::HalfConfigured,
        }],
        omitted: 0,
    }
}

fn log(exit_code: i32, stderr: &str) -> Vec<LogEntry> {
    vec![LogEntry {
        argv: vec![
            "dpkg".to_string(),
            "--force-confdef".to_string(),
            "--force-confold".to_string(),
            "--configure".to_string(),
            "-a".to_string(),
        ],
        exit_code,
        stdout_tail: "Setting up better-monitor (0.2.8) ...".to_string(),
        stderr_tail: stderr.to_string(),
        started_at_unix: 1,
        finished_at_unix: 2,
    }]
}

fn outcome(status: RepairStatus, remaining: PackageSystemState) -> RepairOutcome {
    RepairOutcome {
        protocol_version: PROTOCOL_VERSION,
        status,
        log: log(0, ""),
        remaining: Some(remaining),
    }
}

fn last_activity(state: &ManagerState) -> (ActivityKind, String) {
    let entry = state
        .activity
        .last()
        .expect("the repair is in the activity log");
    (entry.kind, entry.evidence.clone().unwrap_or_default())
}

#[test]
fn the_state_is_asked_of_the_privileged_service_and_passed_through() {
    let service = FakeService::new(Ok(interrupted()), Err(|| unreachable!()));
    assert_eq!(package_system_state(&service).unwrap(), interrupted());
    assert_eq!(service.calls(), vec!["state"]);
}

#[test]
fn a_repair_that_finished_is_recorded_as_a_success_with_its_log() {
    let service = FakeService::new(
        Ok(interrupted()),
        Ok(outcome(RepairStatus::Repaired, PackageSystemState::clean())),
    );
    let mut state = ManagerState::default();
    let revision = state.revision;

    let report = repair_package_system(&mut state, &service);

    assert_eq!(report.result, RepairResult::Repaired);
    assert_eq!(report.log, log(0, ""));
    assert_eq!(report.remaining, Some(PackageSystemState::clean()));
    assert_eq!(service.calls(), vec!["repair"]);
    assert_eq!(
        last_activity(&state),
        (ActivityKind::Success, "package_system.repaired".to_string())
    );
    assert!(
        state.revision > revision,
        "the caller has something to save"
    );
}

#[test]
fn a_repair_that_left_packages_unfinished_says_so_and_names_them() {
    let mut remains = PackageSystemState::clean();
    remains.packages.push(InterruptedPackage {
        package: "better-awake".to_string(),
        status: DpkgPackageStatus::HalfInstalled,
    });
    let service = FakeService::new(
        Ok(interrupted()),
        Ok(outcome(RepairStatus::StillInterrupted, remains.clone())),
    );
    let mut state = ManagerState::default();

    let report = repair_package_system(&mut state, &service);

    assert_eq!(report.result, RepairResult::StillInterrupted);
    assert_eq!(report.remaining, Some(remains));
    assert_eq!(
        last_activity(&state),
        (
            ActivityKind::Warning,
            "package_system.still_interrupted".to_string()
        )
    );
}

#[test]
fn a_failed_repair_keeps_the_daemons_key_and_its_log_tail() {
    let mut failed = outcome(
        RepairStatus::Failed {
            error_key: "daemon.error.repair_failed".to_string(),
        },
        interrupted(),
    );
    failed.log = log(1, "dpkg: error processing package better-monitor");
    let service = FakeService::new(Ok(interrupted()), Ok(failed));
    let mut state = ManagerState::default();

    let report = repair_package_system(&mut state, &service);

    let RepairResult::Failed { evidence } = &report.result else {
        panic!("expected a failure, got {:?}", report.result);
    };
    assert_eq!(evidence.key, "daemon.error.repair_failed");
    assert_eq!(
        report.log[0].stderr_tail,
        "dpkg: error processing package better-monitor"
    );
    assert_eq!(
        last_activity(&state),
        (
            ActivityKind::Failure,
            "daemon.error.repair_failed".to_string()
        )
    );
}

/// Cancelling the password prompt is the person saying no. It is recorded as
/// that, with the key the window already words, and nothing else is claimed.
#[test]
fn a_refused_authorization_is_recorded_as_refused_and_nothing_else() {
    let service = FakeService::new(Ok(interrupted()), Err(|| PlatformError::PolkitDenied));
    let mut state = ManagerState::default();

    let report = repair_package_system(&mut state, &service);

    let RepairResult::Failed { evidence } = &report.result else {
        panic!("expected a refusal, got {:?}", report.result);
    };
    assert_eq!(evidence.key, "daemon.polkit_denied");
    assert!(report.log.is_empty());
    assert_eq!(report.remaining, None);
    assert_eq!(
        last_activity(&state),
        (ActivityKind::Failure, "daemon.polkit_denied".to_string())
    );
}

/// The daemon refuses when its own reading of dpkg shows nothing unfinished.
/// That is not a failure of anything, and is recorded as information.
#[test]
fn a_repair_with_nothing_to_repair_is_information_not_a_failure() {
    let service = FakeService::new(
        Ok(PackageSystemState::clean()),
        Err(|| PlatformError::DaemonRefused("daemon.error.nothing_to_repair".to_string())),
    );
    let mut state = ManagerState::default();

    let report = repair_package_system(&mut state, &service);

    assert_eq!(report.result, RepairResult::NothingToRepair);
    assert_eq!(
        last_activity(&state),
        (
            ActivityKind::Information,
            "package_system.nothing_to_repair".to_string()
        )
    );
}

#[test]
fn a_missing_privileged_service_is_reported_as_unavailable() {
    let service = FakeService::new(
        Err(|| PlatformError::DaemonUnavailable("no such service".to_string())),
        Err(|| PlatformError::DaemonUnavailable("no such service".to_string())),
    );
    assert!(matches!(
        package_system_state(&service),
        Err(PlatformError::DaemonUnavailable(_))
    ));

    let mut state = ManagerState::default();
    let report = repair_package_system(&mut state, &service);
    let RepairResult::Failed { evidence } = &report.result else {
        panic!("expected a failure, got {:?}", report.result);
    };
    assert_eq!(evidence.key, "daemon.unavailable");
}

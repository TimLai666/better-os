//! Repairing a dpkg run a crash interrupted (ADR 0014).
//!
//! The command line and the window both come here, so they ask the same
//! questions and write the same record. Nothing here runs dpkg or reads its
//! files: the privileged service decides whether a repair is needed, runs it,
//! and reports what dpkg's record said afterwards. This module turns that into
//! a result a presentation layer can word and an entry in the activity log.

use manager_ipc::RepairStatus;
/// The wire types a presentation layer reads, re-exported so the command line
/// and the window depend on the shared API rather than on the wire crate.
pub use manager_ipc::{
    DpkgPackageStatus, InterruptedPackage, LogEntry, PROTOCOL_VERSION, PackageSystemState,
};
use manager_platform::{PackageSystemRepair, PlatformError};

use crate::exec::{evidence_for, split_daemon_key};
use crate::{ActivityKind, FailureEvidence, ManagerState};

/// How a requested repair ended, in the terms a person is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairResult {
    /// dpkg finished, and its record shows nothing unfinished.
    Repaired,
    /// dpkg exited cleanly and left packages unfinished, which the report's
    /// `remaining` names.
    StillInterrupted,
    /// The privileged service read dpkg's record and found nothing to repair.
    NothingToRepair,
    /// The repair did not happen or did not finish. The key is the daemon's
    /// own, or the platform's when the daemon was never reached.
    Failed { evidence: FailureEvidence },
}

/// What a repair produced, for the caller to show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairReport {
    pub result: RepairResult,
    /// The command the daemon ran and the tail of its output. Empty when
    /// nothing ran.
    pub log: Vec<LogEntry>,
    /// dpkg's record as the daemon read it after the run, when it could.
    pub remaining: Option<PackageSystemState>,
}

/// Asks the privileged service what dpkg's record says.
pub fn package_system_state(
    service: &dyn PackageSystemRepair,
) -> Result<PackageSystemState, PlatformError> {
    service.package_system_state()
}

/// Asks the privileged service to repair the package system and records the
/// result in the activity log. [`request_repair`] followed by
/// [`record_repair`], for a caller that holds the state the whole time.
pub fn repair_package_system(
    state: &mut ManagerState,
    service: &dyn PackageSystemRepair,
) -> RepairReport {
    let report = request_repair(service);
    record_repair(state, &report);
    report
}

/// Asks the privileged service to repair the package system, and nothing else.
///
/// The window runs this off its UI thread, where it may wait on a password
/// prompt, and records the report afterwards against the state it has then.
pub fn request_repair(service: &dyn PackageSystemRepair) -> RepairReport {
    match service.repair_package_system() {
        Ok(outcome) => RepairReport {
            result: match outcome.status {
                RepairStatus::Repaired => RepairResult::Repaired,
                RepairStatus::StillInterrupted => RepairResult::StillInterrupted,
                RepairStatus::Failed { error_key } => RepairResult::Failed {
                    evidence: split_daemon_key(&error_key),
                },
            },
            log: outcome.log,
            remaining: outcome.remaining,
        },
        Err(PlatformError::DaemonRefused(key)) if key == "daemon.error.nothing_to_repair" => {
            RepairReport {
                result: RepairResult::NothingToRepair,
                log: Vec::new(),
                remaining: None,
            }
        }
        Err(error) => unreachable(&error),
    }
}

/// The report for a repair that never reached the privileged service, such as
/// one whose connection could not be made. Nothing ran, and it says why.
pub fn unreachable(error: &PlatformError) -> RepairReport {
    RepairReport {
        result: RepairResult::Failed {
            evidence: refusal_evidence(error),
        },
        log: Vec::new(),
        remaining: None,
    }
}

/// Writes a repair's result into the activity log.
///
/// The record's revision moves whatever happened, so the caller saves it. No
/// component record changes: a repair finishes dpkg's own work and says
/// nothing about which version of a component is now installed. The next
/// reconciliation reads that from dpkg as it always does.
pub fn record_repair(state: &mut ManagerState, report: &RepairReport) {
    let (kind, evidence) = match &report.result {
        RepairResult::Repaired => (ActivityKind::Success, "package_system.repaired".to_string()),
        RepairResult::StillInterrupted => (
            ActivityKind::Warning,
            "package_system.still_interrupted".to_string(),
        ),
        RepairResult::NothingToRepair => (
            ActivityKind::Information,
            "package_system.nothing_to_repair".to_string(),
        ),
        RepairResult::Failed { evidence } => (ActivityKind::Failure, evidence.key.clone()),
    };
    state.record(kind, None, None, None, Some(evidence));
    state.bump_revision();
}

/// A refusal from the bus carries the daemon's key whole, such as
/// `daemon.error.busy`. It is used as the key itself rather than wrapped in a
/// generic `daemon.refused`, so the window can say which refusal it was.
fn refusal_evidence(error: &PlatformError) -> FailureEvidence {
    match error {
        PlatformError::DaemonRefused(detail) if detail.starts_with("daemon.error.") => {
            split_daemon_key(detail)
        }
        other => evidence_for(other),
    }
}

//! The wire contract between Better Manager and its privileged daemon.
//!
//! ADR 0007 chose a D-Bus system service authorized by polkit, with plans and
//! outcomes carried as JSON documents. This crate owns those documents so both
//! halves of the protocol are generated from one definition instead of two
//! hand-matched encodings.
//!
//! It deliberately does not depend on `manager-core`. The daemon revalidates
//! every plan from scratch, and sharing the planner's types would invite it to
//! inherit the planner's trust assumptions along with them. Everything here is
//! treated as untrusted input in both directions: closed enums, no unknown
//! fields, and size limits checked before parsing.

use std::fmt;

use better_core::ComponentId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The protocol both sides must agree on. A daemon rejects any other value
/// rather than guessing which fields it can still trust.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest accepted plan document. Checked before parsing so an oversized
/// payload never reaches the deserializer.
pub const MAX_PLAN_BYTES: usize = 1024 * 1024;

/// Largest accepted outcome document, which grows with per-step execution logs.
pub const MAX_OUTCOME_BYTES: usize = 4 * 1024 * 1024;

/// Most steps one transaction may carry.
pub const MAX_STEPS: usize = 32;

/// Largest artifact the daemon will stage, enforced while streaming.
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// Longest command output kept per execution log entry. Longer output is
/// truncated rather than dropped, so a failure still carries its tail.
pub const MAX_LOG_OUTPUT_BYTES: usize = 64 * 1024;

/// The only package operations that cross the privileged boundary.
///
/// Enable, Disable, and Verify are absent on purpose. Verify needs no
/// privileges, and enabling or disabling a component has no approved
/// privileged meaning yet; see ADR 0007.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireAction {
    Install,
    Update,
    Remove,
    /// Reinstall a previously cached version, which may be a downgrade.
    Restore,
}

impl WireAction {
    /// Whether the action needs an artifact to act on. Remove works from the
    /// installed package alone.
    pub fn needs_artifact(self) -> bool {
        !matches!(self, WireAction::Remove)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            WireAction::Install => "install",
            WireAction::Update => "update",
            WireAction::Remove => "remove",
            WireAction::Restore => "restore",
        }
    }
}

impl fmt::Display for WireAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The artifact a step acts on, identified by the checksum an administrator
/// authorized rather than by a path the client chose.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireArtifact {
    /// A bare file name. Path separators and traversal are rejected so the
    /// daemon can resolve it inside its own cache and nowhere else.
    pub filename: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireStep {
    pub component: String,
    pub action: WireAction,
    /// What the client believes is installed now. The daemon compares this
    /// against dpkg and refuses on drift instead of overwriting an unexpected
    /// version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<WireArtifact>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePlan {
    pub protocol_version: u32,
    /// Client-generated identity. Re-sending a plan with a known id returns the
    /// recorded outcome instead of running it again.
    pub transaction_id: String,
    /// The release and architecture the client planned for. The daemon reads
    /// its own values and refuses a mismatch rather than trusting these.
    pub target_release: String,
    pub target_architecture: String,
    pub steps: Vec<WireStep>,
}

impl WirePlan {
    /// Parses and validates a plan document. The size limit is applied to the
    /// raw bytes first, so an oversized payload is refused without allocating
    /// its parse tree.
    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_PLAN_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_PLAN_BYTES,
            });
        }
        let plan: WirePlan =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }

    /// Checks everything that can be checked from the document alone.
    ///
    /// Host-dependent checks — that the release and architecture match this
    /// machine, that the cached bytes still hash correctly, that dpkg agrees
    /// about the installed version — belong to the daemon, which has the host
    /// to compare against.
    pub fn validate(&self) -> Result<(), IpcError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: self.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        validate_transaction_id(&self.transaction_id)?;
        validate_release(&self.target_release)?;
        validate_architecture(&self.target_architecture)?;

        if self.steps.is_empty() {
            return Err(IpcError::EmptyPlan);
        }
        if self.steps.len() > MAX_STEPS {
            return Err(IpcError::TooManySteps {
                found: self.steps.len(),
                limit: MAX_STEPS,
            });
        }

        let mut seen = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            step.validate(&self.target_release, &self.target_architecture)?;
            if seen.contains(&step.component.as_str()) {
                return Err(IpcError::DuplicateComponent {
                    component: step.component.clone(),
                });
            }
            seen.push(step.component.as_str());
        }
        Ok(())
    }
}

impl WireStep {
    fn validate(&self, release: &str, architecture: &str) -> Result<(), IpcError> {
        let component =
            ComponentId::new(self.component.clone()).map_err(|_| IpcError::InvalidComponent {
                component: self.component.clone(),
            })?;

        if let Some(version) = &self.before_version {
            validate_version(version)?;
        }

        match (&self.artifact, self.action.needs_artifact()) {
            (None, true) => {
                return Err(IpcError::MissingArtifact {
                    component: self.component.clone(),
                    action: self.action,
                });
            }
            (Some(_), false) => {
                return Err(IpcError::UnexpectedArtifact {
                    component: self.component.clone(),
                    action: self.action,
                });
            }
            _ => {}
        }

        let Some(artifact) = &self.artifact else {
            // Remove carries no target version to check.
            return Ok(());
        };

        let Some(after_version) = &self.after_version else {
            return Err(IpcError::MissingTargetVersion {
                component: self.component.clone(),
            });
        };
        validate_version(after_version)?;
        artifact.validate()?;

        // The file name is part of the release contract, so a mismatch means
        // the plan and the artifact disagree about what is being installed.
        let expected = release_asset_name(&component, after_version, release, architecture);
        if artifact.filename != expected {
            return Err(IpcError::ArtifactNameMismatch {
                component: self.component.clone(),
                expected,
                found: artifact.filename.clone(),
            });
        }
        Ok(())
    }
}

impl WireArtifact {
    fn validate(&self) -> Result<(), IpcError> {
        validate_filename(&self.filename)?;
        validate_sha256(&self.sha256)?;
        if self.size_bytes == 0 {
            return Err(IpcError::InvalidArtifactSize { bytes: 0 });
        }
        if self.size_bytes > MAX_ARTIFACT_BYTES {
            return Err(IpcError::InvalidArtifactSize {
                bytes: self.size_bytes,
            });
        }
        Ok(())
    }
}

/// The release asset name a component version must publish under, matching the
/// packaging contract in `docs/release-packaging.md`.
///
/// A version may carry an epoch because dpkg reports one, but Debian file names
/// never do, so the epoch is stripped the same way `dpkg-deb` names its output.
pub fn release_asset_name(
    component: &ComponentId,
    version: &str,
    release: &str,
    architecture: &str,
) -> String {
    let version = strip_epoch(version);
    format!("{component}_{version}_ubuntu-{release}_{architecture}.deb")
}

/// Drops a leading `epoch:` from a Debian version.
pub fn strip_epoch(version: &str) -> &str {
    match version.split_once(':') {
        Some((epoch, rest)) if !epoch.is_empty() && epoch.chars().all(|c| c.is_ascii_digit()) => {
            rest
        }
        _ => version,
    }
}

fn validate_transaction_id(value: &str) -> Result<(), IpcError> {
    // A UUID shape: 8-4-4-4-12 lowercase hex.
    let groups: Vec<&str> = value.split('-').collect();
    let well_formed = groups.len() == 5
        && [8, 4, 4, 4, 12].iter().zip(&groups).all(|(length, group)| {
            group.len() == *length
                && group
                    .chars()
                    .all(|character| character.is_ascii_hexdigit() && !character.is_uppercase())
        });
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidTransactionId {
            transaction_id: value.to_string(),
        })
    }
}

fn validate_release(value: &str) -> Result<(), IpcError> {
    let well_formed = !value.is_empty()
        && value.len() <= 16
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.');
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidTarget {
            field: "release",
            value: value.to_string(),
        })
    }
}

fn validate_architecture(value: &str) -> Result<(), IpcError> {
    let well_formed = !value.is_empty()
        && value.len() <= 16
        && value
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit());
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidTarget {
            field: "architecture",
            value: value.to_string(),
        })
    }
}

/// Accepts the Debian version characters the packaging contract can produce.
fn validate_version(value: &str) -> Result<(), IpcError> {
    let well_formed = !value.is_empty()
        && value.len() <= 64
        && value.starts_with(|character: char| character.is_ascii_digit())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-' | '~' | ':')
        });
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidVersion {
            version: value.to_string(),
        })
    }
}

/// A bare file name the daemon can safely join onto its own cache directory.
fn validate_filename(value: &str) -> Result<(), IpcError> {
    let well_formed = !value.is_empty()
        && value.len() <= 255
        && value.ends_with(".deb")
        && !value.starts_with('.')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '+' | '~')
        });
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidFilename {
            filename: value.to_string(),
        })
    }
}

fn validate_sha256(value: &str) -> Result<(), IpcError> {
    let well_formed = value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_uppercase());
    if well_formed {
        Ok(())
    } else {
        Err(IpcError::InvalidChecksum {
            sha256: value.to_string(),
        })
    }
}

/// One command the daemon ran on the host, kept so a transaction can be
/// reviewed after the fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub started_at_unix: u64,
    pub finished_at_unix: u64,
}

/// What the daemon observed after applying a step. `Undetermined` exists so a
/// daemon that could not run a check says so instead of reporting health it
/// did not verify.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum HealthResult {
    Healthy,
    Failed(String),
    Undetermined(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepReport {
    pub component: String,
    pub action: WireAction,
    /// The version dpkg reports after the step, not the version that was
    /// requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_version: Option<String>,
    pub health: HealthResult,
    pub log: Vec<LogEntry>,
}

/// Enough to put a component back, or an honest statement that there is
/// nothing to put it back to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackRecord {
    pub component: String,
    /// `None` means the component was not installed before this transaction,
    /// so rolling back means removing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_artifact: Option<WireArtifact>,
    pub transaction_id: String,
    pub recorded_at_unix: u64,
}

/// How far the daemon got putting the host back after a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireRecovery {
    Restored,
    PartiallyRestored,
    ManualRecoveryRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum OutcomeStatus {
    /// Revalidated and queued, but not started.
    Accepted,
    Executing {
        step_index: u32,
        stage: ExecutionStage,
    },
    Succeeded,
    Failed {
        /// Absent when the plan was refused before any step began.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_index: Option<u32>,
        error_key: String,
        /// Absent when the failure happened before the host was changed, which
        /// is the case where there is nothing to recover and no restore point
        /// may be invented.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovery: Option<WireRecovery>,
    },
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStage {
    Verifying,
    Applying,
    CheckingHealth,
    RollingBack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionOutcome {
    pub protocol_version: u32,
    pub transaction_id: String,
    pub status: OutcomeStatus,
    #[serde(default)]
    pub reports: Vec<StepReport>,
    #[serde(default)]
    pub rollback_records: Vec<RollbackRecord>,
}

impl TransactionOutcome {
    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_OUTCOME_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_OUTCOME_BYTES,
            });
        }
        let outcome: TransactionOutcome =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        if outcome.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: outcome.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(outcome)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }
}

/// Protocol-level rejections.
///
/// Every message is a stable machine key. Presentation layers own the localized
/// wording, matching how `manager-platform` and `manager-core` report errors.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum IpcError {
    #[error("ipc.error.payload_too_large:{bytes}:{limit}")]
    PayloadTooLarge { bytes: usize, limit: usize },
    #[error("ipc.error.malformed:{detail}")]
    Malformed { detail: String },
    #[error("ipc.error.protocol_version:{found}:{expected}")]
    ProtocolVersion { found: u32, expected: u32 },
    #[error("ipc.error.invalid_transaction_id:{transaction_id}")]
    InvalidTransactionId { transaction_id: String },
    #[error("ipc.error.invalid_target:{field}:{value}")]
    InvalidTarget { field: &'static str, value: String },
    #[error("ipc.error.empty_plan")]
    EmptyPlan,
    #[error("ipc.error.too_many_steps:{found}:{limit}")]
    TooManySteps { found: usize, limit: usize },
    #[error("ipc.error.duplicate_component:{component}")]
    DuplicateComponent { component: String },
    #[error("ipc.error.invalid_component:{component}")]
    InvalidComponent { component: String },
    #[error("ipc.error.invalid_version:{version}")]
    InvalidVersion { version: String },
    #[error("ipc.error.invalid_filename:{filename}")]
    InvalidFilename { filename: String },
    #[error("ipc.error.invalid_checksum:{sha256}")]
    InvalidChecksum { sha256: String },
    #[error("ipc.error.invalid_artifact_size:{bytes}")]
    InvalidArtifactSize { bytes: u64 },
    #[error("ipc.error.missing_artifact:{component}:{action}")]
    MissingArtifact {
        component: String,
        action: WireAction,
    },
    #[error("ipc.error.unexpected_artifact:{component}:{action}")]
    UnexpectedArtifact {
        component: String,
        action: WireAction,
    },
    #[error("ipc.error.missing_target_version:{component}")]
    MissingTargetVersion { component: String },
    #[error("ipc.error.artifact_name_mismatch:{component}:{expected}:{found}")]
    ArtifactNameMismatch {
        component: String,
        expected: String,
        found: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSACTION_ID: &str = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";

    fn artifact() -> WireArtifact {
        WireArtifact {
            filename: "better-monitor_0.1.0_ubuntu-24.04_amd64.deb".to_string(),
            sha256: "a".repeat(64),
            size_bytes: 4096,
        }
    }

    fn step() -> WireStep {
        WireStep {
            component: "better-monitor".to_string(),
            action: WireAction::Install,
            before_version: None,
            after_version: Some("0.1.0".to_string()),
            artifact: Some(artifact()),
        }
    }

    fn plan() -> WirePlan {
        WirePlan {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: TRANSACTION_ID.to_string(),
            target_release: "24.04".to_string(),
            target_architecture: "amd64".to_string(),
            steps: vec![step()],
        }
    }

    #[test]
    fn a_well_formed_plan_survives_a_json_round_trip() {
        let document = plan().to_json().unwrap();
        assert_eq!(WirePlan::from_json(&document).unwrap(), plan());
    }

    #[test]
    fn an_oversized_payload_is_refused_before_parsing() {
        let document = " ".repeat(MAX_PLAN_BYTES + 1);
        assert!(matches!(
            WirePlan::from_json(&document),
            Err(IpcError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let document = r#"{
            "protocol_version": 1,
            "transaction_id": "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "target_release": "24.04",
            "target_architecture": "amd64",
            "steps": [],
            "run_as": "root"
        }"#;
        assert!(matches!(
            WirePlan::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn another_protocol_version_is_refused_rather_than_partially_trusted() {
        let mut plan = plan();
        plan.protocol_version = PROTOCOL_VERSION + 1;
        assert!(matches!(
            plan.validate(),
            Err(IpcError::ProtocolVersion { .. })
        ));
    }

    #[test]
    fn a_plan_without_steps_is_refused() {
        let mut plan = plan();
        plan.steps.clear();
        assert!(matches!(plan.validate(), Err(IpcError::EmptyPlan)));
    }

    #[test]
    fn more_steps_than_the_limit_are_refused() {
        let mut plan = plan();
        plan.steps = (0..=MAX_STEPS).map(|_| step()).collect();
        assert!(matches!(
            plan.validate(),
            Err(IpcError::TooManySteps { .. })
        ));
    }

    #[test]
    fn one_component_may_not_appear_twice_in_a_transaction() {
        let mut plan = plan();
        plan.steps.push(step());
        assert!(matches!(
            plan.validate(),
            Err(IpcError::DuplicateComponent { .. })
        ));
    }

    #[test]
    fn a_component_name_must_be_a_valid_component_id() {
        let mut plan = plan();
        plan.steps[0].component = "Better Monitor".to_string();
        assert!(matches!(
            plan.validate(),
            Err(IpcError::InvalidComponent { .. })
        ));
    }

    #[test]
    fn a_filename_may_not_carry_a_path() {
        for filename in [
            "../../etc/better-monitor_0.1.0_ubuntu-24.04_amd64.deb",
            "/tmp/better-monitor_0.1.0_ubuntu-24.04_amd64.deb",
            "better-monitor_0.1.0_ubuntu-24.04_amd64.deb/x",
        ] {
            let mut plan = plan();
            plan.steps[0].artifact.as_mut().unwrap().filename = filename.to_string();
            assert!(
                matches!(plan.validate(), Err(IpcError::InvalidFilename { .. })),
                "{filename} should be refused"
            );
        }
    }

    #[test]
    fn a_filename_must_match_the_release_asset_contract() {
        let mut plan = plan();
        plan.steps[0].artifact.as_mut().unwrap().filename =
            "better-files_0.1.0_ubuntu-24.04_amd64.deb".to_string();
        assert!(matches!(
            plan.validate(),
            Err(IpcError::ArtifactNameMismatch { .. })
        ));
    }

    #[test]
    fn a_checksum_must_be_lowercase_hex_of_the_right_length() {
        for sha256 in ["a".repeat(63), "A".repeat(64), "z".repeat(64)] {
            let mut plan = plan();
            plan.steps[0].artifact.as_mut().unwrap().sha256 = sha256.clone();
            assert!(
                matches!(plan.validate(), Err(IpcError::InvalidChecksum { .. })),
                "{sha256} should be refused"
            );
        }
    }

    #[test]
    fn an_artifact_larger_than_the_limit_is_refused() {
        let mut plan = plan();
        plan.steps[0].artifact.as_mut().unwrap().size_bytes = MAX_ARTIFACT_BYTES + 1;
        assert!(matches!(
            plan.validate(),
            Err(IpcError::InvalidArtifactSize { .. })
        ));
    }

    #[test]
    fn an_install_without_an_artifact_is_refused() {
        let mut plan = plan();
        plan.steps[0].artifact = None;
        assert!(matches!(
            plan.validate(),
            Err(IpcError::MissingArtifact { .. })
        ));
    }

    #[test]
    fn a_removal_carrying_an_artifact_is_refused() {
        let mut plan = plan();
        plan.steps[0].action = WireAction::Remove;
        assert!(matches!(
            plan.validate(),
            Err(IpcError::UnexpectedArtifact { .. })
        ));
    }

    #[test]
    fn a_removal_needs_neither_artifact_nor_target_version() {
        let mut plan = plan();
        plan.steps[0].action = WireAction::Remove;
        plan.steps[0].artifact = None;
        plan.steps[0].after_version = None;
        plan.steps[0].before_version = Some("0.1.0".to_string());
        plan.validate().unwrap();
    }

    #[test]
    fn an_install_without_a_target_version_is_refused() {
        let mut plan = plan();
        plan.steps[0].after_version = None;
        assert!(matches!(
            plan.validate(),
            Err(IpcError::MissingTargetVersion { .. })
        ));
    }

    #[test]
    fn a_version_must_look_like_a_debian_version() {
        for version in ["v0.1.0", "0.1.0; rm -rf /", "", "0.1.0 "] {
            let mut plan = plan();
            plan.steps[0].after_version = Some(version.to_string());
            assert!(
                matches!(plan.validate(), Err(IpcError::InvalidVersion { .. })),
                "{version} should be refused"
            );
        }
    }

    #[test]
    fn a_debian_revision_is_accepted() {
        let mut plan = plan();
        plan.steps[0].after_version = Some("0.1.0-1~ubuntu24.04".to_string());
        plan.steps[0].artifact.as_mut().unwrap().filename =
            "better-monitor_0.1.0-1~ubuntu24.04_ubuntu-24.04_amd64.deb".to_string();
        plan.validate().unwrap();
    }

    #[test]
    fn an_epoch_is_accepted_in_a_version_but_never_appears_in_a_filename() {
        assert_eq!(strip_epoch("1:0.1.0-1"), "0.1.0-1");
        assert_eq!(strip_epoch("0.1.0-1"), "0.1.0-1");

        let mut plan = plan();
        plan.steps[0].after_version = Some("1:0.1.0".to_string());
        plan.validate().unwrap();
    }

    #[test]
    fn a_transaction_id_must_be_a_lowercase_uuid() {
        for transaction_id in [
            "not-a-uuid",
            "3F2504E0-4F89-41D3-9A0C-0305E82C3301",
            "3f2504e04f8941d39a0c0305e82c3301",
        ] {
            let mut plan = plan();
            plan.transaction_id = transaction_id.to_string();
            assert!(
                matches!(plan.validate(), Err(IpcError::InvalidTransactionId { .. })),
                "{transaction_id} should be refused"
            );
        }
    }

    #[test]
    fn a_target_release_or_architecture_must_be_plain() {
        let mut plan = plan();
        plan.target_release = "24.04; reboot".to_string();
        assert!(matches!(
            plan.validate(),
            Err(IpcError::InvalidTarget {
                field: "release",
                ..
            })
        ));

        let mut plan = self::tests::plan();
        plan.target_architecture = "amd64 x86".to_string();
        assert!(matches!(
            plan.validate(),
            Err(IpcError::InvalidTarget {
                field: "architecture",
                ..
            })
        ));
    }

    #[test]
    fn an_outcome_survives_a_json_round_trip() {
        let outcome = TransactionOutcome {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: TRANSACTION_ID.to_string(),
            status: OutcomeStatus::Failed {
                step_index: Some(0),
                error_key: "daemon.error.apt_failed:better-monitor".to_string(),
                recovery: Some(WireRecovery::PartiallyRestored),
            },
            reports: vec![StepReport {
                component: "better-monitor".to_string(),
                action: WireAction::Install,
                applied_version: None,
                health: HealthResult::Failed("dpkg status not installed".to_string()),
                log: vec![LogEntry {
                    argv: vec!["apt-get".to_string(), "install".to_string()],
                    exit_code: 100,
                    stdout_tail: String::new(),
                    stderr_tail: "E: Unable to locate package".to_string(),
                    started_at_unix: 1,
                    finished_at_unix: 2,
                }],
            }],
            rollback_records: vec![RollbackRecord {
                component: "better-monitor".to_string(),
                previous_version: None,
                previous_artifact: None,
                transaction_id: TRANSACTION_ID.to_string(),
                recorded_at_unix: 1,
            }],
        };

        let document = outcome.to_json().unwrap();
        assert_eq!(TransactionOutcome::from_json(&document).unwrap(), outcome);
    }

    #[test]
    fn a_failure_before_any_step_carries_no_recovery() {
        let document = r#"{
            "protocol_version": 1,
            "transaction_id": "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            "status": {
                "state": "failed",
                "error_key": "daemon.error.plan_rejected:target_mismatch"
            }
        }"#;
        let outcome = TransactionOutcome::from_json(document).unwrap();
        assert!(matches!(
            outcome.status,
            OutcomeStatus::Failed {
                step_index: None,
                recovery: None,
                ..
            }
        ));
        assert!(outcome.rollback_records.is_empty());
    }

    #[test]
    fn errors_expose_stable_machine_keys() {
        assert_eq!(
            IpcError::ProtocolVersion {
                found: 2,
                expected: 1
            }
            .to_string(),
            "ipc.error.protocol_version:2:1"
        );
        assert_eq!(
            IpcError::MissingArtifact {
                component: "better-monitor".to_string(),
                action: WireAction::Install,
            }
            .to_string(),
            "ipc.error.missing_artifact:better-monitor:install"
        );
    }
}

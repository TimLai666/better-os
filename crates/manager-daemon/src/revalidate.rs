//! Checking a plan against the host, from scratch.
//!
//! The client already validated this plan. That does not count here: the client
//! is untrusted, and `docs/security-and-rollback.md` requires the privileged
//! executor to validate a plan again before acting on it.
//!
//! Everything in this module runs before any state is touched. A rejection here
//! therefore happens before the host changed, which is why it produces no
//! rollback record — inventing a restore point for a transaction that never
//! started would be a lie.

use manager_ipc::{WireAction, WirePlan, WireStep};

use crate::apt::AptDriver;
use crate::host::HostFacts;
use crate::store::ArtifactStore;
use crate::{DaemonError, is_first_party_component};

/// Document-level and host-level checks that need no package manager.
pub fn check_plan(plan: &WirePlan, host: &HostFacts) -> Result<(), DaemonError> {
    // The shared contract first: shape, limits, closed enums, name rules.
    plan.validate()?;

    if plan.target_release != host.release {
        return Err(DaemonError::PlanRejected(format!(
            "plan targets release {} but this host is {}",
            plan.target_release, host.release
        )));
    }
    if plan.target_architecture != host.architecture {
        return Err(DaemonError::PlanRejected(format!(
            "plan targets architecture {} but this host is {}",
            plan.target_architecture, host.architecture
        )));
    }

    for step in &plan.steps {
        if !is_first_party_component(&step.component) {
            return Err(DaemonError::PlanRejected(format!(
                "{} is not a Better OS component",
                step.component
            )));
        }
    }
    Ok(())
}

/// What the host says about a component right now, compared with what the plan
/// expected.
///
/// Refusing on drift is the point: something else changed this package since
/// the plan was made, and applying anyway would overwrite a state the user
/// never reviewed.
pub fn check_no_drift(step: &WireStep, apt: &dyn AptDriver) -> Result<(), DaemonError> {
    let installed = apt.installed_version(&step.component)?;
    let expected = step.before_version.as_deref();

    let agrees = match (&installed, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => {
            manager_ipc::strip_epoch(actual) == manager_ipc::strip_epoch(expected)
        }
        _ => false,
    };
    if agrees {
        Ok(())
    } else {
        Err(DaemonError::StateDrift {
            component: step.component.clone(),
        })
    }
}

/// The last checks before a package is handed to APT: the cached bytes still
/// hash correctly, and the `.deb` really is the package the step named.
pub fn check_artifact(
    step: &WireStep,
    host: &HostFacts,
    artifacts: &ArtifactStore,
    apt: &dyn AptDriver,
) -> Result<(), DaemonError> {
    let Some(artifact) = &step.artifact else {
        return Ok(());
    };
    if !artifacts.contains(&artifact.filename) {
        return Err(DaemonError::ArtifactMissing {
            component: step.component.clone(),
        });
    }
    artifacts.verify(&artifact.filename, &artifact.sha256)?;

    let path = artifacts.path_for(&artifact.filename)?;
    let fields = apt.deb_control_fields(&path)?;

    if fields.package != step.component {
        return Err(DaemonError::PlanRejected(format!(
            "package claims to be {} but the step names {}",
            fields.package, step.component
        )));
    }
    if let Some(expected) = &step.after_version
        && manager_ipc::strip_epoch(&fields.version) != manager_ipc::strip_epoch(expected)
    {
        return Err(DaemonError::PlanRejected(format!(
            "package is version {} but the step names {expected}",
            fields.version
        )));
    }
    if fields.architecture != host.architecture && fields.architecture != "all" {
        return Err(DaemonError::PlanRejected(format!(
            "package is built for {} but this host is {}",
            fields.architecture, host.architecture
        )));
    }
    Ok(())
}

/// Whether applying this action may move a package backwards.
pub fn allows_downgrade(action: WireAction) -> bool {
    matches!(action, WireAction::Restore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::{DebFields, FakeAptDriver};
    use manager_ipc::{PROTOCOL_VERSION, WireArtifact};

    fn host() -> HostFacts {
        HostFacts {
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
        }
    }

    fn step() -> WireStep {
        WireStep {
            component: "better-monitor".to_string(),
            action: WireAction::Install,
            before_version: None,
            after_version: Some("0.1.0".to_string()),
            artifact: Some(WireArtifact {
                filename: "better-monitor_0.1.0_ubuntu-24.04_amd64.deb".to_string(),
                sha256: "a".repeat(64),
                size_bytes: 2048,
            }),
        }
    }

    fn plan() -> WirePlan {
        WirePlan {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: "3f2504e0-4f89-41d3-9a0c-0305e82c3301".to_string(),
            target_release: "24.04".to_string(),
            target_architecture: "amd64".to_string(),
            steps: vec![step()],
        }
    }

    #[test]
    fn a_plan_for_this_host_passes() {
        check_plan(&plan(), &host()).unwrap();
    }

    #[test]
    fn a_plan_built_for_another_release_or_architecture_is_refused() {
        let mut wrong_release = plan();
        wrong_release.target_release = "22.04".to_string();
        wrong_release.steps[0].artifact.as_mut().unwrap().filename =
            "better-monitor_0.1.0_ubuntu-22.04_amd64.deb".to_string();
        assert!(matches!(
            check_plan(&wrong_release, &host()),
            Err(DaemonError::PlanRejected(_))
        ));

        let mut wrong_architecture = plan();
        wrong_architecture.target_architecture = "arm64".to_string();
        wrong_architecture.steps[0]
            .artifact
            .as_mut()
            .unwrap()
            .filename = "better-monitor_0.1.0_ubuntu-24.04_arm64.deb".to_string();
        assert!(matches!(
            check_plan(&wrong_architecture, &host()),
            Err(DaemonError::PlanRejected(_))
        ));
    }

    #[test]
    fn a_plan_naming_a_package_we_do_not_own_is_refused() {
        // The wire contract accepts any valid component id; the whitelist is
        // what stops this daemon from being a general-purpose package tool.
        let mut plan = plan();
        plan.steps[0].component = "coreutils".to_string();
        plan.steps[0].artifact.as_mut().unwrap().filename =
            "coreutils_0.1.0_ubuntu-24.04_amd64.deb".to_string();
        assert!(matches!(
            check_plan(&plan, &host()),
            Err(DaemonError::PlanRejected(_))
        ));
    }

    #[test]
    fn a_host_that_moved_since_planning_stops_the_step() {
        let apt = FakeAptDriver::new().with_installed("better-monitor", "0.2.0");
        let mut step = step();
        step.before_version = Some("0.1.0".to_string());

        assert!(matches!(
            check_no_drift(&step, &apt),
            Err(DaemonError::StateDrift { .. })
        ));
    }

    #[test]
    fn an_epoch_difference_alone_is_not_drift() {
        let apt = FakeAptDriver::new().with_installed("better-monitor", "1:0.1.0");
        let mut step = step();
        step.before_version = Some("0.1.0".to_string());

        check_no_drift(&step, &apt).unwrap();
    }

    #[test]
    fn a_fresh_install_expects_the_package_to_be_absent() {
        let apt = FakeAptDriver::new().with_installed("better-monitor", "0.1.0");
        assert!(matches!(
            check_no_drift(&step(), &apt),
            Err(DaemonError::StateDrift { .. })
        ));
    }

    #[test]
    fn a_package_that_is_not_what_the_step_named_is_refused() {
        let root = std::env::temp_dir().join(format!("better-os-reval-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let artifacts = ArtifactStore::new(&root);
        let filename = "better-monitor_0.1.0_ubuntu-24.04_amd64.deb";
        let content = b"deb";
        let digest = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(content);
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        artifacts
            .stage(filename, &digest, &mut std::io::Cursor::new(content))
            .unwrap();

        let mut step = step();
        step.artifact.as_mut().unwrap().sha256 = digest;

        // The .deb says it is a different package than the step claimed.
        let apt = FakeAptDriver::new().with_deb(
            filename,
            DebFields {
                package: "better-files-example".to_string(),
                version: "0.1.0".to_string(),
                architecture: "amd64".to_string(),
            },
        );
        assert!(matches!(
            check_artifact(&step, &host(), &artifacts, &apt),
            Err(DaemonError::PlanRejected(_))
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_a_restore_may_move_a_package_backwards() {
        assert!(allows_downgrade(WireAction::Restore));
        assert!(!allows_downgrade(WireAction::Install));
        assert!(!allows_downgrade(WireAction::Update));
        assert!(!allows_downgrade(WireAction::Remove));
    }
}

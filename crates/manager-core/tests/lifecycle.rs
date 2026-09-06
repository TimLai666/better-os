use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::{
    ComponentStatus, DesiredOperation, DiskSpaceCheck, DoctorCheckKind, DriftKind, ExecutionMode,
    HealthState, Manager, ManagerError, ManagerState, MockOutcome, OperationProgress,
    OperationStage, RecoveryStatus, RestartRequirement, StageOutcome, SystemProfile,
};
use manager_platform::dpkg::FixedPackageStateProbe;

fn catalog() -> ComponentCatalog {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
    ]
    .into_iter()
    .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
    .collect::<Vec<_>>();

    ComponentCatalog::from_manifests(manifests).unwrap()
}

fn id(value: &str) -> ComponentId {
    ComponentId::new(value).unwrap()
}

/// The version the shipped manifest declares. A test that means "installed at
/// the version the catalog offers" reads it from the catalog, so a release
/// version bump does not have to edit a literal in every such test.
fn catalog_version(component: &str) -> String {
    catalog()
        .get(&id(component))
        .expect("the shipped catalog declares this component")
        .version
        .to_string()
}

fn custom_manifest(
    component: &str,
    version: &str,
    dependencies: &[(&str, &str)],
    conflicts: &[&str],
) -> ComponentManifest {
    let dependency_section = if dependencies.is_empty() {
        "dependencies: []".to_string()
    } else {
        format!(
            "dependencies:\n{}",
            dependencies
                .iter()
                .map(|(id, version)| format!("  - id: {id}\n    version: '{version}'"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let conflict_section = if conflicts.is_empty() {
        "conflicts: []".to_string()
    } else {
        format!(
            "conflicts:\n{}",
            conflicts
                .iter()
                .map(|id| format!("  - {id}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    ComponentManifest::parse_yaml(&format!(
        "schema_version: 2\nid: {component}\ndisplay_name: {component}\ncomponent_type: diagnostic\nversion: {version}\ntargets:\n  distributions: [ubuntu]\n  releases: [\"24.04\"]\n  architectures: [amd64]\nartifacts:\n  - release: \"24.04\"\n    architecture: amd64\n    url: https://example.com/{component}_{version}_ubuntu-24.04_amd64.deb\n    sha256: {checksum}\n    release_asset: {component}_{version}_ubuntu-24.04_amd64.deb\nlifecycle:\n  install: mock-install\n  enable: mock-enable\n  disable: mock-disable\n  remove: mock-remove\n  rollback: mock-rollback\n{dependency_section}\n{conflict_section}\n",
        checksum = "a".repeat(64),
    ))
    .unwrap()
}

fn custom_manager(manifests: Vec<ComponentManifest>) -> Manager {
    Manager::new(
        ComponentCatalog::from_manifests(manifests).unwrap(),
        SystemProfile::default(),
    )
}

#[test]
fn verification_failure_keeps_evidence_and_restores_the_snapshot_after_recheck() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.0.1", true);

    let plan = manager
        .plan(&state, &component, DesiredOperation::Update)
        .unwrap();
    manager.begin(&mut state, plan).unwrap();
    for _ in 0..3 {
        assert!(matches!(
            manager
                .advance_mock(&mut state, MockOutcome::Succeed)
                .unwrap(),
            OperationProgress::InProgress { .. }
        ));
    }

    let failed = manager
        .advance_mock(
            &mut state,
            MockOutcome::FailAt(OperationStage::CheckingHealth),
        )
        .unwrap();
    assert!(matches!(
        failed,
        OperationProgress::Failed { ref failure }
            if failure.stage == OperationStage::CheckingHealth
                && !failure.evidence.is_empty()
    ));
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::RestoreAvailable
    );

    let restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    manager.begin(&mut state, restore).unwrap();
    while state.active_operation.is_some() {
        manager
            .advance_mock(&mut state, MockOutcome::Succeed)
            .unwrap();
    }

    assert_eq!(
        state.component(&component).unwrap().health,
        HealthState::Healthy
    );
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::UpdateAvailable
    );
    assert_eq!(
        state
            .component(&component)
            .unwrap()
            .installed_version
            .as_deref(),
        Some("0.0.1")
    );
    assert!(
        state
            .activity
            .iter()
            .any(|entry| entry.kind.is_recovery_success())
    );
}

#[test]
fn verification_failure_preserves_the_previous_update_snapshot() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.0.1", true);

    let update = manager
        .plan(&state, &component, DesiredOperation::Update)
        .unwrap();
    complete(&manager, &mut state, update, MockOutcome::Succeed);
    assert_eq!(
        state
            .component(&component)
            .and_then(|record| record.restore_snapshot.as_ref())
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some("0.0.1")
    );

    let verify = manager
        .plan(&state, &component, DesiredOperation::Verify)
        .unwrap();
    manager.begin(&mut state, verify).unwrap();
    for _ in 0..3 {
        manager
            .advance_mock(&mut state, MockOutcome::Succeed)
            .unwrap();
    }
    manager
        .advance_mock(
            &mut state,
            MockOutcome::FailAt(OperationStage::CheckingHealth),
        )
        .unwrap();

    assert_eq!(
        state
            .component(&component)
            .and_then(|record| record.restore_snapshot.as_ref())
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some("0.0.1")
    );
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::RestoreAvailable
    );

    let restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    complete(&manager, &mut state, restore, MockOutcome::Succeed);
    assert_eq!(
        state
            .component(&component)
            .unwrap()
            .installed_version
            .as_deref(),
        Some("0.0.1")
    );
}

#[test]
fn successful_verification_keeps_the_previous_update_snapshot() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.0.1", true);

    let update = manager
        .plan(&state, &component, DesiredOperation::Update)
        .unwrap();
    complete(&manager, &mut state, update, MockOutcome::Succeed);

    let verify = manager
        .plan(&state, &component, DesiredOperation::Verify)
        .unwrap();
    complete(&manager, &mut state, verify, MockOutcome::Succeed);

    assert_eq!(
        state
            .component(&component)
            .and_then(|record| record.restore_snapshot.as_ref())
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some("0.0.1")
    );
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Healthy
    );
}

#[test]
fn planning_checks_declared_disk_space_and_exposes_release_notes() {
    let mut manifest = custom_manifest("component", "1.0.0", &[], &[]);
    manifest.artifacts[0].download_size_bytes = Some(512);
    manifest.artifacts[0].required_disk_bytes = Some(2048);
    manifest.release_notes = vec!["Initial mock release".to_string()];
    let component = id("component");
    let manager = Manager::new(
        ComponentCatalog::from_manifests([manifest.clone()]).unwrap(),
        SystemProfile {
            free_disk_bytes: Some(4096),
            ..SystemProfile::default()
        },
    );

    let plan = manager
        .plan(
            &ManagerState::default(),
            &component,
            DesiredOperation::Install,
        )
        .unwrap();
    assert_eq!(
        plan.disk_space(),
        DiskSpaceCheck::Sufficient {
            required_bytes: 2048,
            available_bytes: 4096,
        }
    );
    assert_eq!(plan.steps()[0].estimated_download_bytes, Some(512));
    assert_eq!(plan.steps()[0].required_disk_bytes, Some(2048));
    assert_eq!(
        plan.steps()[0].release_notes,
        vec!["Initial mock release".to_string()]
    );

    let insufficient_manager = Manager::new(
        ComponentCatalog::from_manifests([manifest]).unwrap(),
        SystemProfile {
            free_disk_bytes: Some(2047),
            ..SystemProfile::default()
        },
    );
    assert!(matches!(
        insufficient_manager.plan(
            &ManagerState::default(),
            &component,
            DesiredOperation::Install
        ),
        Err(ManagerError::InsufficientDiskSpace {
            required_bytes: 2048,
            available_bytes: 2047,
        })
    ));
}

#[test]
fn failure_before_component_changes_does_not_invent_a_restore_point() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();

    let plan = manager
        .plan(&state, &component, DesiredOperation::Install)
        .unwrap();
    manager.begin(&mut state, plan).unwrap();
    manager
        .advance_mock(&mut state, MockOutcome::FailAt(OperationStage::Downloading))
        .unwrap();

    let record = state.component(&component).unwrap();
    assert_eq!(record.restore_snapshot, None);
    assert_eq!(record.health, HealthState::Failed);
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Failed
    );
}

#[test]
fn a_failed_restore_keeps_the_original_restore_point() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.0.1", true);

    let update = manager
        .plan(&state, &component, DesiredOperation::Update)
        .unwrap();
    complete(&manager, &mut state, update, MockOutcome::Succeed);

    let restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    manager.begin(&mut state, restore).unwrap();
    for _ in 0..3 {
        manager
            .advance_mock(&mut state, MockOutcome::Succeed)
            .unwrap();
    }
    manager
        .advance_mock(
            &mut state,
            MockOutcome::FailAt(OperationStage::CheckingHealth),
        )
        .unwrap();

    let record = state.component(&component).unwrap();
    assert_eq!(record.installed_version.as_deref(), Some("0.0.1"));
    assert_eq!(
        record
            .restore_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some("0.0.1")
    );
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::RestoreAvailable
    );
}

#[test]
fn active_plan_keeps_the_complete_baseline_for_installed_dependencies() {
    let dependency = custom_manifest("dependency", "1.0.0", &[], &[]);
    let root = custom_manifest("root", "1.0.0", &[("dependency", ">=1.0.0")], &[]);
    let manager = custom_manager(vec![dependency, root]);
    let mut state = ManagerState::default();
    state.set_installed(id("dependency"), "1.0.0", true);

    let plan = manager
        .plan(&state, &id("root"), DesiredOperation::Install)
        .unwrap();
    assert_eq!(plan.steps().len(), 1);
    manager.begin(&mut state, plan).unwrap();

    assert!(
        state
            .active_operation
            .as_ref()
            .unwrap()
            .snapshots
            .contains_key(&id("dependency"))
    );
    manager.validate_state(&state).unwrap();
    manager.cancel(&mut state).unwrap();
    assert!(state.component(&id("root")).is_none());
    assert_eq!(
        state
            .component(&id("dependency"))
            .unwrap()
            .installed_version
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn planning_rejects_an_unsatisfied_catalog_dependency_version() {
    let dependency = custom_manifest("dependency", "1.0.0", &[], &[]);
    let root = custom_manifest("root", "1.0.0", &[("dependency", ">=2.0.0")], &[]);
    let manager = custom_manager(vec![dependency, root]);

    assert!(matches!(
        manager.plan(
            &ManagerState::default(),
            &id("root"),
            DesiredOperation::Install
        ),
        Err(ManagerError::DependencyUnavailable { .. })
    ));
}

#[test]
fn planning_rejects_conflicts_from_planned_and_reverse_components() {
    let dependency = custom_manifest("dependency", "1.0.0", &[], &[]);
    let root = custom_manifest(
        "root",
        "1.0.0",
        &[("dependency", ">=1.0.0")],
        &["dependency"],
    );
    let manager = custom_manager(vec![dependency, root]);
    assert!(matches!(
        manager.plan(
            &ManagerState::default(),
            &id("root"),
            DesiredOperation::Install
        ),
        Err(ManagerError::Conflict { .. })
    ));

    let requested = custom_manifest("requested", "1.0.0", &[], &[]);
    let installed = custom_manifest("installed", "1.0.0", &[], &["requested"]);
    let manager = custom_manager(vec![requested, installed]);
    let mut state = ManagerState::default();
    state.set_installed(id("installed"), "1.0.0", true);
    assert!(matches!(
        manager.plan(&state, &id("requested"), DesiredOperation::Install),
        Err(ManagerError::Conflict { .. })
    ));
}

#[test]
fn planning_rejects_removing_an_installed_dependency() {
    let dependency = custom_manifest("dependency", "1.0.0", &[], &[]);
    let dependent = custom_manifest("dependent", "1.0.0", &[("dependency", ">=1.0.0")], &[]);
    let manager = custom_manager(vec![dependency, dependent]);
    let mut state = ManagerState::default();
    state.set_installed(id("dependency"), "1.0.0", true);
    state.set_installed(id("dependent"), "1.0.0", true);

    assert!(matches!(
        manager.plan(&state, &id("dependency"), DesiredOperation::Remove),
        Err(ManagerError::RequiredBy { .. })
    ));
}

#[test]
fn install_disable_enable_verify_remove_and_restore_share_one_lifecycle() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();

    for operation in [
        DesiredOperation::Install,
        DesiredOperation::Disable,
        DesiredOperation::Enable,
        DesiredOperation::Verify,
    ] {
        let plan = manager.plan(&state, &component, operation).unwrap();
        complete(&manager, &mut state, plan, MockOutcome::Succeed);
    }
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Healthy
    );

    let remove = manager
        .plan(&state, &component, DesiredOperation::Remove)
        .unwrap();
    complete(&manager, &mut state, remove, MockOutcome::Succeed);
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Available
    );
    assert_eq!(
        state
            .component(&component)
            .and_then(|record| record.restore_snapshot.as_ref())
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some(catalog_version("better-monitor").as_str())
    );

    let restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    complete(&manager, &mut state, restore, MockOutcome::Succeed);
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Healthy
    );
}

#[test]
fn update_all_is_stable_and_excludes_components_that_are_already_current() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    state.set_installed(
        id("better-manager"),
        catalog_version("better-manager"),
        true,
    );
    state.set_installed(id("better-monitor"), "0.0.1", true);

    let first = manager.plan_all(&state).unwrap();
    let second = manager.plan_all(&state).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.steps().len(), 1);
    assert_eq!(first.steps()[0].component, id("better-monitor"));
    assert_eq!(first.steps()[0].operation, DesiredOperation::Update);
    assert_eq!(first.state_revision(), state.revision);
}

#[test]
fn disable_enable_and_verify_use_one_persistable_lifecycle() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), catalog_version("better-monitor"), true);

    let disable = manager
        .plan(&state, &component, DesiredOperation::Disable)
        .unwrap();
    complete(&manager, &mut state, disable, MockOutcome::Succeed);
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Disabled
    );

    let enable = manager
        .plan(&state, &component, DesiredOperation::Enable)
        .unwrap();
    complete(&manager, &mut state, enable, MockOutcome::Succeed);
    assert_eq!(
        manager.status(&state, &component).unwrap(),
        ComponentStatus::Healthy
    );

    let verify = manager
        .plan(&state, &component, DesiredOperation::Verify)
        .unwrap();
    manager.begin(&mut state, verify).unwrap();
    for _ in 0..3 {
        manager
            .advance_mock(&mut state, MockOutcome::Succeed)
            .unwrap();
    }
    manager
        .advance_mock(
            &mut state,
            MockOutcome::FailAt(OperationStage::CheckingHealth),
        )
        .unwrap();
    assert!(state.component(&component).unwrap().failure.is_some());

    let recheck = manager
        .plan(&state, &component, DesiredOperation::Verify)
        .unwrap();
    complete(&manager, &mut state, recheck, MockOutcome::Succeed);
    let record = state.component(&component).unwrap();
    assert_eq!(record.health, HealthState::Healthy);
    assert_eq!(record.failure, None);
    assert_eq!(record.recovery, None);
    assert_eq!(
        record
            .restore_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.installed_version.as_deref()),
        Some(catalog_version("better-monitor").as_str())
    );
    assert!(!record.restore_snapshot.as_ref().unwrap().enabled);
}

#[test]
fn rejects_an_active_state_that_skips_a_lifecycle_stage() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    let plan = manager
        .plan(&state, &component, DesiredOperation::Install)
        .unwrap();
    manager.begin(&mut state, plan).unwrap();
    state.active_operation.as_mut().unwrap().stage = OperationStage::CheckingHealth;

    assert!(manager.validate_state(&state).is_err());
    assert!(manager.status(&state, &component).is_err());

    let active = state.active_operation.as_mut().unwrap();
    active.stage = OperationStage::Downloading;
    active.snapshots.clear();
    assert!(manager.validate_state(&state).is_err());
}

#[test]
fn restore_can_report_partial_and_manual_recovery_without_hiding_the_result() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.0.1", true);
    let update = manager
        .plan(&state, &component, DesiredOperation::Update)
        .unwrap();
    manager.begin(&mut state, update).unwrap();
    for _ in 0..3 {
        manager
            .advance_mock(&mut state, MockOutcome::Succeed)
            .unwrap();
    }
    manager
        .advance_mock(
            &mut state,
            MockOutcome::FailAt(OperationStage::CheckingHealth),
        )
        .unwrap();

    let partial_restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    complete(
        &manager,
        &mut state,
        partial_restore,
        MockOutcome::RestorePartially,
    );
    assert_eq!(
        state.component(&component).unwrap().recovery,
        Some(RecoveryStatus::PartiallyRestored)
    );

    let manual_restore = manager
        .plan(&state, &component, DesiredOperation::Restore)
        .unwrap();
    complete(
        &manager,
        &mut state,
        manual_restore,
        MockOutcome::RestoreRequiresManualRecovery,
    );
    assert_eq!(
        state.component(&component).unwrap().recovery,
        Some(RecoveryStatus::ManualRecoveryRequired)
    );
}

#[test]
fn a_plan_carries_the_declared_replacements_enhancements_and_restart_scope() {
    let manifest = ComponentManifest::parse_yaml(include_str!(
        "../../../components/manifests/better-files-example.yaml"
    ))
    .unwrap();
    let component = manifest.id.clone();
    let manager = Manager::new(
        ComponentCatalog::from_manifests([manifest]).unwrap(),
        SystemProfile {
            release: "24.04".to_string(),
            ..SystemProfile::default()
        },
    );

    let plan = manager
        .plan(
            &ManagerState::default(),
            &component,
            DesiredOperation::Install,
        )
        .unwrap();

    assert_eq!(plan.steps()[0].replaces, vec!["org.gnome.Nautilus"]);
    assert!(plan.steps()[0].enhances.is_empty());
    assert_eq!(
        plan.steps()[0].restart_requirement,
        RestartRequirement::LogOut
    );
    assert_eq!(plan.replaces(), vec!["org.gnome.Nautilus".to_string()]);
    assert_eq!(plan.restart_requirement(), RestartRequirement::LogOut);
    assert!(plan.restart_requirement().interrupts_session());
}

#[test]
fn a_plan_reports_declared_enhancements_without_claiming_a_replacement() {
    let manager = Manager::new(catalog(), SystemProfile::default());

    let plan = manager
        .plan(
            &ManagerState::default(),
            &id("better-monitor"),
            DesiredOperation::Install,
        )
        .unwrap();

    assert_eq!(plan.enhances(), vec!["gnome-system-monitor".to_string()]);
    assert!(plan.replaces().is_empty());
    assert_eq!(
        plan.restart_requirement(),
        RestartRequirement::RestartApplication
    );
}

#[test]
fn verification_does_not_claim_a_replacement_or_a_session_interruption() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let component = id("better-monitor");
    let mut state = ManagerState::default();
    state.set_installed(component.clone(), "0.1.0", true);

    let plan = manager
        .plan(&state, &component, DesiredOperation::Verify)
        .unwrap();

    assert!(plan.steps()[0].replaces.is_empty());
    assert!(plan.steps()[0].enhances.is_empty());
    assert_eq!(plan.restart_requirement(), RestartRequirement::NotRequired);
    assert!(!plan.restart_requirement().interrupts_session());
}

#[test]
fn an_undeclared_restart_scope_is_never_reported_as_not_required() {
    let manager = custom_manager(vec![custom_manifest("component", "1.0.0", &[], &[])]);

    let plan = manager
        .plan(
            &ManagerState::default(),
            &id("component"),
            DesiredOperation::Install,
        )
        .unwrap();
    assert_eq!(plan.restart_requirement(), RestartRequirement::NotDeclared);

    assert_eq!(
        RestartRequirement::widest([
            RestartRequirement::NotRequired,
            RestartRequirement::NotDeclared,
        ]),
        RestartRequirement::NotDeclared
    );
    assert_eq!(
        RestartRequirement::widest([
            RestartRequirement::NotDeclared,
            RestartRequirement::Reboot,
            RestartRequirement::RestartApplication,
        ]),
        RestartRequirement::Reboot
    );
    assert_eq!(
        RestartRequirement::widest([]),
        RestartRequirement::NotDeclared
    );
}

#[test]
fn the_manager_takes_host_capabilities_from_the_platform_backend() {
    let platform = manager_platform::MockPlatform::new(SystemProfile {
        distribution: "zorin".to_string(),
        distribution_label: Some("Zorin OS 18".to_string()),
        release: "18".to_string(),
        architecture: "arm64".to_string(),
        free_disk_bytes: Some(8192),
    });

    let manager = Manager::probe(catalog(), &platform).unwrap();

    assert_eq!(manager.profile().distribution, "zorin");
    assert_eq!(manager.profile().free_disk_bytes, Some(8192));
    assert!(matches!(
        manager.status(&ManagerState::default(), &id("better-monitor")),
        Ok(ComponentStatus::Incompatible)
    ));
}

#[test]
fn a_real_plan_carries_the_artifact_it_would_install() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let state = ManagerState::default();

    let plan = manager
        .plan_in_mode(
            &state,
            &id("better-monitor"),
            DesiredOperation::Install,
            ExecutionMode::Real,
        )
        .unwrap();

    assert_eq!(plan.execution_mode(), ExecutionMode::Real);
    assert!(!plan.is_dry_run());
    let artifact = plan.steps()[0]
        .artifact
        .as_ref()
        .expect("a real install names what it installs");
    assert_eq!(artifact.sha256.len(), 64);
    assert!(artifact.url.as_deref().unwrap().starts_with("https://"));
    assert!(artifact.release_asset.ends_with(".deb"));
}

#[test]
fn a_simulated_plan_stays_a_simulation() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let plan = manager
        .plan(
            &ManagerState::default(),
            &id("better-monitor"),
            DesiredOperation::Install,
        )
        .unwrap();

    assert_eq!(plan.execution_mode(), ExecutionMode::Mock);
    assert!(plan.is_dry_run());
}

#[test]
fn a_persisted_real_plan_without_an_artifact_is_refused() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    let plan = manager
        .plan_in_mode(
            &state,
            &id("better-monitor"),
            DesiredOperation::Install,
            ExecutionMode::Real,
        )
        .unwrap();
    manager.begin(&mut state, plan).unwrap();

    // Strip the artifact the way a tampered or hand-edited state file would. A
    // real transaction that cannot say what it installs is not resumable, and
    // must not be loaded as if it were.
    let mut document = serde_json::to_value(&state).unwrap();
    document["active_operation"]["plan"]["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("artifact");
    let tampered: ManagerState = serde_json::from_value(document).unwrap();

    assert!(tampered.validate().is_err());
}

#[test]
fn a_real_transaction_records_which_artifact_produced_the_installed_version() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    let plan = manager
        .plan_in_mode(
            &state,
            &id("better-monitor"),
            DesiredOperation::Install,
            ExecutionMode::Real,
        )
        .unwrap();
    let expected = plan.steps()[0].artifact.clone().unwrap();

    manager.begin(&mut state, plan).unwrap();
    while state.active_operation.is_some() {
        manager
            .advance(&mut state, StageOutcome::Completed)
            .unwrap();
    }

    let record = state.component(&id("better-monitor")).unwrap();
    assert_eq!(record.installed_artifact.as_ref(), Some(&expected));
}

#[test]
fn a_real_restore_without_a_recorded_artifact_is_refused_rather_than_promised() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();

    // A restore point recorded before artifacts were tracked, which is what a
    // migrated version 1 state looks like.
    state.set_installed(id("better-monitor"), "0.1.0", true);
    let record = state.components.get_mut(&id("better-monitor")).unwrap();
    record.restore_snapshot = Some(manager_core::ComponentSnapshot {
        installed_version: Some("0.0.9".to_string()),
        enabled: true,
        health: HealthState::Healthy,
        artifact: None,
    });

    // A simulation can still walk the restore.
    manager
        .plan(&state, &id("better-monitor"), DesiredOperation::Restore)
        .unwrap();

    // A real one cannot: there is no artifact to reinstall, and saying
    // otherwise would offer a restore that cannot happen.
    assert!(matches!(
        manager.plan_in_mode(
            &state,
            &id("better-monitor"),
            DesiredOperation::Restore,
            ExecutionMode::Real,
        ),
        Err(ManagerError::RestoreArtifactMissing(_))
    ));
}

#[test]
fn a_recovery_outcome_outside_a_restore_is_rejected_rather_than_silently_ignored() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    let plan = manager
        .plan(
            &ManagerState::default(),
            &id("better-monitor"),
            DesiredOperation::Install,
        )
        .unwrap();
    manager.begin(&mut state, plan).unwrap();

    assert!(matches!(
        manager.advance(&mut state, StageOutcome::RestoredPartially),
        Err(ManagerError::UnexpectedStageOutcome { .. })
    ));
}

#[test]
fn a_host_that_agrees_with_the_record_produces_no_findings() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    state.set_installed(id("better-monitor"), "0.1.0", true);

    // Debian decoration is not disagreement: the same upstream version dressed
    // up as an epoch and a revision is still the same version.
    let probe = FixedPackageStateProbe::new(&[("better-monitor", "1:0.1.0-1~ubuntu24.04")]);
    assert!(manager.reconcile(&mut state, &probe).unwrap().is_empty());
    assert!(
        state
            .component(&id("better-monitor"))
            .unwrap()
            .drift
            .is_none()
    );
}

#[test]
fn a_component_the_host_no_longer_has_is_reported_without_rewriting_the_record() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    state.set_installed(id("better-monitor"), "0.1.0", true);

    let findings = manager
        .reconcile(&mut state, &FixedPackageStateProbe::new(&[]))
        .unwrap();

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].drift, DriftKind::MissingOnHost);
    // The record is evidence of the disagreement, so it stays as it was.
    assert_eq!(
        state
            .component(&id("better-monitor"))
            .unwrap()
            .installed_version
            .as_deref(),
        Some("0.1.0")
    );
}

#[test]
fn a_version_the_host_disagrees_about_blocks_planning_until_it_is_resolved() {
    let manager = Manager::new(catalog(), SystemProfile::default());
    let mut state = ManagerState::default();
    state.set_installed(id("better-monitor"), "0.0.1", true);

    let findings = manager
        .reconcile(
            &mut state,
            &FixedPackageStateProbe::new(&[("better-monitor", "0.9.9")]),
        )
        .unwrap();
    assert_eq!(
        findings[0].drift,
        DriftKind::VersionMismatch {
            host: "0.9.9".to_string()
        }
    );

    assert!(matches!(
        manager.plan(&state, &id("better-monitor"), DesiredOperation::Update),
        Err(ManagerError::HostDrift(_))
    ));

    // Doctor is where a person is told why.
    let checks = manager.doctor(&state).unwrap();
    assert!(
        checks
            .iter()
            .any(|check| check.kind == DoctorCheckKind::HostReconciliation)
    );
}

/// The field case, as a matrix.
///
/// A Zorin 18 machine had `better-manager` installed by apt and nothing about
/// it in the manager's own state, so the Updates screen — which counted from
/// state alone — said zero while the catalog carried a newer release. The four
/// combinations of "the record knows" and "the host has it", each against a
/// catalog that is newer and one that is level, are what that screen depends on.
mod host_reconciliation {
    use super::*;
    use manager_core::InstallProvenance;

    fn manager() -> Manager {
        Manager::new(catalog(), SystemProfile::default())
    }

    #[test]
    fn nothing_recorded_and_nothing_on_the_host_stays_available() {
        let manager = manager();
        let mut state = ManagerState::default();

        assert!(
            manager
                .reconcile(&mut state, &FixedPackageStateProbe::new(&[]))
                .unwrap()
                .is_empty()
        );
        assert!(state.component(&id("better-monitor")).is_none());
        assert_eq!(
            manager.status(&state, &id("better-monitor")).unwrap(),
            ComponentStatus::Available
        );
    }

    #[test]
    fn a_package_only_the_host_knows_about_is_adopted_at_the_version_dpkg_reports() {
        let manager = manager();
        let mut state = ManagerState::default();

        // Not drift: nothing was recorded, so there is no disagreement — apt
        // put it there and the manager simply had not looked.
        let findings = manager
            .reconcile(
                &mut state,
                &FixedPackageStateProbe::new(&[("better-manager", "0.2.3-1~ubuntu24.04")]),
            )
            .unwrap();
        assert!(findings.is_empty());

        let record = state
            .component(&id("better-manager"))
            .expect("the host's package must be adopted");
        assert_eq!(record.installed_version.as_deref(), Some("0.2.3"));
        assert_eq!(record.provenance, InstallProvenance::Dpkg);
        assert!(record.enabled);
        assert!(record.drift.is_none());
        // Nothing was captured, so nothing is claimed.
        assert!(record.installed_artifact.is_none());
        assert!(record.restore_snapshot.is_none());
        assert_eq!(record.health, HealthState::Healthy);
        // The whole point: the newer catalog release is now visible.
        assert_eq!(
            manager.status(&state, &id("better-manager")).unwrap(),
            ComponentStatus::UpdateAvailable
        );
    }

    #[test]
    fn an_adopted_package_already_at_the_catalog_version_is_healthy_not_updatable() {
        let manager = manager();
        let mut state = ManagerState::default();
        let current = catalog_version("better-manager");

        manager
            .reconcile(
                &mut state,
                &FixedPackageStateProbe::new(&[("better-manager", &current)]),
            )
            .unwrap();

        assert_eq!(
            manager.status(&state, &id("better-manager")).unwrap(),
            ComponentStatus::Healthy
        );
    }

    #[test]
    fn a_record_and_a_host_that_agree_are_left_exactly_as_they_were() {
        let manager = manager();
        let mut state = ManagerState::default();
        state.set_installed(id("better-monitor"), "0.1.0", true);
        let revision = state.revision;

        assert!(
            manager
                .reconcile(
                    &mut state,
                    &FixedPackageStateProbe::new(&[("better-monitor", "0.1.0")]),
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.revision, revision);
        let record = state.component(&id("better-monitor")).unwrap();
        assert_eq!(record.provenance, InstallProvenance::Manager);
        assert_eq!(record.installed_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn adoption_never_overwrites_a_version_the_manager_recorded() {
        let manager = manager();
        let mut state = ManagerState::default();
        state.set_installed(id("better-monitor"), "0.1.0", true);

        let findings = manager
            .reconcile(
                &mut state,
                &FixedPackageStateProbe::new(&[("better-monitor", "0.9.9")]),
            )
            .unwrap();

        assert_eq!(findings.len(), 1);
        let record = state.component(&id("better-monitor")).unwrap();
        assert_eq!(record.installed_version.as_deref(), Some("0.1.0"));
        assert_eq!(record.provenance, InstallProvenance::Manager);
    }

    /// A record that only carries a failed attempt has no installed version, so
    /// it is the "not recorded" half of the matrix — but the failure is
    /// evidence and dpkg's opinion of the package files says nothing about it.
    #[test]
    fn adopting_a_package_keeps_a_recorded_failure_visible() {
        let manager = manager();
        let mut state = ManagerState::default();
        let component = id("better-monitor");
        fail_an_install(&manager, &mut state, &component, OperationStage::Installing);
        assert!(
            state
                .component(&component)
                .unwrap()
                .installed_version
                .is_none()
        );

        manager
            .reconcile(
                &mut state,
                &FixedPackageStateProbe::new(&[("better-monitor", "0.1.0")]),
            )
            .unwrap();

        let record = state.component(&component).unwrap();
        assert_eq!(record.installed_version.as_deref(), Some("0.1.0"));
        assert_eq!(record.provenance, InstallProvenance::Dpkg);
        assert!(record.failure.is_some());
        assert_eq!(record.health, HealthState::Failed);
    }

    /// A dpkg version this crate cannot compare is worse than no version at
    /// all: every later decision would be made against a string nobody parsed.
    #[test]
    fn a_host_version_that_is_not_a_version_is_left_alone_rather_than_adopted() {
        let manager = manager();
        let mut state = ManagerState::default();

        assert!(
            manager
                .reconcile(
                    &mut state,
                    &FixedPackageStateProbe::new(&[("better-monitor", "not-a-version")]),
                )
                .unwrap()
                .is_empty()
        );
        assert!(state.component(&id("better-monitor")).is_none());
        manager
            .validate_state(&state)
            .expect("an unparseable host version must never reach the state file");
    }

    #[test]
    fn an_adopted_component_can_be_updated_through_the_ordinary_plan_flow() {
        let manager = manager();
        let mut state = ManagerState::default();
        manager
            .reconcile(
                &mut state,
                &FixedPackageStateProbe::new(&[("better-manager", "0.2.3")]),
            )
            .unwrap();

        let plan = manager
            .plan(&state, &id("better-manager"), DesiredOperation::Update)
            .expect("an adopted component must be plannable");
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(plan.steps()[0].before_version.as_deref(), Some("0.2.3"));
        assert_eq!(
            plan.steps()[0].after_version.as_deref(),
            Some(catalog_version("better-manager").as_str())
        );

        complete(&manager, &mut state, plan, MockOutcome::Succeed);
        let record = state.component(&id("better-manager")).unwrap();
        // A reviewed transaction now owns it, so the provenance stops saying
        // "someone else put this here".
        assert_eq!(record.provenance, InstallProvenance::Manager);
    }
}

/// The manager is a component in its own catalog, and the one row whose
/// lifecycle it cannot carry out from the outside.
mod self_component {
    use super::*;

    #[test]
    fn the_manager_refuses_to_plan_its_own_removal() {
        let manager = Manager::new(catalog(), SystemProfile::default());
        let mut state = ManagerState::default();
        state.set_installed(id("better-manager"), "0.2.3", true);

        assert!(matches!(
            manager.plan(&state, &id("better-manager"), DesiredOperation::Remove),
            Err(ManagerError::CannotRemoveSelf(_))
        ));
        // Every other component still removes.
        state.set_installed(id("better-monitor"), "0.1.0", true);
        assert!(
            manager
                .plan(&state, &id("better-monitor"), DesiredOperation::Remove)
                .is_ok()
        );
    }

    #[test]
    fn the_manager_updates_itself_through_the_same_plan_every_component_uses() {
        let manager = Manager::new(catalog(), SystemProfile::default());
        let mut state = ManagerState::default();
        state.set_installed(id("better-manager"), "0.2.3", true);

        let plan = manager
            .plan(&state, &id("better-manager"), DesiredOperation::Update)
            .expect("the manager must be updatable");
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(plan.steps()[0].operation, DesiredOperation::Update);
        assert!(manager_core::is_self_component(&plan.steps()[0].component));
    }
}

/// A failure is a record of one attempt, not a permanent property of a
/// component. The field state kept `health: failed` from an install that was
/// refused for a reason since fixed, and the screen showed it forever.
mod failure_clearing {
    use super::*;

    #[test]
    fn a_successful_install_clears_the_failure_the_previous_attempt_recorded() {
        let manager = Manager::new(catalog(), SystemProfile::default());
        let mut state = ManagerState::default();
        let component = id("better-monitor");

        fail_an_install(&manager, &mut state, &component, OperationStage::Installing);
        assert_eq!(
            manager.status(&state, &component).unwrap(),
            ComponentStatus::Failed
        );

        // Retry after the cause was fixed.
        let plan = manager
            .plan(&state, &component, DesiredOperation::Install)
            .expect("a failed install must be retryable");
        complete(&manager, &mut state, plan, MockOutcome::Succeed);

        let record = state.component(&component).unwrap();
        assert!(record.failure.is_none());
        assert_eq!(record.health, HealthState::Healthy);
        assert_eq!(record.recovery, None);
        assert_eq!(
            manager.status(&state, &component).unwrap(),
            ComponentStatus::Healthy
        );
    }

    #[test]
    fn a_failure_before_anything_was_applied_leaves_nothing_installed_to_verify() {
        let manager = Manager::new(catalog(), SystemProfile::default());
        let mut state = ManagerState::default();
        let component = id("better-monitor");
        fail_an_install(
            &manager,
            &mut state,
            &component,
            OperationStage::Downloading,
        );

        let record = state.component(&component).unwrap();
        assert!(record.installed_version.is_none());
        assert!(record.failure.is_some());
        // Which is why a screen must offer installing again rather than a
        // health check: there is nothing installed to check.
        assert!(matches!(
            manager.plan(&state, &component, DesiredOperation::Verify),
            Err(ManagerError::NotInstalled(_))
        ));
        assert!(
            manager
                .plan(&state, &component, DesiredOperation::Install)
                .is_ok()
        );
    }
}

/// Drives one install until it fails at the given stage.
fn fail_an_install(
    manager: &Manager,
    state: &mut ManagerState,
    component: &ComponentId,
    stage: OperationStage,
) {
    let plan = manager
        .plan(state, component, DesiredOperation::Install)
        .expect("the install must plan");
    manager.begin(state, plan).unwrap();
    while state.active_operation.is_some() {
        manager
            .advance_mock(state, MockOutcome::FailAt(stage))
            .unwrap();
    }
    assert!(
        state
            .component(component)
            .is_some_and(|record| record.failure.is_some()),
        "the install should have recorded a failure"
    );
}

fn complete(
    manager: &Manager,
    state: &mut ManagerState,
    plan: manager_core::TransactionPlan,
    final_outcome: MockOutcome,
) {
    manager.begin(state, plan).unwrap();
    while state.active_operation.is_some() {
        let stage = state.active_operation.as_ref().unwrap().stage;
        let outcome = if stage == OperationStage::CheckingHealth {
            final_outcome
        } else {
            MockOutcome::Succeed
        };
        manager.advance_mock(state, outcome).unwrap();
    }
}

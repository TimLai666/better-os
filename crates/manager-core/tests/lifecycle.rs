use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::{
    ComponentStatus, DesiredOperation, DiskSpaceCheck, ExecutionMode, HealthState, Manager,
    ManagerError, ManagerState, MockOutcome, OperationProgress, OperationStage, RecoveryStatus,
    RestartRequirement, StageOutcome, SystemProfile,
};

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
        Some("0.1.0")
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
    state.set_installed(id("better-manager"), "0.1.0", true);
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
    state.set_installed(component.clone(), "0.1.0", true);

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
        Some("0.1.0")
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

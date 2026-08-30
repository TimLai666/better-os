//! Whole event sequences, replayed through the state machine.
//!
//! These are the cases that cannot be produced on demand against real hardware:
//! pulling a stick mid-write, a flush that fails, the service dying between a
//! copy and its flush, two devices with the same serial. Replaying them as
//! recorded sequences is the only way each one is covered every time the tests
//! run.

use storage_core::machine::ObservedSignals;
use storage_core::{
    DeviceEvent, DeviceHandle, DeviceIdentity, DeviceMachine, DeviceRegistry, DeviceState,
    DeviceStateKind, DiagnosticKind, Effect, EvidencePolicy, FlushOutcome, FlushScope,
    FlushVerification, IdentityEvidence, OpenWriters, PendingWriteback, PerformanceOptIn,
    PreferenceSet, RemovalPolicy, ScanCoverage, SignalStatus, Timestamp, Transport, WritebackScope,
    WriterIdentity,
};

const MOUNT: &str = "/run/media/user/FIELD DATA";

fn identity(serial: &str, path: &str) -> DeviceIdentity {
    DeviceIdentity::from_evidence(IdentityEvidence {
        filesystem_uuid: Some("A1B2-C3D4".to_string()),
        drive_serial: Some(serial.to_string()),
        vendor: Some("Generic".to_string()),
        model: Some("Flash Disk".to_string()),
        transport: Transport::Usb,
        topology: Some("usb-0:1.2".to_string()),
        partition_number: Some(1),
        device_path: path.to_string(),
        label: Some("FIELD DATA".to_string()),
        ..IdentityEvidence::default()
    })
}

fn machine() -> DeviceMachine {
    DeviceMachine::connect(
        identity("SN-0001", "/dev/sdb1"),
        RemovalPolicy::DirectRemoval,
        Timestamp::START,
    )
}

fn signals(at: u64, pending_bytes: u64, writers: Vec<WriterIdentity>) -> DeviceEvent {
    DeviceEvent::SignalsObserved(ObservedSignals {
        at: Timestamp::from_millis(at),
        mounted: true,
        writeback: SignalStatus::Observed(PendingWriteback {
            bytes: pending_bytes,
            scope: WritebackScope::Device,
        }),
        open_writers: SignalStatus::Observed(OpenWriters {
            writers,
            coverage: ScanCoverage::Complete,
        }),
    })
}

fn mount(machine: &mut DeviceMachine, at: u64) {
    machine.apply(
        DeviceEvent::Mounted {
            mount_point: MOUNT.to_string(),
        },
        Timestamp::from_millis(at),
    );
}

fn flush_completed(at: u64) -> DeviceEvent {
    DeviceEvent::FlushCompleted(FlushVerification {
        scope: FlushScope::Filesystem,
        completed_at: Timestamp::from_millis(at),
    })
}

#[test]
fn connect_mount_write_flush_idle_unplug_ends_clean_and_says_ready_exactly_once() {
    let mut machine = machine();
    let mut kinds = Vec::new();

    mount(&mut machine, 10);
    kinds.push(machine.state().kind());

    // A copy this system started.
    kinds.push(
        machine
            .apply(
                DeviceEvent::OperationStarted {
                    operation: "copy-1".to_string(),
                },
                Timestamp::from_millis(20),
            )
            .state
            .kind(),
    );

    // The kernel still owes the device bytes.
    kinds.push(
        machine
            .apply(
                signals(30, 4 * 1024 * 1024, Vec::new()),
                Timestamp::from_millis(30),
            )
            .state
            .kind(),
    );

    // The copy finishes and its filesystem-scoped flush completes.
    let transition = machine.apply(
        DeviceEvent::OperationCompleted {
            operation: "copy-1".to_string(),
            flush: FlushOutcome::Completed(FlushVerification {
                scope: FlushScope::Filesystem,
                completed_at: Timestamp::from_millis(40),
            }),
        },
        Timestamp::from_millis(40),
    );
    assert!(transition.effects.contains(&Effect::RequestSignalRefresh));
    kinds.push(transition.state.kind());

    // Idle: nothing pending, nobody holding it.
    let transition = machine.apply(signals(50, 0, Vec::new()), Timestamp::from_millis(50));
    kinds.push(transition.state.kind());
    assert!(transition.state.permits_direct_removal());
    let proof = transition.state.readiness_proof().expect("a proof");
    assert!(proof.fully_corroborated());
    assert_eq!(
        proof.flush().map(|flush| flush.scope),
        Some(FlushScope::Filesystem)
    );

    // Unplugged while provably idle: no warning, no diagnostic.
    let transition = machine.apply(DeviceEvent::Disconnected, Timestamp::from_millis(60));
    assert_eq!(transition.state.kind(), DeviceStateKind::Disconnected);
    assert!(transition.diagnostics.is_empty());
    assert!(transition.effects.contains(&Effect::ReleaseMountState));
    let DeviceState::Disconnected(disconnected) = transition.state else {
        panic!("expected disconnected");
    };
    assert!(disconnected.unsafe_removal.is_none());

    assert_eq!(
        kinds,
        vec![
            DeviceStateKind::Unknown,
            DeviceStateKind::Writing,
            DeviceStateKind::Writing,
            // The flush completed, but the last thing anyone observed about the
            // device was four megabytes owed to it. Readiness waits for an
            // observation, not for the flush call to return.
            DeviceStateKind::Writing,
            DeviceStateKind::ReadyToUnplug,
        ],
        "readiness was claimed at the wrong point in the sequence"
    );
}

#[test]
fn unplugging_during_a_write_warns_recommends_a_check_and_never_reports_completion() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(
        DeviceEvent::OperationStarted {
            operation: "copy-1".to_string(),
        },
        Timestamp::from_millis(20),
    );
    machine.apply(
        signals(25, 8 * 1024 * 1024, Vec::new()),
        Timestamp::from_millis(25),
    );
    assert_eq!(machine.state().kind(), DeviceStateKind::Writing);

    let transition = machine.apply(DeviceEvent::Disconnected, Timestamp::from_millis(30));
    let diagnostic = transition
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == DiagnosticKind::UnsafeRemoval)
        .expect("an unsafe removal diagnostic");
    let record = diagnostic
        .unsafe_removal
        .as_ref()
        .expect("the diagnostic carries the record");
    assert!(record.recommend_filesystem_check);
    assert_eq!(record.previous_state, DeviceStateKind::Writing);
    assert_eq!(record.unfinished_operations, vec!["copy-1".to_string()]);

    let DeviceState::Disconnected(disconnected) = &transition.state else {
        panic!("expected disconnected");
    };
    assert!(disconnected.unsafe_removal.is_some());
}

#[test]
fn a_device_that_vanishes_before_its_flush_is_confirmed_is_not_recorded_as_clean() {
    let mut machine = machine();
    mount(&mut machine, 10);
    // Signals say idle, but no flush has been verified yet.
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    assert_eq!(machine.state().kind(), DeviceStateKind::Unknown);

    let transition = machine.apply(DeviceEvent::Disconnected, Timestamp::from_millis(21));
    let record = transition.diagnostics[0]
        .unsafe_removal
        .as_ref()
        .expect("a record");
    // Nothing was known to be in flight, so a filesystem check is not implied,
    // but the removal is still not reported as a clean one.
    assert!(!record.recommend_filesystem_check);
    assert_eq!(record.previous_state, DeviceStateKind::Unknown);
}

#[test]
fn a_failed_flush_leaves_the_device_unknown_until_a_later_flush_succeeds() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));

    let transition = machine.apply(
        DeviceEvent::FlushFailed {
            detail: "syncfs: input/output error".to_string(),
        },
        Timestamp::from_millis(30),
    );
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
    assert_eq!(transition.diagnostics[0].kind, DiagnosticKind::FlushFailed);

    // More clean-looking signals do not clear a failed flush.
    let transition = machine.apply(signals(40, 0, Vec::new()), Timestamp::from_millis(40));
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);

    // A flush that actually succeeds does.
    machine.apply(flush_completed(50), Timestamp::from_millis(50));
    let transition = machine.apply(signals(51, 0, Vec::new()), Timestamp::from_millis(51));
    assert_eq!(transition.state.kind(), DeviceStateKind::ReadyToUnplug);
}

#[test]
fn a_filesystem_error_is_not_cleared_by_the_next_quiet_observation() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    machine.apply(flush_completed(30), Timestamp::from_millis(30));
    assert_eq!(machine.state().kind(), DeviceStateKind::ReadyToUnplug);

    let transition = machine.apply(
        DeviceEvent::FilesystemError {
            detail: "ext4: remounted read-only".to_string(),
        },
        Timestamp::from_millis(40),
    );
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
    assert_eq!(
        transition.diagnostics[0].kind,
        DiagnosticKind::FilesystemError
    );
    assert_eq!(
        machine
            .apply(signals(50, 0, Vec::new()), Timestamp::from_millis(50))
            .state
            .kind(),
        DeviceStateKind::Unknown
    );
}

#[test]
fn a_service_restart_invalidates_everything_it_did_not_watch() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    machine.apply(flush_completed(30), Timestamp::from_millis(30));
    assert_eq!(machine.state().kind(), DeviceStateKind::ReadyToUnplug);

    let transition = machine.apply(DeviceEvent::ServiceRestarted, Timestamp::from_millis(40));
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
    assert_eq!(
        transition.diagnostics[0].kind,
        DiagnosticKind::ServiceRestartedMidState
    );
    assert!(transition.effects.contains(&Effect::RequestSignalRefresh));

    // Recovery needs fresh signals and a fresh flush, not just the passage of
    // time: the gap could have contained a write.
    let transition = machine.apply(signals(50, 0, Vec::new()), Timestamp::from_millis(50));
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
    assert!(transition.effects.contains(&Effect::RequestFilesystemFlush));
    assert_eq!(
        machine
            .apply(flush_completed(60), Timestamp::from_millis(60))
            .state
            .kind(),
        DeviceStateKind::ReadyToUnplug
    );
}

#[test]
fn a_restart_that_lands_mid_copy_still_reports_the_copy_as_writing() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(
        DeviceEvent::OperationStarted {
            operation: "copy-1".to_string(),
        },
        Timestamp::from_millis(20),
    );
    let transition = machine.apply(DeviceEvent::ServiceRestarted, Timestamp::from_millis(30));
    // The tracked operation belongs to this process's own bookkeeping, which
    // the restart notice does not erase.
    assert_eq!(transition.state.kind(), DeviceStateKind::Writing);
}

#[test]
fn a_process_holding_a_file_makes_the_device_busy_and_names_it() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    machine.apply(flush_completed(30), Timestamp::from_millis(30));

    let transition = machine.apply(
        signals(
            40,
            0,
            vec![WriterIdentity {
                pid: 4242,
                name: Some("libreoffice".to_string()),
            }],
        ),
        Timestamp::from_millis(40),
    );
    assert_eq!(transition.state.kind(), DeviceStateKind::Busy);
    assert!(!transition.state.permits_direct_removal());
}

#[test]
fn a_blocker_that_cannot_be_identified_still_blocks() {
    let mut machine = machine().with_evidence_policy(EvidencePolicy {
        require_complete_writer_scan: true,
        ..EvidencePolicy::default()
    });
    mount(&mut machine, 10);
    machine.apply(flush_completed(20), Timestamp::from_millis(20));
    let transition = machine.apply(
        DeviceEvent::SignalsObserved(ObservedSignals {
            at: Timestamp::from_millis(30),
            mounted: true,
            writeback: SignalStatus::Observed(PendingWriteback {
                bytes: 0,
                scope: WritebackScope::Device,
            }),
            open_writers: SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Partial {
                    unreadable_processes: 12,
                },
            }),
        }),
        Timestamp::from_millis(30),
    );
    assert_eq!(transition.state.kind(), DeviceStateKind::Busy);
}

#[test]
fn an_unsupported_filesystem_or_transport_reports_unknown_rather_than_ready() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    let transition = machine.apply(
        DeviceEvent::OperationCompleted {
            operation: "copy-1".to_string(),
            flush: FlushOutcome::Unsupported {
                detail: "this mount does not support a verifiable flush".to_string(),
            },
        },
        Timestamp::from_millis(30),
    );
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
    assert!(!transition.state.permits_direct_removal());
    // And it does not keep asking for a flush that cannot work.
    assert!(!transition.effects.contains(&Effect::RequestFilesystemFlush));
}

#[test]
fn a_device_unplugged_while_being_viewed_asks_for_its_mount_state_to_be_released() {
    let mut machine = machine();
    mount(&mut machine, 10);
    machine.apply(signals(20, 0, Vec::new()), Timestamp::from_millis(20));
    machine.apply(flush_completed(30), Timestamp::from_millis(30));
    let transition = machine.apply(DeviceEvent::Disconnected, Timestamp::from_millis(40));
    assert!(transition.effects.contains(&Effect::ReleaseMountState));
    assert!(machine.mount_point().is_none());
    assert!(!machine.is_connected());
}

#[test]
fn two_devices_with_one_identity_neither_inherit_the_preference_nor_merge() {
    let device = identity("SN-CLONE", "/dev/sdb1");
    let mut preferences = PreferenceSet::new();
    preferences
        .set_performance(&device, PerformanceOptIn::acknowledging_all_risks())
        .unwrap();

    let mut registry = DeviceRegistry::new(EvidencePolicy::default());
    let first = DeviceHandle::new("/org/freedesktop/UDisks2/block_devices/sdb1");
    let second = DeviceHandle::new("/org/freedesktop/UDisks2/block_devices/sdc1");
    registry.connect(
        first.clone(),
        device,
        &preferences,
        Timestamp::from_millis(1),
    );
    registry.connect(
        second.clone(),
        identity("SN-CLONE", "/dev/sdc1"),
        &preferences,
        Timestamp::from_millis(2),
    );

    assert_eq!(registry.len(), 2);
    for handle in [&first, &second] {
        let machine = registry
            .get(handle)
            .expect("both devices are still present");
        assert_eq!(machine.state().kind(), DeviceStateKind::Unknown);
    }

    // Even a full clean sequence cannot make an ambiguous device ready.
    registry.apply(
        &first,
        DeviceEvent::Mounted {
            mount_point: MOUNT.to_string(),
        },
        Timestamp::from_millis(3),
    );
    registry.apply(&first, signals(4, 0, Vec::new()), Timestamp::from_millis(4));
    let transition = registry
        .apply(&first, flush_completed(5), Timestamp::from_millis(5))
        .expect("a transition");
    assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
}

#[test]
fn a_service_restart_reaches_every_connected_device() {
    let preferences = PreferenceSet::new();
    let mut registry = DeviceRegistry::new(EvidencePolicy::default());
    for (index, handle) in ["a", "b", "c"].iter().enumerate() {
        registry.connect(
            DeviceHandle::new(*handle),
            identity(&format!("SN-{index}"), &format!("/dev/sd{handle}1")),
            &preferences,
            Timestamp::START,
        );
    }
    let transitions =
        registry.apply_to_all(DeviceEvent::ServiceRestarted, Timestamp::from_millis(9));
    assert_eq!(transitions.len(), 3);
    assert!(
        transitions
            .iter()
            .all(|(_, transition)| transition.state.kind() == DeviceStateKind::Unknown)
    );
}

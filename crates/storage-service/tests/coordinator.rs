//! The service's behavior, driven entirely by fakes.
//!
//! No D-Bus, no disk, no `/proc`. Every acceptance criterion that is about what
//! the service does — rather than about how it is reached — is asserted here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use storage_core::{DeviceHandle, DeviceStateKind, PERFORMANCE_RISK_KEYS, RemovalPolicy};
use storage_platform::fake::{
    FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, internal_disk, usb_stick,
};
use storage_platform::traits::FlushReport;
use storage_platform::{DeviceAddress, PlatformEvent};
use storage_service::coordinator::Clock;
use storage_service::protocol::StateReport;
use storage_service::{PreferenceStore, StorageCoordinator};

const OBJECT: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";
const DEVICE: &str = "/dev/sdb1";
const UUID: &str = "A1B2-C3D4";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "better-os-storage-service-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn store(&self) -> PreferenceStore {
        PreferenceStore::at_path(self.root.join("storage-preferences.json"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Harness {
    coordinator: StorageCoordinator<FakeDeviceControl>,
    control: FakeDeviceControl,
    flush: FakeFlush,
    writeback: FakeWriteback,
    open_use: FakeOpenUse,
    clock: Clock,
}

fn harness(fixture: &Fixture, control: FakeDeviceControl) -> Harness {
    let flush = FakeFlush::default();
    let writeback = FakeWriteback::idle();
    let open_use = FakeOpenUse::idle();
    let clock = Clock::manual();
    let coordinator = StorageCoordinator::new(
        control.clone(),
        Arc::new(flush.clone()),
        Arc::new(writeback.clone()),
        Arc::new(open_use.clone()),
        fixture.store(),
        clock.clone(),
    )
    .unwrap();
    Harness {
        coordinator,
        control,
        flush,
        writeback,
        open_use,
        clock,
    }
}

fn handle() -> DeviceHandle {
    DeviceHandle::new(OBJECT)
}

fn address() -> DeviceAddress {
    DeviceAddress {
        object_path: OBJECT.to_string(),
        device_path: DEVICE.to_string(),
    }
}

fn state(harness: &Harness) -> StateReport {
    harness
        .coordinator
        .report(&handle())
        .expect("the device is known")
        .state
}

/// Connects, mounts, and reaches whatever state the evidence supports.
async fn connect_and_mount(harness: &mut Harness) {
    harness.coordinator.refresh_inventory().await.unwrap();
    harness.clock.advance(10);
    harness.coordinator.mount(&handle()).await.unwrap();
}

#[tokio::test]
async fn a_device_this_host_has_never_seen_defaults_to_direct_removal() {
    let fixture = Fixture::new("default");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    harness.coordinator.refresh_inventory().await.unwrap();

    let report = harness.coordinator.report(&handle()).expect("the device");
    assert_eq!(report.policy, RemovalPolicy::DirectRemoval);
    assert_eq!(report.identity_confidence, "stable");
}

#[tokio::test]
async fn internal_disks_and_devices_without_a_drive_are_never_admitted() {
    let fixture = Fixture::new("internal");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([
            usb_stick(OBJECT, DEVICE, UUID),
            internal_disk("/objects/nvme0n1p2", "/dev/nvme0n1p2"),
        ]),
    );
    harness.coordinator.refresh_inventory().await.unwrap();
    assert_eq!(harness.coordinator.reports().len(), 1);
    assert_eq!(harness.coordinator.reports()[0].object_path, OBJECT);
}

#[tokio::test]
async fn an_idle_mounted_device_reaches_ready_and_the_flush_was_filesystem_scoped() {
    let fixture = Fixture::new("ready");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    let flushed = harness.flush.flushed();
    assert_eq!(flushed.len(), 1, "expected exactly one flush: {flushed:?}");
    assert_eq!(flushed[0], Path::new("/run/media/user/sdb1"));
}

#[tokio::test]
async fn ready_is_never_reported_while_a_write_is_pending() {
    let fixture = Fixture::new("pending");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    // Another application writes four megabytes. Nothing told this service; it
    // sees the bytes the kernel still owes the device.
    harness.writeback.pending_bytes(4 * 1024 * 1024);
    harness.clock.advance(10);
    harness.coordinator.observe(&handle()).await;

    let writing = state(&harness);
    assert_eq!(writing.kind(), DeviceStateKind::Writing);
    assert!(!writing.permits_direct_removal());

    // And it does not become ready again until the bytes are gone and a new
    // flush is verified.
    harness.writeback.pending_bytes(0);
    harness.clock.advance(10);
    harness.coordinator.observe(&handle()).await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);
    assert_eq!(harness.flush.flushed().len(), 2);
}

#[tokio::test]
async fn a_process_holding_a_file_open_keeps_the_device_busy() {
    let fixture = Fixture::new("busy");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness.open_use.set(storage_core::SignalStatus::Observed(
        storage_core::OpenWriters {
            writers: vec![storage_core::WriterIdentity {
                pid: 4242,
                name: Some("gimp".to_string()),
            }],
            coverage: storage_core::ScanCoverage::Complete,
        },
    ));
    harness.clock.advance(10);
    harness.coordinator.observe(&handle()).await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::Busy);
}

#[tokio::test]
async fn a_signal_this_session_cannot_read_leaves_the_device_unknown() {
    let fixture = Fixture::new("denied");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness
        .open_use
        .set(storage_core::SignalStatus::PermissionDenied {
            detail: "/proc/931/fd".to_string(),
        });
    harness.clock.advance(10);
    harness.coordinator.observe(&handle()).await;

    let state = state(&harness);
    assert_eq!(state.kind(), DeviceStateKind::Unknown);
    assert!(!state.permits_direct_removal());
}

#[tokio::test]
async fn a_failed_flush_is_reported_rather_than_hidden() {
    let fixture = Fixture::new("flushfail");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    harness.flush.set(FlushReport::Failed {
        detail: "syncfs: input/output error".to_string(),
    });
    connect_and_mount(&mut harness).await;

    assert_eq!(state(&harness).kind(), DeviceStateKind::Unknown);
    assert!(
        harness
            .coordinator
            .diagnostics()
            .any(|diagnostic| diagnostic.kind == storage_core::DiagnosticKind::FlushFailed)
    );
}

#[tokio::test]
async fn a_file_operation_blocks_readiness_until_its_completion_is_reported() {
    let fixture = Fixture::new("operation");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    let flushes_before = harness.flush.flushed().len();

    harness.clock.advance(10);
    harness
        .coordinator
        .operation_started(&handle(), "copy-1".to_string())
        .await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::Writing);

    harness.clock.advance(10);
    harness
        .coordinator
        .operation_completed(&handle(), "copy-1".to_string())
        .await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    // One flush for the completed operation, and nothing per written file.
    assert_eq!(harness.flush.flushed().len(), flushes_before + 1);
}

#[tokio::test]
async fn a_client_that_leaves_the_bus_ends_every_operation_it_started() {
    let fixture = Fixture::new("departed");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness.clock.advance(10);
    for (client, operation) in [(":1.7", "copy-1"), (":1.7", "copy-2"), (":1.8", "copy-3")] {
        harness
            .coordinator
            .operation_started_by(client, &handle(), operation.to_string())
            .await;
    }
    assert_eq!(state(&harness).kind(), DeviceStateKind::Writing);
    let flushes_before = harness.flush.flushed().len();

    // The first client crashes. Its two operations end, each with the flush
    // a completion brings; the other client's write still holds the device.
    harness.clock.advance(10);
    harness.coordinator.client_departed(":1.7").await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::Writing);
    assert_eq!(harness.flush.flushed().len(), flushes_before + 2);

    harness.clock.advance(10);
    harness.coordinator.client_departed(":1.8").await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);
}

#[tokio::test]
async fn a_completion_after_the_client_left_is_accepted_and_changes_nothing() {
    let fixture = Fixture::new("late-completion");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness.clock.advance(10);
    harness
        .coordinator
        .operation_started_by(":1.7", &handle(), "copy-1".to_string())
        .await;
    harness.clock.advance(10);
    harness.coordinator.client_departed(":1.7").await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    // The completion was already on its way when the client went.
    harness.clock.advance(10);
    harness
        .coordinator
        .operation_completed(&handle(), "copy-1".to_string())
        .await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    // A client that did complete its operation has nothing left to end.
    harness
        .coordinator
        .operation_started_by(":1.9", &handle(), "copy-2".to_string())
        .await;
    harness
        .coordinator
        .operation_completed(&handle(), "copy-2".to_string())
        .await;
    let flushes = harness.flush.flushed().len();
    harness.coordinator.client_departed(":1.9").await;
    harness.coordinator.client_departed(":1.7").await;
    assert_eq!(harness.flush.flushed().len(), flushes);
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);
}

#[tokio::test]
async fn an_operation_started_in_process_is_not_ended_by_any_departure() {
    let fixture = Fixture::new("in-process");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness.clock.advance(10);
    harness
        .coordinator
        .operation_started(&handle(), "copy-1".to_string())
        .await;
    harness.clock.advance(10);
    harness.coordinator.client_departed(":1.7").await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::Writing);

    harness
        .coordinator
        .operation_completed(&handle(), "copy-1".to_string())
        .await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);
}

#[tokio::test]
async fn unplugging_during_a_write_produces_a_warning_and_a_diagnostic_record() {
    let fixture = Fixture::new("unsafe");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    harness.clock.advance(10);
    harness
        .coordinator
        .operation_started(&handle(), "copy-1".to_string())
        .await;

    let mut updates = harness.coordinator.subscribe();
    harness.clock.advance(10);
    harness.control.detach(&address());
    harness
        .coordinator
        .handle_event(PlatformEvent::Removed { address: address() })
        .await;

    let report = updates.try_recv().expect("a final report for the device");
    let StateReport::Disconnected {
        unsafe_removal: Some(record),
    } = report.state
    else {
        panic!("expected an unsafe removal report, got {:?}", report.state);
    };
    assert!(record.recommend_filesystem_check);
    assert_eq!(record.unfinished_operations, vec!["copy-1".to_string()]);
    assert!(
        harness
            .coordinator
            .diagnostics()
            .any(|diagnostic| diagnostic.kind == storage_core::DiagnosticKind::UnsafeRemoval)
    );
    assert!(harness.coordinator.report(&handle()).is_none());
}

#[tokio::test]
async fn unplugging_an_idle_device_leaves_no_warning_behind() {
    let fixture = Fixture::new("clean");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    harness.clock.advance(10);
    harness
        .coordinator
        .handle_event(PlatformEvent::Removed { address: address() })
        .await;

    assert!(
        harness
            .coordinator
            .diagnostics()
            .all(|diagnostic| diagnostic.kind != storage_core::DiagnosticKind::UnsafeRemoval)
    );
    assert!(harness.coordinator.reports().is_empty());
}

#[tokio::test]
async fn performance_mode_needs_every_risk_acknowledged_and_then_requires_eject() {
    let fixture = Fixture::new("performance");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;

    let refused = harness
        .coordinator
        .set_policy(
            &handle(),
            RemovalPolicy::Performance,
            vec!["storage.performance.eject_required".to_string()],
        )
        .await;
    assert!(refused.is_err(), "a partial acknowledgement was accepted");
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    harness
        .coordinator
        .set_policy(
            &handle(),
            RemovalPolicy::Performance,
            PERFORMANCE_RISK_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect(),
        )
        .await
        .unwrap();

    let state = state(&harness);
    assert_eq!(state.kind(), DeviceStateKind::PerformanceMode);
    assert!(!state.permits_direct_removal());
    let StateReport::PerformanceMode { eject_required, .. } = state else {
        panic!("expected performance mode");
    };
    assert!(eject_required);
}

#[tokio::test]
async fn a_performance_override_survives_the_service_restarting() {
    let fixture = Fixture::new("survives");
    let control = FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]);
    let mut first = harness(&fixture, control.clone());
    first.coordinator.refresh_inventory().await.unwrap();
    first
        .coordinator
        .set_policy(
            &handle(),
            RemovalPolicy::Performance,
            PERFORMANCE_RISK_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect(),
        )
        .await
        .unwrap();
    drop(first);

    // A new process, the same preference file, and a device that came back on a
    // different kernel path.
    let mut restarted = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, "/dev/sdg1", UUID)]),
    );
    restarted.coordinator.refresh_inventory().await.unwrap();
    let report = restarted.coordinator.report(&handle()).expect("the device");
    assert_eq!(report.policy, RemovalPolicy::Performance);
    assert_eq!(report.device_path, "/dev/sdg1");
    let _ = control;
}

#[tokio::test]
async fn restoring_defaults_puts_every_device_back_and_reports_what_changed() {
    let fixture = Fixture::new("restore");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    harness.coordinator.refresh_inventory().await.unwrap();
    harness
        .coordinator
        .set_policy(
            &handle(),
            RemovalPolicy::Performance,
            PERFORMANCE_RISK_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect(),
        )
        .await
        .unwrap();

    let plan = harness.coordinator.restore_defaults().await.unwrap();
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].to, RemovalPolicy::DirectRemoval);
    assert_eq!(
        harness.coordinator.report(&handle()).unwrap().policy,
        RemovalPolicy::DirectRemoval
    );

    // Running it again changes nothing, which is what an uninstall check wants
    // to be able to assert.
    assert!(
        harness
            .coordinator
            .restore_defaults()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn eject_reports_a_drive_that_could_not_be_powered_off_honestly() {
    let fixture = Fixture::new("eject");
    let control = FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]);
    control.refuse_power_off(&address());
    let mut harness = harness(&fixture, control);
    connect_and_mount(&mut harness).await;

    let outcome = harness.coordinator.eject(&handle()).await.unwrap();
    assert!(outcome.unmounted);
    assert!(!outcome.powered_off);
    assert!(
        harness
            .control
            .calls()
            .iter()
            .any(|call| call.starts_with("eject"))
    );
}

#[tokio::test]
async fn a_service_restart_makes_every_device_unknown_until_it_is_re_observed() {
    let fixture = Fixture::new("restart");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);

    // The restart notice invalidates the evidence; the effects it asks for then
    // rebuild it, which is why the device comes back to ready by itself.
    harness.clock.advance(10);
    harness.coordinator.notify_service_restarted().await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::ReadyToUnplug);
    assert!(harness.coordinator.diagnostics().any(
        |diagnostic| diagnostic.kind == storage_core::DiagnosticKind::ServiceRestartedMidState
    ));

    // With a signal that cannot be read, the same restart leaves it unknown.
    harness
        .open_use
        .set(storage_core::SignalStatus::Unavailable {
            detail: "scan failed".to_string(),
        });
    harness.clock.advance(10);
    harness.coordinator.notify_service_restarted().await;
    assert_eq!(state(&harness).kind(), DeviceStateKind::Unknown);
}

#[tokio::test]
async fn state_outlives_a_client_that_stops_listening() {
    let fixture = Fixture::new("clientrestart");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    let first_client = harness.coordinator.subscribe();
    connect_and_mount(&mut harness).await;
    drop(first_client);

    // A new client asks from scratch and gets the current truth, not a replay.
    let mut second_client = harness.coordinator.subscribe();
    assert!(second_client.try_recv().is_err());
    let reports = harness.coordinator.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].state.kind(), DeviceStateKind::ReadyToUnplug);
}

#[tokio::test]
async fn two_devices_reporting_one_identity_are_both_left_unknown() {
    let fixture = Fixture::new("clones");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([
            usb_stick(OBJECT, DEVICE, UUID),
            usb_stick(
                "/org/freedesktop/UDisks2/block_devices/sdc1",
                "/dev/sdc1",
                UUID,
            ),
        ]),
    );
    harness.coordinator.refresh_inventory().await.unwrap();

    let reports = harness.coordinator.reports();
    assert_eq!(reports.len(), 2);
    for report in reports {
        assert_eq!(report.state.kind(), DeviceStateKind::Unknown);
        assert!(!report.state.permits_direct_removal());
    }
}

// --- Format and write image: fakes only, never a real drive -------------

#[tokio::test]
async fn format_is_refused_until_the_person_confirms_erasing_and_then_goes_through_the_platform() {
    let fixture = Fixture::new("format");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    let request = storage_platform::FormatRequest {
        kind: storage_platform::FilesystemKind::Exfat,
        label: "PHOTOS".into(),
    };
    let refused = harness.coordinator.format(&handle(), &request, false).await;
    assert!(matches!(
        refused,
        Err(storage_service::coordinator::ServiceError::Refused(_))
    ));
    assert!(
        !harness
            .control
            .calls()
            .iter()
            .any(|call| call.starts_with("format"))
    );

    harness
        .coordinator
        .format(&handle(), &request, true)
        .await
        .unwrap();
    assert!(
        harness
            .control
            .calls()
            .contains(&format!("format {OBJECT} exfat PHOTOS"))
    );
}

#[tokio::test]
async fn a_label_the_filesystem_cannot_hold_is_refused_before_anything_is_erased() {
    let fixture = Fixture::new("label");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    let too_long = storage_platform::FormatRequest {
        kind: storage_platform::FilesystemKind::Fat32,
        label: "A LABEL FAR TOO LONG".into(),
    };
    assert!(
        harness
            .coordinator
            .format(&handle(), &too_long, true)
            .await
            .is_err()
    );
    let bad = storage_platform::FormatRequest {
        kind: storage_platform::FilesystemKind::Fat32,
        label: "a*b".into(),
    };
    assert!(
        harness
            .coordinator
            .format(&handle(), &bad, true)
            .await
            .is_err()
    );
    assert!(
        !harness
            .control
            .calls()
            .iter()
            .any(|call| call.starts_with("format"))
    );
}

#[tokio::test]
async fn nothing_is_erased_while_a_file_operation_is_writing_to_the_drive() {
    let fixture = Fixture::new("busy");
    let mut harness = harness(
        &fixture,
        FakeDeviceControl::new([usb_stick(OBJECT, DEVICE, UUID)]),
    );
    connect_and_mount(&mut harness).await;
    harness
        .coordinator
        .operation_started_by(":1.42", &handle(), "copy".into())
        .await;
    let request = storage_platform::FormatRequest {
        kind: storage_platform::FilesystemKind::Ext4,
        label: "data".into(),
    };
    let refused = harness.coordinator.format(&handle(), &request, true).await;
    assert!(matches!(
        refused,
        Err(storage_service::coordinator::ServiceError::Refused(reason)) if reason == "storage.erase.busy"
    ));
}

const DRIVE: &str = "/org/freedesktop/UDisks2/drives/Generic_Flash_SN1";
const DISK: &str = "/org/freedesktop/UDisks2/block_devices/sdc";
const PART1: &str = "/org/freedesktop/UDisks2/block_devices/sdc1";
const PART2: &str = "/org/freedesktop/UDisks2/block_devices/sdc2";

/// A drive with two partitions, both mounted, and a separate stick.
async fn drive_with_two_volumes(fixture: &Fixture) -> Harness {
    use storage_platform::fake::{usb_disk, usb_partition};
    let mut harness = harness(
        fixture,
        FakeDeviceControl::new([
            usb_disk(DISK, "/dev/sdc", DRIVE),
            usb_partition(PART1, "/dev/sdc1", "AAAA-1111", DRIVE),
            usb_partition(PART2, "/dev/sdc2", "BBBB-2222", DRIVE),
            usb_stick(OBJECT, DEVICE, UUID),
        ]),
    );
    harness.coordinator.refresh_inventory().await.unwrap();
    harness.clock.advance(10);
    for volume in [PART1, PART2] {
        harness
            .coordinator
            .mount(&DeviceHandle::new(volume))
            .await
            .unwrap();
    }
    harness
}

fn drive_address(object_path: &str, device_path: &str) -> DeviceAddress {
    DeviceAddress {
        object_path: object_path.to_string(),
        device_path: device_path.to_string(),
    }
}

fn no_progress() -> storage_platform::ImageProgress {
    Arc::new(|_| {})
}

fn refused_with(
    result: Result<impl std::fmt::Debug, storage_service::coordinator::ServiceError>,
    prefix: &str,
) {
    match result {
        Err(storage_service::coordinator::ServiceError::Refused(reason)) => {
            assert!(reason.starts_with(prefix), "{reason}")
        }
        other => panic!("expected a refusal starting {prefix}, got {other:?}"),
    }
}

#[tokio::test]
async fn an_image_goes_to_the_whole_drive_after_every_volume_on_it_is_unmounted() {
    let fixture = Fixture::new("image");
    let mut harness = drive_with_two_volumes(&fixture).await;
    let image = fixture.root.join("installer.iso");
    std::fs::write(&image, b"ISO 9660 stand-in").unwrap();
    let disk = DeviceHandle::new(DISK);

    refused_with(
        harness
            .coordinator
            .write_image(&disk, &image, false, no_progress())
            .await,
        "storage.erase.not_confirmed",
    );
    // A partition is not a drive: the image would land inside it.
    refused_with(
        harness
            .coordinator
            .write_image(&DeviceHandle::new(PART1), &image, true, no_progress())
            .await,
        "storage.image.not_a_drive",
    );
    refused_with(
        harness
            .coordinator
            .write_image(&disk, Path::new("relative.iso"), true, no_progress())
            .await,
        "storage.image.not_a_file",
    );
    assert_eq!(
        harness.control.written(&drive_address(DISK, "/dev/sdc")),
        None
    );

    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let progress: storage_platform::ImageProgress = {
        let heard = heard.clone();
        Arc::new(move |bytes| heard.lock().unwrap().push(bytes))
    };
    let written = harness
        .coordinator
        .write_image(&disk, &image, true, progress)
        .await
        .unwrap();
    assert_eq!(written, 17);
    assert_eq!(
        harness
            .control
            .written(&drive_address(DISK, "/dev/sdc"))
            .unwrap(),
        b"ISO 9660 stand-in"
    );
    assert_eq!(*heard.lock().unwrap(), vec![8, 17]);

    // Both volumes were unmounted, and before the write.
    let calls = harness.control.calls();
    let at = |call: String| calls.iter().position(|made| *made == call).unwrap();
    let write = at(format!("write image {DISK} {}", image.display()));
    assert!(at(format!("unmount {PART1}")) < write);
    assert!(at(format!("unmount {PART2}")) < write);
    for volume in [PART1, PART2] {
        let report = harness
            .coordinator
            .report(&DeviceHandle::new(volume))
            .unwrap();
        assert_eq!(report.mount_point, None);
        assert!(!report.erasing, "the write is over");
    }
    // The stick on another drive was never touched.
    assert!(!calls.contains(&format!("unmount {OBJECT}")));
}

#[tokio::test]
async fn a_volume_that_will_not_unmount_stops_the_write_before_a_byte_is_written() {
    let fixture = Fixture::new("image-busy-volume");
    let mut harness = drive_with_two_volumes(&fixture).await;
    harness
        .control
        .refuse_unmount(&drive_address(PART2, "/dev/sdc2"));
    let image = fixture.root.join("installer.iso");
    std::fs::write(&image, b"ISO").unwrap();

    refused_with(
        harness
            .coordinator
            .write_image(&DeviceHandle::new(DISK), &image, true, no_progress())
            .await,
        "storage.image.unmount_failed:/dev/sdc2",
    );
    assert_eq!(
        harness.control.written(&drive_address(DISK, "/dev/sdc")),
        None
    );
    // Nothing is left marked as being written.
    assert!(!harness.coordinator.is_erasing(&DeviceHandle::new(PART1)));
    harness
        .coordinator
        .mount(&DeviceHandle::new(PART1))
        .await
        .unwrap();
}

#[tokio::test]
async fn while_an_image_is_written_the_drive_and_its_volumes_refuse_everything_else() {
    let fixture = Fixture::new("image-erasing");
    let mut harness = drive_with_two_volumes(&fixture).await;
    let image = fixture.root.join("installer.iso");
    std::fs::write(&image, b"ISO").unwrap();
    let disk = DeviceHandle::new(DISK);
    let volume = DeviceHandle::new(PART1);

    let write = harness
        .coordinator
        .begin_image_write(&disk, &image, true)
        .await
        .unwrap();
    assert_eq!(write.total, 3);
    assert!(harness.coordinator.report(&volume).unwrap().erasing);
    refused_with(
        harness.coordinator.mount(&volume).await,
        "storage.erase.busy",
    );
    refused_with(
        harness.coordinator.eject(&volume).await,
        "storage.erase.busy",
    );
    let format = storage_platform::FormatRequest {
        kind: storage_platform::FilesystemKind::Exfat,
        label: "X".into(),
    };
    refused_with(
        harness.coordinator.format(&volume, &format, true).await,
        "storage.erase.busy",
    );
    refused_with(
        harness
            .coordinator
            .begin_image_write(&disk, &image, true)
            .await,
        "storage.erase.busy",
    );
    // Another drive is served as usual.
    harness.coordinator.mount(&handle()).await.unwrap();

    harness.coordinator.finish_image_write(&write);
    harness.coordinator.mount(&volume).await.unwrap();
}

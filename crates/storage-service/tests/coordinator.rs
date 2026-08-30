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

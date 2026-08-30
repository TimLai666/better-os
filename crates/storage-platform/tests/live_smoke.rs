//! Live checks against the real UDisks2 and the real kernel interfaces.
//!
//! Ignored by default: they need a session with UDisks2 running, and one of
//! them needs a USB stick physically plugged in. CI has neither, and a test
//! that silently passes on a machine with no external storage would be worse
//! than no test.
//!
//! Run them by hand:
//!
//! ```text
//! # every live check that does not need hardware plugged in
//! cargo test -p storage-platform --test live_smoke -- --ignored --nocapture
//!
//! # the one that needs a USB stick or SD card connected
//! cargo test -p storage-platform --test live_smoke -- --ignored --nocapture \
//!     an_external_device_is_detected_and_identified_stably
//! ```
//!
//! `cargo run -p storage-platform --bin better-storage-doctor` prints the same
//! information in a form meant for reading rather than asserting.

use storage_core::{DeviceIdentity, IdentityConfidence, SignalStatus};
use storage_platform::traits::DeviceControl;
use storage_platform::traits::{OpenUseInspector, WritebackInspector};
use storage_platform::{LinuxWriteback, ProcOpenUse, Roots, UDisks2};

#[tokio::test]
#[ignore = "needs a live UDisks2 on the system bus"]
async fn udisks2_answers_and_every_block_device_classifies_without_panicking() {
    let udisks = UDisks2::connect()
        .await
        .expect("UDisks2 is reachable on the system bus");
    let devices = udisks.enumerate().await.expect("an inventory");
    println!("block devices: {}", devices.len());
    assert!(
        !devices.is_empty(),
        "a running system always has at least one block device"
    );

    for device in &devices {
        let class = device.classify();
        let identity = DeviceIdentity::from_evidence(device.identity_evidence());
        println!(
            "{:<24} {:?} {} ({:?})",
            device.address.device_path,
            class,
            identity.key(),
            identity.confidence()
        );
        // The system disk must never be classified as external, whatever this
        // particular machine looks like.
        if device.block.hint_system {
            assert!(!class.is_external());
        }
    }
}

#[tokio::test]
#[ignore = "needs a USB stick or SD card physically connected"]
async fn an_external_device_is_detected_and_identified_stably() {
    let udisks = UDisks2::connect().await.expect("UDisks2 is reachable");
    let devices = udisks.enumerate().await.expect("an inventory");
    let external: Vec<_> = devices
        .iter()
        .filter(|device| device.classify().is_external())
        .collect();
    assert!(
        !external.is_empty(),
        "no external hot-pluggable device is connected; plug one in and run this again"
    );

    for device in external {
        let identity = DeviceIdentity::from_evidence(device.identity_evidence());
        println!(
            "{} -> {} ({:?})",
            device.address.device_path,
            identity.key(),
            identity.confidence()
        );
        assert_ne!(
            identity.confidence(),
            IdentityConfidence::Volatile,
            "{} has no stable identifier at all, so no preference could ever be remembered for it",
            device.address.device_path
        );
    }
}

#[test]
#[ignore = "reads the running host's /proc and /sys"]
fn the_host_reports_which_writeback_and_open_use_signals_it_actually_has() {
    let roots = Roots::system();
    let writeback = LinuxWriteback::new(roots.clone());
    let open_use = ProcOpenUse::new(roots);

    // `/` is always mounted, so this reports what the signals look like on this
    // machine rather than depending on external hardware.
    let status = open_use.open_writers(std::path::Path::new("/"));
    match &status {
        SignalStatus::Observed(open) => println!(
            "open writers on /: {} found, coverage {:?}",
            open.writers.len(),
            open.coverage
        ),
        other => println!("open writers on /: {other:?}"),
    }
    assert!(
        matches!(status, SignalStatus::Observed(_)),
        "a Linux host with procfs should always produce an open-writer observation"
    );

    let device = storage_platform::PlatformDevice {
        address: storage_platform::DeviceAddress {
            object_path: String::new(),
            device_path: "/dev/does-not-exist".to_string(),
        },
        block: storage_platform::BlockInfo {
            device_path: "/dev/does-not-exist".to_string(),
            ..storage_platform::BlockInfo::default()
        },
        drive: None,
        mount_point: None,
    };
    println!("writeback fallback: {:?}", writeback.pending(&device));
}

//! What this service sends, read by the Better Files that v0.2.16 shipped
//! (ticket 171). The two packages update separately, so a released window
//! talks to a newer service. Its protocol types are copied below from the
//! `v0.2.16` tag (`crates/storage-service/src/protocol.rs`) exactly as they
//! were, `deny_unknown_fields` included: a field this service adds to a
//! report would make that window throw the whole device list away and stop
//! showing devices until it is updated itself.

use std::sync::Arc;

use storage_core::{DeviceHandle, RemovalPolicy};
use storage_platform::fake::{FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, usb_stick};
use storage_service::coordinator::Clock;
use storage_service::protocol::{DeviceListDocument, EjectReport, PROTOCOL_VERSION};
use storage_service::{PreferenceStore, StorageCoordinator};

/// v0.2.16's own types, copied from the tag.
mod released {
    use serde::{Deserialize, Serialize};
    use storage_core::RemovalPolicy;

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum BlockerReport {
        Process { pid: i32, name: Option<String> },
        Unidentified { detail: String },
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct UnsafeRemovalReport {
        pub at_millis: u64,
        pub previous_state: String,
        pub unfinished_operations: Vec<String>,
        pub detail: String,
        pub recommend_filesystem_check: bool,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case", tag = "state")]
    pub enum StateReport {
        ReadyToUnplug {
            proven_at_millis: u64,
            flush_scope: Option<String>,
            fully_corroborated: bool,
            mounted: bool,
        },
        Writing {
            reason: String,
            detail: String,
        },
        Busy {
            blockers: Vec<BlockerReport>,
        },
        PerformanceMode {
            eject_required: bool,
            active_write: bool,
        },
        Unknown {
            reason: String,
            detail: String,
        },
        Disconnected {
            unsafe_removal: Option<UnsafeRemovalReport>,
        },
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct DeviceReport {
        pub object_path: String,
        pub device_path: String,
        pub display_name: String,
        pub identity: String,
        pub identity_confidence: String,
        pub filesystem: Option<String>,
        pub mount_point: Option<String>,
        pub policy: RemovalPolicy,
        pub state: StateReport,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub drive: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub drive_name: Option<String>,
        #[serde(default)]
        pub size: u64,
        #[serde(default)]
        pub whole_disk: bool,
        #[serde(default)]
        pub erasing: bool,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct DeviceListDocument {
        pub protocol_version: u32,
        pub devices: Vec<DeviceReport>,
    }

    /// The request a released service reads (the other way round: a new
    /// window asking an old service).
    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct FormatDeviceRequest {
        pub protocol_version: u32,
        pub object_path: String,
        pub filesystem: storage_platform::FilesystemKind,
        pub label: String,
        pub erase_confirmed: bool,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct EjectReport {
        pub protocol_version: u32,
        pub object_path: String,
        pub unmounted: bool,
        pub powered_off: bool,
        pub detail: String,
    }
}

const OBJECT: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";

#[tokio::test]
async fn a_released_better_files_reads_every_list_report_and_eject_this_service_sends() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Clock::manual();
    let mut coordinator = StorageCoordinator::new(
        FakeDeviceControl::new([usb_stick(OBJECT, "/dev/sdb1", "A1B2-C3D4")]),
        Arc::new(FakeFlush::default()),
        Arc::new(FakeWriteback::idle()),
        Arc::new(FakeOpenUse::idle()),
        PreferenceStore::at_path(directory.path().join("storage.json")),
        clock.clone(),
    )
    .unwrap();
    coordinator.refresh_inventory().await.unwrap();
    clock.advance(10);
    coordinator.mount(&DeviceHandle::new(OBJECT)).await.unwrap();

    // `ListDevices`, as the service answers it.
    let list = DeviceListDocument::new(coordinator.reports())
        .to_json()
        .unwrap();
    let read: released::DeviceListDocument =
        serde_json::from_str(&list).expect("v0.2.16 reads the device list");
    assert_eq!(read.protocol_version, PROTOCOL_VERSION);
    assert_eq!(read.devices.len(), 1);
    assert_eq!(read.devices[0].policy, RemovalPolicy::DirectRemoval);

    // `DeviceStateChanged`, one report as the signal carries it.
    for report in coordinator.reports() {
        let signal = serde_json::to_string(&report).unwrap();
        serde_json::from_str::<released::DeviceReport>(&signal)
            .expect("v0.2.16 reads a state change");
    }

    // `Eject`'s answer.
    let outcome = coordinator.eject(&DeviceHandle::new(OBJECT)).await.unwrap();
    let eject = EjectReport {
        protocol_version: PROTOCOL_VERSION,
        object_path: OBJECT.to_string(),
        unmounted: outcome.unmounted,
        powered_off: outcome.powered_off,
        detail: outcome.detail.clone(),
    }
    .to_json()
    .unwrap();
    serde_json::from_str::<released::EjectReport>(&eject).expect("v0.2.16 reads an eject");
}

/// The other way round: a report from a newer service with a field this one
/// does not know is still read.
#[test]
fn a_report_with_a_field_added_later_is_still_read() {
    let mut value: serde_json::Value = serde_json::to_value(released::DeviceListDocument {
        protocol_version: PROTOCOL_VERSION,
        devices: Vec::new(),
    })
    .unwrap();
    value["added_later"] = serde_json::json!(true);
    DeviceListDocument::from_json(&value.to_string()).expect("an added field is ignored");
}

/// A new window asks a released service to format: with the identity the
/// released service refuses the request naming the field (which is what
/// the client sees to ask again without it), and without it the request is
/// one the released service reads.
#[test]
fn a_format_request_without_identity_is_one_a_released_service_reads() {
    let request = |identity: Option<&str>| storage_service::protocol::FormatDeviceRequest {
        protocol_version: PROTOCOL_VERSION,
        object_path: OBJECT.to_string(),
        filesystem: storage_platform::FilesystemKind::Exfat,
        label: "PHOTOS".to_string(),
        erase_confirmed: true,
        identity: identity.map(str::to_string),
    };
    let without = request(None).to_json().unwrap();
    serde_json::from_str::<released::FormatDeviceRequest>(&without)
        .expect("v0.2.16 reads a request with no identity");
    let with = request(Some("uuid:A1B2-C3D4")).to_json().unwrap();
    let refusal = serde_json::from_str::<released::FormatDeviceRequest>(&with)
        .expect_err("v0.2.16 reads requests strictly")
        .to_string();
    assert!(refusal.contains("unknown field `identity`"), "{refusal}");
}

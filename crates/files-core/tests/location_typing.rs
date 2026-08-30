//! The rule Issue #6 asks to be enforced from the first commit: an
//! Applications row is not a file, and no public API turns one into a path.
//!
//! These assertions are as much about the shape of the API as about the
//! values. A test that only checked `as_local_path() == None` would keep
//! passing if someone added a `From<Entry> for PathBuf`; the compile-fail
//! commentary below records what must not exist, and the runtime assertions
//! cover every location and entry kind rather than the one that was
//! convenient.

use std::path::PathBuf;

use app_catalog_core::{CatalogBuilder, DesktopId, DirectoryRank, EntryScope, ExecutableProbe};
use files_core::applications::{ApplicationView, OpenIntent, OpenRefusal};
use files_core::{
    DeviceLocation, Entry, EntryBody, EntryKind, ListingRequest, ListingSession, Location,
    LocationKind, NetworkLocation, NetworkScheme, TrashLocation,
};
use storage_core::{DeviceIdentity, IdentityEvidence, Transport};

struct AlwaysResolves;

impl ExecutableProbe for AlwaysResolves {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        Some(PathBuf::from("/usr/bin").join(program))
    }
}

fn applications_entries() -> Vec<Entry> {
    let probe = AlwaysResolves;
    let mut builder = CatalogBuilder::new(&probe);
    let directory = DirectoryRank {
        rank: 0,
        scope: EntryScope::System,
    };
    builder.add_entry(
        DesktopId::new("org.example.Editor.desktop").unwrap(),
        PathBuf::from("/usr/share/applications/org.example.Editor.desktop"),
        &directory,
        b"[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\n",
    );
    let catalog = builder.build();

    let request = ListingRequest::new(Location::Applications);
    let (mut session, mut sink) = ListingSession::start(&request);
    files_core::list_applications(&catalog, &ApplicationView::default(), &mut sink).unwrap();
    sink.finish().unwrap();
    session
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            files_core::ListingEvent::Batch(batch) => Some(batch.entries),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn an_applications_entry_can_never_be_coerced_into_a_filesystem_path() {
    let entries = applications_entries();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];

    // The only accessor there is answers `None`.
    assert_eq!(entry.as_local_path(), None);

    // And the body carries a desktop ID, not anything path-shaped. There is
    // deliberately no `From<ApplicationFacts> for PathBuf`, no `Deref` to a
    // path, and no `path()` on the facts: adding any of them would let a
    // consumer hand `std::fs` a value Issue #4 says must never exist.
    match &entry.body {
        EntryBody::Application(facts) => {
            assert_eq!(facts.desktop_id.as_str(), "org.example.Editor.desktop");
        }
        other => panic!("expected an application body, got {other:?}"),
    }

    // Opening it produces a launch intent, not a path to execute.
    match files_core::open_intent(entry) {
        OpenIntent::Launch { desktop_id, action } => {
            assert_eq!(desktop_id.as_str(), "org.example.Editor.desktop");
            assert_eq!(action, None);
        }
        other => panic!("expected a launch intent, got {other:?}"),
    }
}

#[test]
fn only_the_filesystem_backed_locations_yield_a_path() {
    let identity = DeviceIdentity::from_evidence(IdentityEvidence {
        device_path: "/dev/sdb1".to_string(),
        filesystem_uuid: Some("1234-ABCD".to_string()),
        transport: Transport::Usb,
        ..IdentityEvidence::default()
    });

    let cases: Vec<(Location, LocationKind, bool)> = vec![
        (
            Location::local("/home/user").unwrap(),
            LocationKind::Local,
            true,
        ),
        (Location::Applications, LocationKind::Applications, false),
        (Location::Recent, LocationKind::Recent, false),
        (
            Location::Trash(TrashLocation::Root),
            LocationKind::Trash,
            false,
        ),
        (
            Location::Device(Box::new(DeviceLocation::new(identity, "photos"))),
            LocationKind::Device,
            false,
        ),
        (
            Location::Network(
                NetworkLocation::new(NetworkScheme::Smb, "server", "/share").unwrap(),
            ),
            LocationKind::Network,
            false,
        ),
        (
            Location::parse_uri("mtp://phone/DCIM"),
            LocationKind::Unsupported,
            false,
        ),
    ];

    for (location, kind, has_path) in cases {
        assert_eq!(location.kind(), kind);
        assert_eq!(
            location.as_local_path().is_some(),
            has_path,
            "{location} reported the wrong kind of path availability"
        );
    }
}

#[test]
fn a_device_location_has_no_path_until_a_mount_point_is_supplied() {
    let identity = DeviceIdentity::from_evidence(IdentityEvidence {
        device_path: "/dev/sdb1".to_string(),
        drive_serial: Some("SN12345".to_string()),
        transport: Transport::Usb,
        ..IdentityEvidence::default()
    });
    let device = DeviceLocation::new(identity, "photos/2024");
    let location = Location::Device(Box::new(device.clone()));

    // Nothing about the location alone produces a path.
    assert_eq!(location.as_local_path(), None);

    // The resolution requires the caller to say where the device is mounted,
    // which is the only place that answer exists.
    let resolved = device
        .resolve(std::path::Path::new("/media/user/STICK"))
        .unwrap();
    assert_eq!(
        resolved.as_path(),
        std::path::Path::new("/media/user/STICK/photos/2024")
    );
}

#[test]
fn a_trashed_entry_is_not_openable_as_a_file() {
    use files_core::{FileTime, LocalPath, TrashedFacts};

    let entry = Entry {
        name: "report.txt".to_string(),
        kind: EntryKind::File,
        size: files_core::EntrySize::Bytes(10),
        modified: None,
        permissions: files_core::PermissionsSummary::UNKNOWN,
        hidden: files_core::HiddenState::Visible,
        mime: None,
        body: EntryBody::Trashed(TrashedFacts {
            item: "report.txt".to_string(),
            original_path: PathBuf::from("/home/user/report.txt"),
            deleted_at: Some(FileTime::new(1_700_000_000, 0)),
            stored_path: LocalPath::new("/home/user/.local/share/Trash/files/report.txt").unwrap(),
        }),
    };
    assert_eq!(entry.as_local_path(), None);
    assert_eq!(
        files_core::open_intent(&entry),
        OpenIntent::Refused(OpenRefusal::ItemIsInTrash)
    );
}

#[test]
fn an_unsupported_location_survives_a_round_trip_without_being_reinterpreted() {
    // A tab saved by a future version that speaks a scheme this build does
    // not. It must come back out exactly as it went in, so upgrading does not
    // silently lose the user's session.
    let raw = "sftp+kerberos://gateway.example/home/user";
    let parsed = Location::parse_uri(raw);
    assert_eq!(parsed.kind(), LocationKind::Unsupported);
    assert_eq!(parsed.to_uri(), raw);
    assert!(!parsed.is_listable());
    assert_eq!(parsed.as_local_path(), None);
}

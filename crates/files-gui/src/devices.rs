//! External devices in the sidebar: their states, and what happens to the
//! window when one is plugged in, opened, or pulled out.
//!
//! Ticket 34 drew device rows from the mount table, which is what a session can
//! honestly report on its own, and every one of them read as
//! `DeviceStateKind::Unknown`. This is the other half: a link to the storage
//! layer that produces the real five states, and the window behaviour they
//! imply.
//!
//! **Where the states come from.** [`DeviceLink`] is reached over
//! `org.betteros.Storage1` when the session service is running, and when it is
//! not, the same `storage-core` state machine runs in this process over
//! `storage-platform` events — the pattern `monitor-gui` established, including
//! its rule that the window says so. [`CollectionMode::InProcess`] is drawn as
//! a note, because a file manager collecting device state in its own process
//! stops collecting when it is closed, and the user should know that before
//! they trust a green light.
//!
//! **Unknown is never softened.** A device with no link behind it, or one the
//! link has not observed, is [`DeviceStateKind::Unknown`], which reads as
//! "Removal status cannot be verified" and never as "Ready to unplug". Issue #5
//! is explicit that a green indicator needs evidence, and the absence of a
//! service is the absence of evidence.
//!
//! **Idle stays quiet.** [`DeviceRow::is_warning`] is true for exactly two
//! states. The other three draw as ordinary rows, because Issue #5 asks for the
//! sidebar not to become a permanent warning console.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use files_core::{LocalPath, Location};
use storage_core::{DeviceStateKind, RemovalPolicy};
use storage_service::protocol::{DeviceReport, StateReport};

use crate::i18n::Copy;

/// Where the window's device states are coming from.
///
/// The same four states `monitor-gui` models, for the same reason: a window
/// that is collecting in its own process, and one that is reading a service,
/// are different promises and only one of them survives the window closing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollectionMode {
    /// The link has not answered yet.
    Connecting,
    /// Reading `org.betteros.Storage1`.
    Service,
    /// No service, so the state machine is running here. The detail is the
    /// reason the service was not reachable.
    InProcess { detail: String },
    /// Neither worked. No device state is available at all, and the rows say
    /// so rather than showing an idle-looking Unknown.
    Unavailable { detail: String },
}

impl CollectionMode {
    /// The note the window shows, and whether it is a warning.
    ///
    /// `None` for the service case: a working service is the expected state and
    /// deserves no banner.
    pub fn note(&self, c: &'static Copy) -> Option<(&'static str, bool)> {
        match self {
            CollectionMode::Service => None,
            CollectionMode::Connecting => Some((c.devices_connecting, false)),
            CollectionMode::InProcess { .. } => Some((c.devices_in_process, true)),
            CollectionMode::Unavailable { .. } => Some((c.devices_unavailable, true)),
        }
    }

    /// The reason string, for a diagnostics view. Never shown as the note
    /// itself: it is a D-Bus error message, not a sentence for a person.
    pub fn detail(&self) -> Option<&str> {
        match self {
            CollectionMode::InProcess { detail } | CollectionMode::Unavailable { detail } => {
                Some(detail)
            }
            _ => None,
        }
    }

    /// Whether any device state can be believed at all.
    pub fn has_states(&self) -> bool {
        matches!(
            self,
            CollectionMode::Service | CollectionMode::InProcess { .. }
        )
    }
}

/// One device, as the sidebar draws it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceRow {
    /// The UDisks2 object path. The identifier every request uses, and never
    /// shown to the user.
    pub object_path: String,
    pub label: String,
    /// The identity key the storage layer files this device under. A string
    /// here rather than a `storage_core::IdentityKey`, because that type is
    /// built from evidence and has no constructor from a name — reconstructing
    /// one from the wire would be claiming evidence this side never saw.
    pub identity: String,
    /// True when the identity only holds for this connection, which is what
    /// `IdentityConfidence::Volatile` means. A weak identity is still
    /// persistable and is not flagged.
    pub identity_volatile: bool,
    pub mount_point: Option<PathBuf>,
    pub policy: RemovalPolicy,
    pub state: DeviceStateKind,
    /// The applications holding the device, when the scan identified any. Empty
    /// for every state but Busy, and possibly empty for Busy too — a blocker
    /// the scan could not name is still a blocker.
    pub blockers: Vec<String>,
    /// Set when this device was pulled out during a write. Survives the row
    /// being removed, so the warning outlives the device.
    pub unsafe_removal: Option<UnsafeRemoval>,
}

/// What was lost, or might have been.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsafeRemoval {
    pub previous_state: String,
    pub unfinished_operations: Vec<String>,
    pub recommend_filesystem_check: bool,
}

impl DeviceRow {
    /// Whether clicking this row can navigate straight in, or has to mount
    /// first.
    pub fn is_mounted(&self) -> bool {
        self.mount_point.is_some()
    }

    /// The location this row opens, once it is mounted.
    pub fn location(&self) -> Option<Location> {
        let mount_point = self.mount_point.as_ref()?;
        LocalPath::new(mount_point.clone())
            .ok()
            .map(Location::Local)
    }

    /// Whether the row is drawn as a warning.
    ///
    /// Two of the five, deliberately. Writing is a warning because unplugging
    /// now loses data. Performance mode is a warning because direct removal is
    /// not promised and the user has to eject. Busy and Unknown are stated, not
    /// shouted, and Ready is quiet — which is Issue #5's "the normal idle state
    /// should remain visually quiet".
    pub fn is_warning(&self) -> bool {
        matches!(
            self.state,
            DeviceStateKind::Writing | DeviceStateKind::PerformanceMode
        )
    }

    /// Whether Better Files may say this device can be unplugged.
    pub fn permits_direct_removal(&self) -> bool {
        self.state.permits_direct_removal()
    }

    /// The sentence under the device name.
    pub fn state_label(&self, c: &'static Copy) -> String {
        state_label(self.state, &self.blockers, c)
    }
}

/// The five user-visible states, in Issue #5's own words.
///
/// The strings live in `i18n` so both languages are checked by the compiler,
/// and the Busy one is a template because it names an application. A Busy
/// device whose blocker the scan could not identify gets the unidentified
/// wording rather than "In use by " with nothing after it.
pub fn state_label(state: DeviceStateKind, blockers: &[String], c: &'static Copy) -> String {
    match state {
        DeviceStateKind::ReadyToUnplug => c.device_state_ready.to_string(),
        DeviceStateKind::Writing => c.device_state_writing.to_string(),
        DeviceStateKind::Busy => match blockers.first() {
            Some(application) => c.device_state_busy.replace("{app}", application),
            None => c.device_state_busy_unidentified.to_string(),
        },
        DeviceStateKind::PerformanceMode => c.device_state_performance.to_string(),
        // Disconnected is not one of the five: the row is gone. Reaching here
        // means a report arrived for a device that has already left, and the
        // honest answer is the same one an unverifiable state gets.
        DeviceStateKind::Unknown | DeviceStateKind::Disconnected => {
            c.device_state_unknown.to_string()
        }
    }
}

/// What a link tells the window about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceNotice {
    /// The whole inventory, as of now. Replaces what the window held.
    Inventory(Vec<DeviceReport>),
    Mode(CollectionMode),
    Mounted {
        object_path: String,
        mount_point: PathBuf,
    },
    MountFailed {
        object_path: String,
        detail: String,
    },
    /// An eject finished. `powered_off` false with `unmounted` true is an
    /// unmount that worked and a power-off that was unavailable, which is not a
    /// clean eject and is not reported as one.
    Ejected {
        object_path: String,
        unmounted: bool,
        powered_off: bool,
    },
    EjectFailed {
        object_path: String,
        detail: String,
    },
    /// The device is gone. `unsafe_removal` is set when it went while a write
    /// was outstanding.
    Disconnected {
        object_path: String,
        unsafe_removal: Option<UnsafeRemoval>,
    },
}

/// The window's connection to the storage layer.
///
/// Deliberately synchronous and non-blocking from the caller's side: a request
/// is posted and the answer arrives through [`DeviceLink::poll`] on a later
/// frame. Everything async lives behind the implementation, so the session —
/// and every test of it — is ordinary straight-line code.
pub trait DeviceLink: Send + Sync {
    fn mode(&self) -> CollectionMode;
    /// Asks for a mount. The answer is a `Mounted` or `MountFailed` notice.
    fn request_mount(&self, object_path: &str);
    fn request_eject(&self, object_path: &str);
    /// Asks for a fresh inventory.
    fn request_refresh(&self);
    /// Takes whatever has arrived. Never blocks.
    fn poll(&self) -> Vec<DeviceNotice>;
}

/// A link with nothing behind it.
///
/// Not a stand-in for a broken one: this is what a build with no storage layer
/// honestly has, and it reports `Unavailable` so no row claims a state.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDeviceLink;

impl DeviceLink for NoDeviceLink {
    fn mode(&self) -> CollectionMode {
        CollectionMode::Unavailable {
            detail: "no storage link in this build".to_string(),
        }
    }
    fn request_mount(&self, _object_path: &str) {}
    fn request_eject(&self, _object_path: &str) {}
    fn request_refresh(&self) {}
    fn poll(&self) -> Vec<DeviceNotice> {
        Vec::new()
    }
}

/// The device rows the window is currently drawing.
///
/// Holds the rows in the order the link reported them and the mode they came
/// from. Applying a notice is the only way it changes, so there is one place
/// that decides what a disconnect does to the list.
#[derive(Clone, Debug, Default)]
pub struct DeviceInventory {
    rows: Vec<DeviceRow>,
    /// Unsafe-removal records for devices that are no longer here. Kept until
    /// the user dismisses them: a warning about data that may not have been
    /// written must not disappear with the row that caused it.
    warnings: HashMap<String, UnsafeRemoval>,
}

impl DeviceInventory {
    pub fn rows(&self) -> &[DeviceRow] {
        &self.rows
    }

    pub fn get(&self, object_path: &str) -> Option<&DeviceRow> {
        self.rows.iter().find(|row| row.object_path == object_path)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Outstanding unsafe-removal warnings, newest last.
    pub fn warnings(&self) -> Vec<(&str, &UnsafeRemoval)> {
        self.rows
            .iter()
            .filter_map(|row| {
                row.unsafe_removal
                    .as_ref()
                    .map(|record| (row.object_path.as_str(), record))
            })
            .chain(
                self.warnings
                    .iter()
                    .map(|(path, record)| (path.as_str(), record)),
            )
            .collect()
    }

    pub fn dismiss_warning(&mut self, object_path: &str) {
        self.warnings.remove(object_path);
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.object_path == object_path)
        {
            row.unsafe_removal = None;
        }
    }

    /// Replaces the rows from a full inventory.
    pub fn apply_inventory(&mut self, reports: Vec<DeviceReport>) {
        self.rows = reports.into_iter().map(row_from).collect();
    }

    /// Removes a device and returns the mount point it had, so the caller knows
    /// which navigation state is now stale.
    pub fn remove(
        &mut self,
        object_path: &str,
        unsafe_removal: Option<UnsafeRemoval>,
    ) -> Option<PathBuf> {
        let index = self
            .rows
            .iter()
            .position(|row| row.object_path == object_path)?;
        let row = self.rows.remove(index);
        if let Some(record) = unsafe_removal.or(row.unsafe_removal) {
            self.warnings.insert(object_path.to_string(), record);
        }
        row.mount_point
    }

    pub fn set_mount_point(&mut self, object_path: &str, mount_point: PathBuf) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.object_path == object_path)
        {
            row.mount_point = Some(mount_point);
        }
    }

    pub fn clear_mount_point(&mut self, object_path: &str) -> Option<PathBuf> {
        self.rows
            .iter_mut()
            .find(|row| row.object_path == object_path)
            .and_then(|row| row.mount_point.take())
    }
}

/// Turns one wire report into a row.
pub fn row_from(report: DeviceReport) -> DeviceRow {
    let state = report.state.kind();
    let blockers = blockers_of(&report.state);
    let unsafe_removal = match &report.state {
        StateReport::Disconnected {
            unsafe_removal: Some(record),
        } => Some(UnsafeRemoval {
            previous_state: record.previous_state.clone(),
            unfinished_operations: record.unfinished_operations.clone(),
            recommend_filesystem_check: record.recommend_filesystem_check,
        }),
        _ => None,
    };
    DeviceRow {
        label: display_label(&report),
        object_path: report.object_path,
        identity_volatile: report.identity_confidence == "volatile",
        identity: report.identity,
        mount_point: report.mount_point.map(PathBuf::from),
        policy: report.policy,
        state,
        blockers,
        unsafe_removal,
    }
}

/// The applications a Busy state named.
fn blockers_of(state: &StateReport) -> Vec<String> {
    match state {
        StateReport::Busy { blockers } => blockers
            .iter()
            .filter_map(|blocker| match blocker {
                storage_service::protocol::BlockerReport::Process { name, pid } => {
                    Some(name.clone().unwrap_or_else(|| format!("PID {pid}")))
                }
                // An unidentified blocker is real and unnameable. It is not
                // turned into a name here; the label falls back to the
                // unidentified wording instead.
                storage_service::protocol::BlockerReport::Unidentified { .. } => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The name a device row shows.
///
/// The mount point's own last component is what the user recognizes — a disk at
/// `/media/tim/PHOTOS` is "PHOTOS" — then the service's display name. Neither is
/// a UDisks2 object path or a `/dev` node.
fn display_label(report: &DeviceReport) -> String {
    report
        .mount_point
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| report.display_name.clone())
}

/// Whether a location is on a mount point that has gone away.
///
/// Used for both halves of the cleanup Issue #6 requires: moving a pane that is
/// standing on the device, and forgetting the history entries that point into
/// it.
pub fn is_under(location: &Location, mount_point: &Path) -> bool {
    location
        .as_local_path()
        .is_some_and(|path| path.as_path().starts_with(mount_point))
}

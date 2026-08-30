//! The host side of Better Launcher.
//!
//! `launcher-core` decides what the overlay shows. This crate decides how the
//! overlay reaches the machine: how an application is started, how the visible
//! library learns that an application was installed, what the current session
//! can actually offer, how a second launch reaches the first one, and where a
//! gesture would attach if one existed.
//!
//! Four rules shape everything here.
//!
//! - **Nothing is discovered or launched twice.** Discovery, watching, and
//!   process spawning belong to `app-catalog-platform`, and this crate calls
//!   into it. There is no second `.desktop` scanner and no second launch path;
//!   [`launch::CatalogLauncher`] is a delegation, not an implementation.
//! - **A capability is detected, never assumed.** [`session`] reports what the
//!   session is, and an activation path this session cannot offer is absent
//!   from [`session::SessionCapabilities::activation_paths`] rather than
//!   present and broken. A machine with no gesture support is not an error
//!   state; it is a machine with two activation paths instead of three.
//! - **Nothing here needs privilege.** No raw input device is opened, no input
//!   is grabbed, and no system setting is written. The global keyboard
//!   shortcut is expressed as the GNOME keys that would carry it
//!   ([`shortcut`]), and applying them is Better Defaults' job, over its own
//!   reviewed boundary.
//! - **The gesture boundary is typed and empty.** [`gesture`] carries the
//!   event shape, the threshold and cooldown policy, and a recognizer that is
//!   tested against replayed samples. The only adapter in this build is a mock
//!   one. Which adapter ships is
//!   [ADR 0008](../../../docs/decisions/0008-launcher-gesture-integration.md).
//!
//! There is no GPUI dependency here, so all of it is tested with no display
//! backend.

pub mod activation;
#[cfg(feature = "session-bus")]
pub mod bus;
pub mod catalog;
pub mod gesture;
pub mod launch;
pub mod session;
pub mod shortcut;

use thiserror::Error;

pub use activation::{
    ActivationPath, ActivationRequest, FakeNameRegistry, InstanceRole, NameOwnership, NameRegistry,
    OverlayCommand, OverlayVisibility, SingleInstance,
};
pub use catalog::{LauncherSnapshot, MetadataWatch, load_snapshot};
pub use gesture::{
    AdapterDescription, GestureAdapter, GestureDirection, GestureOutcome, GesturePhase,
    GestureRecognizer, GestureSample, GestureThresholds, MockGestureAdapter,
};
pub use launch::{ApplicationStarter, CatalogLauncher, RecordingStarter};
pub use session::{SessionCapabilities, SessionType, ShellKind, ShortcutAvailability};
pub use shortcut::GnomeCustomKeybinding;

/// Host-side failures. Every variant renders as a stable machine key, so the
/// overlay can word a failure in the user's language without parsing prose.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PlatformError {
    #[error("{0}")]
    Catalog(#[from] app_catalog_platform::PlatformError),
    /// The overlay asked to start something the catalog no longer contains.
    /// This is reachable: an application can be removed between the frame that
    /// drew it and the click that selected it.
    #[error("launcher.platform.error.unknown_application:{0}")]
    UnknownApplication(String),
    /// A second launch could not hand its request to the running instance.
    /// Reported rather than resolved by opening a second window, because two
    /// overlays is a worse outcome than one clear failure.
    #[error("launcher.platform.error.activation_failed:{0}")]
    ActivationFailed(String),
}

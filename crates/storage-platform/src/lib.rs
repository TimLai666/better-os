//! Linux storage integration: UDisks2, flushing, and the two inspection
//! signals.
//!
//! This crate knows about D-Bus, `/proc`, and `ioctl`. It knows nothing about
//! whether a device is safe to unplug — that decision lives in `storage-core`,
//! which is given what this crate observes and decides on its own.
//!
//! Everything the host does is behind a trait in [`traits`], and every trait has
//! a fake behind the `test-support` feature, so the coordination logic is
//! testable with no hardware. The UDisks2 implementation compiles against real
//! interface definitions, and `better-storage-doctor` runs it against the live
//! system when someone wants to know what this machine actually exposes.

#[cfg(feature = "test-support")]
pub mod fake;
pub mod flush;
pub mod model;
pub mod openuse;
pub mod probe;
pub mod roots;
pub mod traits;
pub mod udisks;
pub mod writeback;

pub use flush::LinuxFlush;
pub use model::{BlockInfo, DeviceAddress, DeviceClass, DriveInfo, PlatformDevice, PlatformEvent};
pub use openuse::ProcOpenUse;
pub use probe::{DoctorReport, probe};
pub use roots::Roots;
pub use traits::{
    DeviceControl, EjectOutcome, FlushBackend, FlushReport, OpenUseInspector, PlatformError,
    WritebackInspector,
};
pub use udisks::UDisks2;
pub use writeback::LinuxWriteback;

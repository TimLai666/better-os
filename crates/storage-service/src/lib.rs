//! The user-session storage service.
//!
//! It owns device state for the whole session: one `storage-core` state machine
//! per connected external volume, fed by `storage-platform`, published to
//! clients over the session bus. Better Files is a client, not the owner —
//! closing it does not end the service, and other applications writing to the
//! same device are observed through the platform signals rather than through
//! the file manager.
//!
//! Nothing here needs privilege. Mounting goes through UDisks2, which holds its
//! own; the signals are read from `/proc`; the flush is `syncfs` on a mount the
//! session already owns. `docs/storage-safety-signals.md` records the two
//! places where privilege would buy a better answer, and issue #5 defers that
//! boundary to an ADR rather than to this code.

pub mod coordinator;
pub mod protocol;
pub mod service;
pub mod store;

pub use coordinator::{Clock, ServiceError, StorageCoordinator};
pub use protocol::{
    DeviceListDocument, DeviceReport, EjectReport, OperationNotice, PROTOCOL_VERSION,
    ProtocolError, SetPolicyRequest, StateReport,
};
pub use service::{BUS_NAME, OBJECT_PATH, StorageService};
pub use store::{PreferenceStore, StoreError};

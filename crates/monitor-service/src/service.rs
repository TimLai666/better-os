//! The session D-Bus surface.
//!
//! Bus name `org.betteros.Monitor1`, object path `/org/betteros/Monitor1`, on
//! the *session* bus — one Better Monitor service per logged-in user,
//! recording that user's machine and answering that user's clients. Requests
//! and replies cross as JSON documents, the shape ADR 0007 chose for the
//! privileged daemon and `awake-ipc` chose for Better Awake, so every local
//! protocol in this workspace is read the same way.
//!
//! Nothing here is authorized. The caller is already the same user as the
//! service, and asking polkit whether a user may look at their own machine
//! would be theatre. The service also holds no privilege to lend: it reads
//! `/proc` and `/sys` unprivileged and writes to that user's own state
//! directory.

use std::sync::Arc;

use monitor_ipc::MAX_REQUEST_BYTES;
use zbus::interface;

use crate::engine::MonitorEngine;

pub use monitor_ipc::{BUS_NAME, INTERFACE_NAME, OBJECT_PATH};

pub struct MonitorDbusService {
    engine: Arc<MonitorEngine>,
}

impl MonitorDbusService {
    pub fn new(engine: Arc<MonitorEngine>) -> Self {
        Self { engine }
    }
}

#[interface(name = "org.betteros.Monitor1")]
impl MonitorDbusService {
    /// One request in, one reply out.
    ///
    /// A malformed document comes back as a typed rejection rather than a bus
    /// error, because "your request was wrong" is an answer a window has to be
    /// able to show, not a transport failure it has to guess about.
    async fn request(&self, request_json: &str) -> String {
        if request_json.len() > MAX_REQUEST_BYTES {
            return monitor_ipc::MonitorResponse::rejected(
                monitor_ipc::IpcError::PayloadTooLarge {
                    bytes: request_json.len(),
                    limit: MAX_REQUEST_BYTES,
                }
                .to_string(),
            )
            .to_json()
            .unwrap_or_default();
        }
        self.engine.handle_document(request_json).await
    }

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        monitor_ipc::PROTOCOL_VERSION
    }

    /// Raw collector rounds since the service started.
    ///
    /// A property rather than a request, so a client can watch collection
    /// continue without building a document for it.
    #[zbus(property)]
    async fn rounds_collected(&self) -> u64 {
        self.engine.rounds()
    }
}

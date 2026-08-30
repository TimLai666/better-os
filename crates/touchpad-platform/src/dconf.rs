//! Writing to dconf the way dconf itself does it.
//!
//! ADR 0009 recorded three options for changing a dconf key and chose the third
//! — read and verify, report manual action required for a change — because the
//! second needed a GVariant change set, a D-Bus client, and a live session bus
//! to test against. This is that second option, built.
//!
//! Editing `~/.config/dconf/user` directly is still wrong, and nothing here
//! does it. The dconf service owns that file, caches it, and rewrites it; a
//! change written behind the service is ignored by the running session and
//! overwritten by the next write the service makes. Going through
//! `ca.desrt.dconf.Writer.Change` is what makes a change real: the service
//! writes the database, updates its own cache, and emits the `Notify` signal
//! that tells every running application to re-read.
//!
//! Nothing here is privileged. The user writer at `/ca/desrt/dconf/Writer/user`
//! is the caller's own per-user database, reached over the session bus.

use tokio::runtime::Runtime;
use zbus::Connection;

use crate::PlatformError;
use crate::gvariant::Changeset;

const SERVICE: &str = "ca.desrt.dconf";
const INTERFACE: &str = "ca.desrt.dconf.Writer";
/// The caller's own per-user database. There is also a system writer; Better
/// Touchpad has no business with it and never names it.
pub const USER_WRITER_PATH: &str = "/ca/desrt/dconf/Writer/user";

pub struct DconfWriter {
    runtime: Runtime,
    connection: Connection,
    object_path: String,
}

impl DconfWriter {
    /// Connects to the session bus this process was started in.
    pub fn connect() -> Result<Self, PlatformError> {
        Self::build(None, USER_WRITER_PATH)
    }

    /// Connects to an explicit bus address and writer path. Only a test uses
    /// this, and only so it never touches the developer's own session.
    pub fn connect_to(address: &str, object_path: &str) -> Result<Self, PlatformError> {
        Self::build(Some(address), object_path)
    }

    fn build(address: Option<&str>, object_path: &str) -> Result<Self, PlatformError> {
        // One worker: this runtime exists to dispatch a handful of method
        // calls, not to do work. Same shape as `launcher-platform`'s bus.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|error| PlatformError::NoSessionBus(error.to_string()))?;
        let connection = runtime.block_on(async {
            match address {
                Some(address) => {
                    let address: zbus::Address =
                        address.parse().map_err(|error: zbus::Error| {
                            PlatformError::NoSessionBus(error.to_string())
                        })?;
                    zbus::connection::Builder::address(address)
                        .map_err(|error| PlatformError::NoSessionBus(error.to_string()))?
                        .build()
                        .await
                        .map_err(|error| PlatformError::NoSessionBus(error.to_string()))
                }
                None => Connection::session()
                    .await
                    .map_err(|error| PlatformError::NoSessionBus(error.to_string())),
            }
        })?;
        Ok(Self {
            runtime,
            connection,
            object_path: object_path.to_string(),
        })
    }

    /// Whether the service is there and answering, without changing anything.
    ///
    /// This is what makes the difference between "the control centre can apply
    /// settings" and "it cannot", and it is asked before any control is
    /// offered rather than after a user has moved a slider.
    pub fn probe(&self) -> Result<(), PlatformError> {
        self.runtime.block_on(async {
            self.connection
                .call_method(
                    Some(SERVICE),
                    self.object_path.as_str(),
                    Some("org.freedesktop.DBus.Peer"),
                    "Ping",
                    &(),
                )
                .await
                .map(|_| ())
                .map_err(|error| PlatformError::CallFailed(error.to_string()))
        })
    }

    /// Sends one change set and returns the tag the service assigned it.
    ///
    /// An empty change set is not sent. The service would accept it, but a
    /// call that writes nothing is a notification for no reason.
    pub fn change(&self, changeset: &Changeset) -> Result<String, PlatformError> {
        if changeset.is_empty() {
            return Ok(String::new());
        }
        let blob = changeset.serialise();
        self.runtime.block_on(async {
            let reply = self
                .connection
                .call_method(
                    Some(SERVICE),
                    self.object_path.as_str(),
                    Some(INTERFACE),
                    "Change",
                    &(blob,),
                )
                .await
                .map_err(|error| PlatformError::CallFailed(error.to_string()))?;
            reply
                .body()
                .deserialize::<String>()
                .map_err(|error| PlatformError::CallFailed(error.to_string()))
        })
    }
}

impl std::fmt::Debug for DconfWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DconfWriter")
            .field("object_path", &self.object_path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreachable_bus_is_reported_rather_than_retried_forever() {
        let error = DconfWriter::connect_to(
            "unix:path=/nonexistent/definitely-not-a-bus",
            USER_WRITER_PATH,
        )
        .expect_err("there is no bus at that path");
        assert!(matches!(error, PlatformError::NoSessionBus(_)));
    }

    #[test]
    fn an_address_that_is_not_an_address_is_refused_before_any_connection() {
        assert!(matches!(
            DconfWriter::connect_to("this is not a bus address", USER_WRITER_PATH),
            Err(PlatformError::NoSessionBus(_))
        ));
    }
}

//! The session-bus implementation of the single-instance rule.
//!
//! This is the transport under [`crate::activation`] and it holds none of the
//! policy. It does three things: ask the bus for the well-known name, serve
//! `org.freedesktop.Application` when it gets it, and call that interface on
//! the running instance when it does not.
//!
//! `org.freedesktop.Application` rather than an interface of our own, because
//! it is the interface a desktop entry marked `DBusActivatable` is activated
//! through. Using it means a panel, a dock, and `gio launch` reach the running
//! overlay by the same route a second `better-launcher` process does.
//!
//! The two verbs are kept apart deliberately. `Activate` is what clicking a
//! launcher icon sends, and it opens; `ActivateAction("toggle")` is what a
//! second launch from the keyboard shortcut sends, and it toggles. Collapsing
//! them would make clicking the icon of an open launcher close it.
//!
//! zbus is used in its tokio flavor, which is the one flavor this workspace
//! has, so the runtime here is the same shape as `awake-gui`'s: owned by this
//! process, small, and outliving nothing.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::{OwnedValue, Value};

use crate::PlatformError;
use crate::activation::{ActivationRequest, NameOwnership, NameRegistry, SingleInstance};

/// The interface every activatable desktop application implements.
pub const APPLICATION_INTERFACE: &str = "org.freedesktop.Application";

/// The action name a second launch uses to ask for a toggle.
pub const TOGGLE_ACTION: &str = "toggle";
/// The action name used to ask an open overlay to close.
pub const CLOSE_ACTION: &str = "close";

fn failed(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::ActivationFailed(error.to_string())
}

/// The served side: turns bus calls into [`ActivationRequest`]s.
struct ActivationInterface {
    sender: UnboundedSender<ActivationRequest>,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl ActivationInterface {
    /// A launcher icon, a dock, or `gio launch`. Opens; never closes.
    async fn activate(&self, _platform_data: HashMap<String, OwnedValue>) {
        let _ = self.sender.send(ActivationRequest::Open);
    }

    /// The launcher opens applications, not files. A URI list is accepted
    /// because the interface requires the method to exist, and ignored,
    /// because opening a file is the file manager's job.
    async fn open(&self, _uris: Vec<String>, _platform_data: HashMap<String, OwnedValue>) {
        let _ = self.sender.send(ActivationRequest::Open);
    }

    async fn activate_action(
        &self,
        action_name: String,
        _parameter: Vec<OwnedValue>,
        _platform_data: HashMap<String, OwnedValue>,
    ) {
        let request = match action_name.as_str() {
            TOGGLE_ACTION => ActivationRequest::Toggle,
            CLOSE_ACTION => ActivationRequest::Close,
            // An unknown action opens rather than doing nothing: something
            // asked for the launcher, and showing it is the answer that
            // cannot be wrong.
            _ => ActivationRequest::Open,
        };
        let _ = self.sender.send(request);
    }
}

/// A connection to the session bus, used as the registry behind
/// [`SingleInstance`].
pub struct SessionBusRegistry {
    runtime: Runtime,
    connection: Connection,
    object_path: String,
    inbox: Mutex<Option<UnboundedReceiver<ActivationRequest>>>,
}

impl SessionBusRegistry {
    /// Connects to the session bus this process was started in.
    pub fn connect() -> Result<Self, PlatformError> {
        Self::build(None)
    }

    /// Connects to an explicit bus address. Only a test uses this, and only so
    /// it never touches the developer's own session bus.
    pub fn connect_to(address: &str) -> Result<Self, PlatformError> {
        Self::build(Some(address))
    }

    fn build(address: Option<&str>) -> Result<Self, PlatformError> {
        // One worker: this runtime exists to dispatch a handful of method
        // calls, not to do work.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(failed)?;
        let connection = runtime.block_on(async {
            match address {
                Some(address) => {
                    let address: zbus::Address = address.parse().map_err(failed)?;
                    zbus::connection::Builder::address(address)
                        .map_err(failed)?
                        .build()
                        .await
                        .map_err(failed)
                }
                None => Connection::session().await.map_err(failed),
            }
        })?;
        Ok(Self {
            runtime,
            connection,
            object_path: SingleInstance::OBJECT_PATH.to_string(),
            inbox: Mutex::new(None),
        })
    }

    /// The requests the running instance has received. `None` once it has been
    /// taken, and before the name was acquired.
    pub fn take_inbox(&self) -> Option<UnboundedReceiver<ActivationRequest>> {
        self.inbox.lock().expect("inbox lock").take()
    }

    /// The connection, so the caller can keep it alive for as long as the
    /// overlay is running. Dropping it un-owns the name.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

impl NameRegistry for SessionBusRegistry {
    fn request_name(&self, name: &str) -> Result<NameOwnership, PlatformError> {
        let path = self.object_path.clone();
        let (sender, receiver) = unbounded_channel();
        let acquired = self.runtime.block_on(async {
            // DoNotQueue: a launcher that silently became the owner later,
            // after the first instance quit, would be a process that looked
            // dead and then started answering. With that flag, zbus reports a
            // taken name as `Error::NameTaken` rather than as a reply, so
            // "someone else has it" arrives on both paths and both mean the
            // same thing here.
            let acquired = match self
                .connection
                .request_name_with_flags(name, RequestNameFlags::DoNotQueue.into())
                .await
            {
                Ok(reply) => matches!(
                    reply,
                    RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
                ),
                Err(zbus::Error::NameTaken) => false,
                Err(error) => return Err(failed(error)),
            };
            if acquired {
                self.connection
                    .object_server()
                    .at(path.as_str(), ActivationInterface { sender })
                    .await
                    .map_err(failed)?;
            }
            Ok::<_, PlatformError>(acquired)
        })?;

        if acquired {
            *self.inbox.lock().expect("inbox lock") = Some(receiver);
            Ok(NameOwnership::Acquired)
        } else {
            Ok(NameOwnership::AlreadyOwned)
        }
    }

    fn forward(&self, name: &str, request: ActivationRequest) -> Result<(), PlatformError> {
        let path = self.object_path.clone();
        self.runtime.block_on(async {
            let platform_data: HashMap<&str, Value<'_>> = HashMap::new();
            let parameter: Vec<Value<'_>> = Vec::new();
            match request {
                ActivationRequest::Open => self
                    .connection
                    .call_method(
                        Some(name),
                        path.as_str(),
                        Some(APPLICATION_INTERFACE),
                        "Activate",
                        &(platform_data,),
                    )
                    .await
                    .map(|_| ())
                    .map_err(failed),
                ActivationRequest::Toggle | ActivationRequest::Close => {
                    let action = if request == ActivationRequest::Toggle {
                        TOGGLE_ACTION
                    } else {
                        CLOSE_ACTION
                    };
                    self.connection
                        .call_method(
                            Some(name),
                            path.as_str(),
                            Some(APPLICATION_INTERFACE),
                            "ActivateAction",
                            &(action, parameter, platform_data),
                        )
                        .await
                        .map(|_| ())
                        .map_err(failed)
                }
            }
        })
    }
}

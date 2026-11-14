//! The session-bus implementation of the single-instance rule.
//!
//! This is the transport under [`crate::activation`] and it holds none of the
//! policy. It does three things: serve `org.freedesktop.Application`, ask the
//! bus for the well-known name, and call that interface on the running
//! instance when it does not get it. The interface is served through the
//! connection builder, so zbus has subscribed the object server to method
//! calls before the name is requested and a click that arrives with the name
//! is answered rather than dropped.
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
    /// The channel's receiving end, before the name is acquired. The interface
    /// is served at connection time, so the channel exists before the name
    /// does; [`SessionBusRegistry::take_inbox`] must still report `None` until
    /// this process is the owner. Until then the only route in is this
    /// connection's unique name, which nobody is told, and a process that does
    /// not get the name forwards and exits; so nothing accumulates here.
    /// `None` for a forward-only registry, which serves nothing.
    pending_inbox: Mutex<Option<UnboundedReceiver<ActivationRequest>>>,
    inbox: Mutex<Option<UnboundedReceiver<ActivationRequest>>>,
    /// Whether `org.freedesktop.Application` is served, so the name may be
    /// owned.
    serves: bool,
}

impl SessionBusRegistry {
    /// Connects to the session bus this process was started in, ready to own
    /// the launcher's name: `org.freedesktop.Application` is served from the
    /// start.
    pub fn connect() -> Result<Self, PlatformError> {
        Self::build(None, true)
    }

    /// Connects to an explicit bus address. Only a test uses this, and only so
    /// it never touches the developer's own session bus.
    pub fn connect_to(address: &str) -> Result<Self, PlatformError> {
        Self::build(Some(address), true)
    }

    /// Connects for a process that only ever forwards to the launcher, such as
    /// the gesture service: it serves no `org.freedesktop.Application` of its
    /// own, so nothing on the bus mistakes it for a launcher, and it cannot
    /// take the launcher's name ([`NameRegistry::request_name`] refuses).
    pub fn connect_forward_only() -> Result<Self, PlatformError> {
        Self::build(None, false)
    }

    /// [`SessionBusRegistry::connect_forward_only`] on an explicit bus
    /// address, for a test.
    pub fn connect_forward_only_to(address: &str) -> Result<Self, PlatformError> {
        Self::build(Some(address), false)
    }

    fn build(address: Option<&str>, serves: bool) -> Result<Self, PlatformError> {
        // One worker: this runtime exists to dispatch a handful of method
        // calls, not to do work.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(failed)?;
        let object_path = SingleInstance::OBJECT_PATH.to_string();
        let (sender, receiver) = unbounded_channel();
        let connection = runtime.block_on(async {
            let builder = match address {
                Some(address) => {
                    let address: zbus::Address = address.parse().map_err(failed)?;
                    zbus::connection::Builder::address(address).map_err(failed)?
                }
                None => zbus::connection::Builder::session().map_err(failed)?,
            };
            if !serves {
                return builder.build().await.map_err(failed);
            }
            // Served here rather than when the name is acquired:
            // `Builder::build` waits until the object server has subscribed to
            // method calls, which is what makes a call that arrives with the
            // name answerable. Serving after `request_name` would leave a gap
            // in which the caller waits for its timeout.
            builder
                .serve_at(object_path.as_str(), ActivationInterface { sender })
                .map_err(failed)?
                .build()
                .await
                .map_err(failed)
        })?;
        Ok(Self {
            runtime,
            connection,
            object_path,
            pending_inbox: Mutex::new(serves.then_some(receiver)),
            serves,
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
        if !self.serves {
            // Owning the name with nothing served behind it would answer
            // every click with an unknown-object error.
            return Err(PlatformError::ActivationFailed(format!(
                "a forward-only registry cannot own {name}"
            )));
        }
        let acquired = self.runtime.block_on(async {
            // DoNotQueue: a launcher that silently became the owner later,
            // after the first instance quit, would be a process that looked
            // dead and then started answering. With that flag, zbus reports a
            // taken name as `Error::NameTaken` rather than as a reply, so
            // "someone else has it" arrives on both paths and both mean the
            // same thing here.
            match self
                .connection
                .request_name_with_flags(name, RequestNameFlags::DoNotQueue.into())
                .await
            {
                Ok(reply) => Ok(matches!(
                    reply,
                    RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner
                )),
                Err(zbus::Error::NameTaken) => Ok(false),
                Err(error) => Err(failed(error)),
            }
        })?;

        if acquired {
            // Only now does this process own the name, so only now is there an
            // inbox to hand out. The interface itself was served when the
            // connection was built.
            if let Some(receiver) = self
                .pending_inbox
                .lock()
                .expect("pending inbox lock")
                .take()
            {
                *self.inbox.lock().expect("inbox lock") = Some(receiver);
            }
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

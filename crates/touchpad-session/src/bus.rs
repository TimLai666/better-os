//! The session-bus transport under the GNOME Shell adapter.
//!
//! This file is the only place in Better Touchpad that talks to a bus, and it
//! holds no policy whatsoever: it makes the call [`crate::gnome::ShellRequest`]
//! names, and it turns the extension's two signals into
//! [`crate::gnome::ShellGestureEvent`] values without interpreting either of
//! them. Every decision about what an event means is made above it, in Rust
//! that never sees a bus.
//!
//! zbus is used in its tokio flavor, the one flavor this workspace has, and the
//! runtime is owned here and shared between the calls and the signal stream —
//! the same shape `launcher-platform`'s `SessionBusRegistry` uses, and for the
//! same reason: one small runtime, owned by the process, outliving nothing.
//!
//! The blocking `recv` is deliberate. The pipeline that consumes these events
//! is synchronous — it is a recognizer, a configuration, and an adapter — and
//! making it async would spread a runtime through code that has nothing to
//! await.

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::runtime::Runtime;
use zbus::Connection;

use crate::gnome::{
    BUS_NAME, INTERFACE_NAME, InputDeviceNode, OBJECT_PATH, SERVICE_BUS_NAME, SERVICE_INTERFACE,
    SERVICE_OBJECT_PATH, ServiceStatus, SessionEvent, ShellBridge, ShellCapabilities, ShellError,
    ShellEvents, ShellGestureEvent, ShellRequest,
};

fn unreachable(error: impl std::fmt::Display) -> ShellError {
    ShellError::Unreachable(error.to_string())
}

fn failed(error: impl std::fmt::Display) -> ShellError {
    ShellError::CallFailed(error.to_string())
}

/// A connection to the extension over the session bus.
pub struct SessionBusShell {
    runtime: Arc<Runtime>,
    /// An `Option` only so that [`Drop`] can put it down inside the runtime.
    /// A zbus connection needs the reactor to close itself, and dropping one
    /// with no runtime entered aborts the process — which is a poor way for a
    /// session service to end.
    connection: Option<Connection>,
    destination: String,
}

impl Drop for SessionBusShell {
    fn drop(&mut self) {
        let _entered = self.runtime.enter();
        drop(self.connection.take());
    }
}

impl SessionBusShell {
    /// Connects to the session bus this process was started in.
    pub fn connect() -> Result<Self, ShellError> {
        Self::build(None, BUS_NAME)
    }

    /// Connects to an explicit address and destination. Only a test uses this,
    /// and only so it never touches the developer's own session bus.
    pub fn connect_to(address: &str, destination: &str) -> Result<Self, ShellError> {
        Self::build(Some(address), destination)
    }

    fn build(address: Option<&str>, destination: &str) -> Result<Self, ShellError> {
        // One worker: this runtime dispatches a handful of method calls and
        // one signal stream, not work.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(unreachable)?;
        let connection = runtime.block_on(async {
            match address {
                Some(address) => {
                    let address: zbus::Address = address.parse().map_err(unreachable)?;
                    zbus::connection::Builder::address(address)
                        .map_err(unreachable)?
                        .build()
                        .await
                        .map_err(unreachable)
                }
                None => Connection::session().await.map_err(unreachable),
            }
        })?;
        Ok(Self {
            runtime: Arc::new(runtime),
            connection: Some(connection),
            destination: destination.to_string(),
        })
    }

    /// Whether the extension is on the bus at all.
    ///
    /// This is the fact behind the adapter-bridge health check: a name nobody
    /// owns means the extension is not installed, not enabled, or was disabled
    /// by a shell upgrade, and all three look the same from here.
    pub fn is_reachable(&self) -> bool {
        self.name_has_owner(&self.destination)
    }

    /// Whether the gesture service owns [`SERVICE_BUS_NAME`], which it does
    /// for exactly as long as it runs (ticket 131). Better Touchpad's page asks
    /// this so a dead service is never shown as connected.
    pub fn gesture_service_running(&self) -> bool {
        self.name_has_owner(SERVICE_BUS_NAME)
    }

    /// What the gesture service says about itself, or `None` when it does
    /// not answer (not running, or a build from before it could say).
    pub fn gesture_service_status(&self) -> Option<ServiceStatus> {
        let connection = self.connection();
        self.runtime.block_on(async {
            let reply = connection
                .call_method(
                    Some(SERVICE_BUS_NAME),
                    SERVICE_OBJECT_PATH,
                    Some(SERVICE_INTERFACE),
                    "Status",
                    &(),
                )
                .await
                .ok()?;
            let (watching, problem): (bool, String) = reply.body().deserialize().ok()?;
            Some(ServiceStatus { watching, problem })
        })
    }

    fn name_has_owner(&self, name: &str) -> bool {
        let connection = self.connection();
        self.runtime.block_on(async {
            let Ok(proxy) = zbus::fdo::DBusProxy::new(connection).await else {
                return false;
            };
            let Ok(name) = name.try_into() else {
                return false;
            };
            proxy.name_has_owner(name).await.unwrap_or(false)
        })
    }

    /// GNOME Shell's own `UserExtensionsEnabled`, or `None` when the shell
    /// does not answer. While it is false GNOME 46 runs no extension but its
    /// session mode's, so an adapter that is enabled in the list still never
    /// owns its name (ticket 88).
    pub fn user_extensions_enabled(&self) -> Option<bool> {
        let connection = self.connection();
        self.runtime.block_on(async {
            let proxy = zbus::fdo::PropertiesProxy::builder(connection)
                .destination("org.gnome.Shell")
                .ok()?
                .path("/org/gnome/Shell")
                .ok()?
                .build()
                .await
                .ok()?;
            let interface = "org.gnome.Shell.Extensions".try_into().ok()?;
            let value = proxy.get(interface, "UserExtensionsEnabled").await.ok()?;
            bool::try_from(value).ok()
        })
    }

    /// The extension's signals, as a blocking stream.
    pub fn events(&self) -> Result<SessionBusShellEvents, ShellError> {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(INTERFACE_NAME)
            .map_err(failed)?
            .path(OBJECT_PATH)
            .map_err(failed)?
            .build();
        let connection = self.connection().clone();
        let stream = self
            .runtime
            .block_on(
                async move { zbus::MessageStream::for_match_rule(rule, &connection, None).await },
            )
            .map_err(failed)?;
        Ok(SessionBusShellEvents {
            runtime: self.runtime.clone(),
            stream: Some(stream),
            quiet_for: None,
        })
    }

    /// The connection is only ever `None` inside [`Drop`].
    fn connection(&self) -> &Connection {
        self.connection.as_ref().expect("the connection is open")
    }

    fn call_method<B>(&self, method: &str, body: &B) -> Result<zbus::Message, ShellError>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        let connection = self.connection();
        self.runtime.block_on(async {
            connection
                .call_method(
                    Some(self.destination.as_str()),
                    OBJECT_PATH,
                    Some(INTERFACE_NAME),
                    method,
                    body,
                )
                .await
                .map_err(failed)
        })
    }
}

impl ShellBridge for SessionBusShell {
    fn call(&self, request: ShellRequest) -> Result<(), ShellError> {
        match request {
            ShellRequest::ShowOverview | ShellRequest::ShowDesktop => {
                self.call_method(request.method(), &())?;
            }
            ShellRequest::SwitchWorkspace(direction) => {
                self.call_method(request.method(), &(direction.wire(),))?;
            }
            ShellRequest::SuppressBuiltInGestures(suppress) => {
                self.call_method(request.method(), &(suppress,))?;
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> Result<ShellCapabilities, ShellError> {
        let reply = self.call_method("Capabilities", &())?;
        let document: String = reply
            .body()
            .deserialize()
            .map_err(|error| ShellError::Capabilities(error.to_string()))?;
        ShellCapabilities::from_json(&document)
    }
}

/// The extension's gesture signals, read one at a time.
pub struct SessionBusShellEvents {
    runtime: Arc<Runtime>,
    /// `Option` for the same reason the connection is: the stream holds a
    /// connection, and closing one needs the reactor.
    stream: Option<zbus::MessageStream>,
    quiet_for: Option<std::time::Duration>,
}

impl Drop for SessionBusShellEvents {
    fn drop(&mut self) {
        let _entered = self.runtime.enter();
        drop(self.stream.take());
    }
}

impl SessionBusShellEvents {
    /// Ends the stream after this long with nothing on it.
    ///
    /// The service leaves this unset: a session where nobody makes a gesture is
    /// the ordinary case, and treating an idle hour as a failure would restart
    /// the pipeline for no reason. A caller that has to finish — a test, or a
    /// one-shot check — bounds the wait with this instead of assuming an event
    /// will arrive.
    pub fn ending_after_quiet(mut self, quiet_for: std::time::Duration) -> Self {
        self.quiet_for = Some(quiet_for);
        self
    }

    fn next_message(&mut self) -> Option<zbus::Message> {
        let stream = self.stream.as_mut()?;
        match self.quiet_for {
            None => self.runtime.block_on(stream.next())?.ok(),
            Some(quiet_for) => self
                .runtime
                .block_on(async { tokio::time::timeout(quiet_for, stream.next()).await })
                .ok()??
                .ok(),
        }
    }
}

impl ShellEvents for SessionBusShellEvents {
    fn recv(&mut self) -> Option<ShellGestureEvent> {
        loop {
            let message = self.next_message()?;
            if let Some(event) = gesture_from(&message) {
                return Some(event);
            }
        }
    }
}

/// One of the extension's two gesture signals, read as it arrived, or `None`
/// for any other message.
fn gesture_from(message: &zbus::Message) -> Option<ShellGestureEvent> {
    let header = message.header();
    let member = header.member()?.to_string();
    let body = message.body();
    let (phase, fingers, first, second, at_ms, device) =
        match body.deserialize::<(u32, u32, f64, f64, u64, String)>() {
            Ok((phase, fingers, first, second, at_ms, node)) => (
                phase,
                fingers,
                first,
                second,
                at_ms,
                InputDeviceNode::from_wire(&node),
            ),
            // Protocol 1 had no device argument, and the pinch signal of
            // protocol 2 up to 0.2.16 lost it to a wrong variant type in the
            // extension (ticket 131). GNOME Shell keeps running the extension
            // it loaded until the next login, so this shape can still arrive,
            // and it is read as a gesture from an unknown device.
            Err(_) => match body.deserialize::<(u32, u32, f64, f64, u64)>() {
                Ok((phase, fingers, first, second, at_ms)) => {
                    (phase, fingers, first, second, at_ms, None)
                }
                // A signal whose body is neither shape the interface has
                // declared did not come from an extension this build
                // understands. Dropping it is better than inventing a gesture
                // out of it.
                Err(_) => return None,
            },
        };
    match member.as_str() {
        "SwipeGesture" => Some(ShellGestureEvent::Swipe {
            phase,
            fingers,
            dx: first,
            dy: second,
            at_ms,
            device,
        }),
        "PinchGesture" => Some(ShellGestureEvent::Pinch {
            phase,
            fingers,
            scale: first,
            angle_delta: second,
            at_ms,
            device,
        }),
        _ => None,
    }
}

/// Why the gesture service could not take its place on the bus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceBusError {
    /// Another process already owns the service's name.
    AlreadyRunning,
    Bus(String),
}

impl std::fmt::Display for ServiceBusError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(formatter, "{SERVICE_BUS_NAME} is already owned"),
            Self::Bus(error) => write!(formatter, "{error}"),
        }
    }
}

fn service_bus(error: impl std::fmt::Display) -> ServiceBusError {
    ServiceBusError::Bus(error.to_string())
}

/// The gesture service's own connection (ticket 131).
///
/// It owns [`SERVICE_BUS_NAME`] for as long as the process runs, which is what
/// the extension watches to give GNOME its gestures back when the service is
/// gone. Every call to the adapter goes over this same connection, because the
/// extension takes a request to turn GNOME's gestures off only from that
/// name's owner.
///
/// Everything the service waits for arrives on one stream: the adapter's name
/// gaining or losing its owner, the adapter's gesture signals, and a stop that
/// [`Self::stop_sender`] hands to whoever catches the signals. Subscriptions
/// are made before the name is requested and before the adapter is looked
/// for, so nothing that happens in between is missed.
pub struct ServiceBus {
    runtime: Arc<Runtime>,
    connection: Option<Connection>,
    adapter: String,
    sender: std::sync::mpsc::Sender<SessionEvent>,
    receiver: std::sync::mpsc::Receiver<SessionEvent>,
    status: Arc<std::sync::Mutex<ServiceStatus>>,
    refused: Arc<std::sync::atomic::AtomicUsize>,
}

/// `org.betteros.TouchpadGestures1` on the service's own name: what a window
/// asks to say whether the service can follow its configuration.
struct StatusInterface {
    status: Arc<std::sync::Mutex<ServiceStatus>>,
}

#[zbus::interface(name = "org.betteros.TouchpadGestures1")]
impl StatusInterface {
    /// `(watching, problem)`.
    fn status(&self) -> (bool, String) {
        let status = self
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default();
        (status.watching, status.problem)
    }
}

impl Drop for ServiceBus {
    fn drop(&mut self) {
        let _entered = self.runtime.enter();
        drop(self.connection.take());
    }
}

impl ServiceBus {
    /// The session bus this process was started in.
    pub fn connect() -> Result<Self, ServiceBusError> {
        Self::build(None, BUS_NAME, SERVICE_BUS_NAME)
    }

    /// An explicit address and names. Only a test uses this, so it never
    /// touches the developer's own session bus.
    pub fn connect_to(
        address: &str,
        adapter: &str,
        service: &str,
    ) -> Result<Self, ServiceBusError> {
        Self::build(Some(address), adapter, service)
    }

    fn build(address: Option<&str>, adapter: &str, service: &str) -> Result<Self, ServiceBusError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(service_bus)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let adapter_name = adapter.to_string();
        let status = Arc::new(std::sync::Mutex::new(ServiceStatus {
            watching: true,
            problem: String::new(),
        }));
        let status_for_bus = status.clone();
        let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let refused_by_check = refused.clone();
        let connection = runtime.block_on(async {
            let connection = match address {
                Some(address) => {
                    let address: zbus::Address = address.parse().map_err(service_bus)?;
                    zbus::connection::Builder::address(address)
                        .map_err(service_bus)?
                        .build()
                        .await
                        .map_err(service_bus)?
                }
                None => Connection::session().await.map_err(service_bus)?,
            };
            let owners = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")
                .map_err(service_bus)?
                .interface("org.freedesktop.DBus")
                .map_err(service_bus)?
                .member("NameOwnerChanged")
                .map_err(service_bus)?
                .arg(0, adapter_name.as_str())
                .map_err(service_bus)?
                .build();
            let owners = zbus::MessageStream::for_match_rule(owners, &connection, Some(16))
                .await
                .map_err(service_bus)?;
            // The sender is in the rule, so the bus delivers only the
            // signals of whoever owns the adapter's name when each is sent
            // (ticket 131): a forged one never wakes this process, costs
            // nothing, and cannot push a real one out of the queue. The
            // well-known name rather than the owner's unique one, because the
            // bus resolves it at the moment of sending, so there is no
            // re-subscription after an owner change and no gap in which the
            // new owner's first signals could be missed. The owner check in
            // `forward_gestures` stays as a second line.
            let gestures = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender(adapter_name.as_str())
                .map_err(service_bus)?
                .interface(INTERFACE_NAME)
                .map_err(service_bus)?
                .path(OBJECT_PATH)
                .map_err(service_bus)?
                .build();
            let gestures = zbus::MessageStream::for_match_rule(gestures, &connection, Some(256))
                .await
                .map_err(service_bus)?;

            // The adapter's unique name now. A gesture signal counts only when
            // it comes from it (ticket 131): a signal from anyone else would
            // make the service perform actions and would open the "a gesture
            // arrived" gate that suppression waits for.
            let owner = Arc::new(std::sync::Mutex::new(
                owner_of(&connection, &adapter_name).await,
            ));
            let to_owners = sender.clone();
            tokio::spawn(forward_owners(owners, to_owners, owner.clone()));
            let to_gestures = sender.clone();
            tokio::spawn(forward_gestures(
                gestures,
                to_gestures,
                owner,
                connection.clone(),
                adapter_name.clone(),
                refused_by_check.clone(),
            ));
            connection
                .object_server()
                .at(
                    SERVICE_OBJECT_PATH,
                    StatusInterface {
                        status: status_for_bus.clone(),
                    },
                )
                .await
                .map_err(service_bus)?;

            let reply = match connection
                .request_name_with_flags(service, zbus::fdo::RequestNameFlags::DoNotQueue.into())
                .await
            {
                Ok(reply) => reply,
                Err(zbus::Error::NameTaken) => return Err(ServiceBusError::AlreadyRunning),
                Err(error) => return Err(service_bus(error)),
            };
            match reply {
                zbus::fdo::RequestNameReply::PrimaryOwner
                | zbus::fdo::RequestNameReply::AlreadyOwner => Ok(connection),
                _ => Err(ServiceBusError::AlreadyRunning),
            }
        })?;
        Ok(Self {
            runtime: Arc::new(runtime),
            connection: Some(connection),
            adapter: adapter.to_string(),
            sender,
            receiver,
            status,
            refused,
        })
    }

    /// What `Status()` answers from now on.
    pub fn set_status(&self, status: ServiceStatus) {
        if let Ok(mut slot) = self.status.lock() {
            *slot = status;
        }
    }

    /// A handle `Status()` answers from, for a thread that learns the
    /// watch's state.
    pub fn status_handle(&self) -> Arc<std::sync::Mutex<ServiceStatus>> {
        self.status.clone()
    }

    /// How many gesture signals reached this process from a sender that was
    /// not the adapter's owner. The match rule keeps them out, so this stays
    /// zero unless a bus ignores the rule's sender.
    pub fn signals_refused(&self) -> usize {
        self.refused.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The connection is only ever `None` inside [`Drop`].
    fn connection(&self) -> &Connection {
        self.connection.as_ref().expect("the connection is open")
    }

    /// Whether the adapter extension owns its name now.
    pub fn adapter_present(&self) -> bool {
        let connection = self.connection();
        self.runtime.block_on(async {
            let Ok(proxy) = zbus::fdo::DBusProxy::new(connection).await else {
                return false;
            };
            let Ok(name) = self.adapter.as_str().try_into() else {
                return false;
            };
            proxy.name_has_owner(name).await.unwrap_or(false)
        })
    }

    /// A shell bridge over this same connection, so the adapter sees every
    /// call come from the owner of [`SERVICE_BUS_NAME`].
    pub fn shell(&self) -> SessionBusShell {
        SessionBusShell {
            runtime: self.runtime.clone(),
            connection: Some(self.connection().clone()),
            destination: self.adapter.clone(),
        }
    }

    /// Where a stop is sent from outside, such as a thread that waits for
    /// SIGTERM.
    pub fn stop_sender(&self) -> std::sync::mpsc::Sender<SessionEvent> {
        self.sender.clone()
    }

    /// Blocks until something happens.
    pub fn next_event(&self) -> SessionEvent {
        self.receiver.recv().unwrap_or(SessionEvent::Closed)
    }

    /// The same, giving up after `wait` with `None`. The service never bounds
    /// its wait; a test that has to finish does.
    pub fn next_event_within(&self, wait: std::time::Duration) -> Option<SessionEvent> {
        match self.receiver.recv_timeout(wait) {
            Ok(event) => Some(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(SessionEvent::Closed),
        }
    }
}

/// The unique name that owns `name` now, or `None`.
async fn owner_of(connection: &Connection, name: &str) -> Option<String> {
    let proxy = zbus::fdo::DBusProxy::new(connection).await.ok()?;
    let name: zbus::names::BusName<'_> = name.try_into().ok()?;
    proxy
        .get_name_owner(name)
        .await
        .ok()
        .map(|owner| owner.to_string())
}

type Owner = Arc<std::sync::Mutex<Option<String>>>;

fn set_owner(owner: &Owner, value: Option<String>) {
    if let Ok(mut slot) = owner.lock() {
        *slot = value;
    }
}

fn is_owner(owner: &Owner, sender: &str) -> bool {
    owner
        .lock()
        .map(|slot| slot.as_deref() == Some(sender))
        .unwrap_or(false)
}

async fn forward_owners(
    mut owners: zbus::MessageStream,
    sender: std::sync::mpsc::Sender<SessionEvent>,
    owner: Owner,
) {
    while let Some(message) = owners.next().await {
        let Ok(message) = message else { break };
        // Only the bus itself announces owners.
        let from_bus = message
            .header()
            .sender()
            .is_some_and(|sender| sender.as_str() == "org.freedesktop.DBus");
        let Ok((_, old, new)) = message.body().deserialize::<(String, String, String)>() else {
            continue;
        };
        if !from_bus {
            continue;
        }
        set_owner(&owner, (!new.is_empty()).then(|| new.clone()));
        if !old.is_empty() && sender.send(SessionEvent::AdapterVanished).is_err() {
            return;
        }
        if !new.is_empty() && sender.send(SessionEvent::AdapterAppeared).is_err() {
            return;
        }
    }
    let _ = sender.send(SessionEvent::Closed);
}

async fn forward_gestures(
    mut gestures: zbus::MessageStream,
    sender: std::sync::mpsc::Sender<SessionEvent>,
    owner: Owner,
    connection: Connection,
    adapter: String,
    refused: Arc<std::sync::atomic::AtomicUsize>,
) {
    while let Some(message) = gestures.next().await {
        let Ok(message) = message else { break };
        let Some(from) = message.header().sender().map(|from| from.to_string()) else {
            continue;
        };
        if !is_owner(&owner, &from) {
            // An owner change and a signal from the new owner travel on
            // different streams, so a sender that is not the owner on record
            // is checked against the bus once before it is dropped.
            let now = owner_of(&connection, &adapter).await;
            let trusted = now.as_deref() == Some(from.as_str());
            set_owner(&owner, now);
            if !trusted {
                refused.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                continue;
            }
        }
        if let Some(event) = gesture_from(&message)
            && sender.send(SessionEvent::Gesture(event)).is_err()
        {
            return;
        }
    }
    let _ = sender.send(SessionEvent::Closed);
}

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
    BUS_NAME, INTERFACE_NAME, OBJECT_PATH, ShellBridge, ShellCapabilities, ShellError, ShellEvents,
    ShellGestureEvent, ShellRequest,
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
        let connection = self.connection();
        self.runtime.block_on(async {
            let Ok(proxy) = zbus::fdo::DBusProxy::new(connection).await else {
                return false;
            };
            let Ok(name) = self.destination.as_str().try_into() else {
                return false;
            };
            proxy.name_has_owner(name).await.unwrap_or(false)
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
            let header = message.header();
            let member = header.member()?.to_string();
            let Ok((phase, fingers, first, second, at_ms)) =
                message.body().deserialize::<(u32, u32, f64, f64, u64)>()
            else {
                // A signal whose body is not the shape the interface declares
                // did not come from an extension this build understands.
                // Dropping it is better than inventing a gesture out of it.
                continue;
            };
            let event = match member.as_str() {
                "SwipeGesture" => ShellGestureEvent::Swipe {
                    phase,
                    fingers,
                    dx: first,
                    dy: second,
                    at_ms,
                },
                "PinchGesture" => ShellGestureEvent::Pinch {
                    phase,
                    fingers,
                    scale: first,
                    angle_delta: second,
                    at_ms,
                },
                _ => continue,
            };
            return Some(event);
        }
    }
}

//! The session-bus transport: the extension's methods as a [`Shell`], the
//! extension's signals and presence as events, and the service's own status
//! interface. No decision is made here; [`Relay`] makes them all.
//!
//! A signal is believed only when its sender is the extension's current
//! unique name. A match rule naming the well-known name is not enough: a
//! signal sent straight to this connection skips the bus's match rules, and
//! anyone on the session bus can send one.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use window_attention_core::bus::{
    ACTIVATE_METHOD, ADAPTER_BUS_NAME, ADAPTER_INTERFACE, ADAPTER_OBJECT_PATH, CLEAR_METHOD,
    PROTOCOL_VERSION, SERVICE_BUS_NAME, SERVICE_INTERFACE, SERVICE_OBJECT_PATH, SHOW_BANNER_METHOD,
    SUPPRESS_METHOD, WINDOW_ATTENTION_ENDED_SIGNAL, WINDOW_ATTENTION_SIGNAL,
};
use window_attention_core::{AppId, AttentionRequest, ShellAction};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::zvariant::Value;

use crate::{Relay, Shell, parse_request};

/// The Unity launcher API every GNOME taskbar and dock that draws a mark
/// listens to: Zorin's taskbar, Dash to Panel, and Dash to Dock subscribe to
/// this signal from any sender and any path.
pub const LAUNCHER_ENTRY_INTERFACE: &str = "com.canonical.Unity.LauncherEntry";
pub const LAUNCHER_ENTRY_PATH: &str = "/org/betteros/WindowAttention1/LauncherEntry";

/// The application as the launcher API names it.
pub fn launcher_uri(app: &AppId) -> String {
    format!("application://{app}")
}

/// How long the service keeps asking the extension to take over, in total,
/// before it says the takeover failed. The extension learns that this service
/// owns its name from a signal of its own and refuses until then; on a loaded
/// session that has been seen to take seconds.
pub const TAKEOVER_PATIENCE: Duration = Duration::from_secs(30);

/// The waits between takeover attempts: doubling from 100 ms, at most 5 s
/// apart, adding up to [`TAKEOVER_PATIENCE`].
pub fn takeover_backoff() -> Vec<Duration> {
    let mut waits = Vec::new();
    let mut wait = Duration::from_millis(100);
    let mut total = Duration::ZERO;
    while total + wait <= TAKEOVER_PATIENCE {
        waits.push(wait);
        total += wait;
        wait = (wait * 2).min(Duration::from_secs(5));
    }
    waits
}

/// The extension, reached by its well-known name.
pub struct BusShell {
    connection: Connection,
}

impl BusShell {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    fn call<B>(&self, method: &str, body: &B) -> Result<(), String>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.connection
            .call_method(
                Some(ADAPTER_BUS_NAME),
                ADAPTER_OBJECT_PATH,
                Some(ADAPTER_INTERFACE),
                method,
                body,
            )
            .map(|_| ())
            .map_err(|error| format!("{method}: {error}"))
    }

    fn launcher_update(&self, app: &AppId, urgent: bool) -> Result<(), String> {
        let mut properties: HashMap<&str, Value<'_>> = HashMap::new();
        properties.insert("urgent", Value::from(urgent));
        self.connection
            .emit_signal(
                None::<&str>,
                LAUNCHER_ENTRY_PATH,
                LAUNCHER_ENTRY_INTERFACE,
                "Update",
                &(launcher_uri(app), properties),
            )
            .map_err(|error| format!("Update: {error}"))
    }
}

impl Shell for BusShell {
    fn suppress_built_in(&mut self, suppress: bool) -> Result<(), String> {
        self.call(SUPPRESS_METHOD, &(suppress,))
    }

    fn mark_taskbar(&mut self, app: &AppId) -> Result<(), String> {
        self.launcher_update(app, true)
    }

    fn unmark_taskbar(&mut self, app: &AppId) -> Result<(), String> {
        self.launcher_update(app, false)
    }

    fn show_banner(&mut self, window: u64) -> Result<(), String> {
        self.call(SHOW_BANNER_METHOD, &(window,))
    }

    fn activate(&mut self, window: u64) -> Result<(), String> {
        self.call(ACTIVATE_METHOD, &(window,))
    }

    fn clear(&mut self, window: u64) -> Result<(), String> {
        self.call(CLEAR_METHOD, &(window,))
    }
}

/// What the service reports about itself.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Status {
    /// The extension is on the bus.
    pub adapter_present: bool,
    /// The extension accepted the request to take GNOME's handler off.
    pub suppressed: bool,
    /// The last thing that went wrong, if anything.
    pub problem: String,
}

enum Event {
    /// A signal, with the unique name that sent it.
    Attention(String, AttentionRequest),
    Ended(String, u64),
    /// The extension's unique name, or empty when it left.
    AdapterOwner(String),
    ChoicesChanged,
    /// The service is asked to end (SIGTERM, SIGINT).
    Stop,
}

struct StatusInterface {
    status: Arc<Mutex<Status>>,
    events: Mutex<mpsc::Sender<Event>>,
}

#[zbus::interface(name = "org.betteros.WindowAttention1")]
impl StatusInterface {
    /// `(adapter_present, suppressed, protocol_version, problem)`.
    fn status(&self) -> (bool, bool, u32, String) {
        let status = self
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default();
        (
            status.adapter_present,
            status.suppressed,
            PROTOCOL_VERSION,
            status.problem,
        )
    }

    /// Better Manager saved the choices.
    fn choices_changed(&self) {
        if let Ok(events) = self.events.lock() {
            let _ = events.send(Event::ChoicesChanged);
        }
    }
}

#[derive(Debug)]
pub enum ServeError {
    /// Another instance already owns the name.
    AlreadyRunning,
    Bus(String),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(formatter, "{SERVICE_BUS_NAME} is already owned"),
            Self::Bus(error) => write!(formatter, "{error}"),
        }
    }
}

fn bus(error: impl std::fmt::Display) -> ServeError {
    ServeError::Bus(error.to_string())
}

fn signal_rule(member: &str) -> Result<MatchRule<'_>, ServeError> {
    Ok(MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(ADAPTER_BUS_NAME)
        .map_err(bus)?
        .path(ADAPTER_OBJECT_PATH)
        .map_err(bus)?
        .interface(ADAPTER_INTERFACE)
        .map_err(bus)?
        .member(member)
        .map_err(bus)?
        .build())
}

/// The extension's unique name now, or `None` when it is not on the bus.
fn adapter_owner(connection: &Connection) -> Option<String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(connection).ok()?;
    dbus.get_name_owner(ADAPTER_BUS_NAME.try_into().ok()?)
        .ok()
        .map(|owner| owner.to_string())
}

/// Serves until the connection closes. `log` receives one line per thing
/// that happened, for the journal.
pub fn serve(
    connection: Connection,
    relay: Relay,
    log: impl FnMut(String),
) -> Result<(), ServeError> {
    let (_keep, never) = mpsc::channel();
    serve_until(connection, relay, log, never)
}

/// [`serve`], ending when `stop` receives: every taskbar mark this service
/// holds is taken back first, so none outlives it, and then it returns. Its
/// bus name goes with the connection, and the extension gives GNOME its own
/// handler back.
pub fn serve_until(
    connection: Connection,
    mut relay: Relay,
    mut log: impl FnMut(String),
    stop: mpsc::Receiver<()>,
) -> Result<(), ServeError> {
    let status = Arc::new(Mutex::new(Status::default()));
    let (sender, events) = mpsc::channel();
    let stop_sender = sender.clone();
    std::thread::spawn(move || {
        if stop.recv().is_ok() {
            let _ = stop_sender.send(Event::Stop);
        }
    });
    connection
        .object_server()
        .at(
            SERVICE_OBJECT_PATH,
            StatusInterface {
                status: status.clone(),
                events: Mutex::new(sender.clone()),
            },
        )
        .map_err(bus)?;

    // Subscribe before anything can happen, so nothing is missed between
    // owning the name and listening.
    let owner_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .map_err(bus)?
        .interface("org.freedesktop.DBus")
        .map_err(bus)?
        .member("NameOwnerChanged")
        .map_err(bus)?
        .arg(0, ADAPTER_BUS_NAME)
        .map_err(bus)?
        .build();
    let owners = MessageIterator::for_match_rule(owner_rule, &connection, Some(16)).map_err(bus)?;
    let requests = MessageIterator::for_match_rule(
        signal_rule(WINDOW_ATTENTION_SIGNAL)?,
        &connection,
        Some(64),
    )
    .map_err(bus)?;
    let endings = MessageIterator::for_match_rule(
        signal_rule(WINDOW_ATTENTION_ENDED_SIGNAL)?,
        &connection,
        Some(64),
    )
    .map_err(bus)?;

    let owner_sender = sender.clone();
    std::thread::spawn(move || {
        for message in owners {
            let Ok(message) = message else { break };
            // Only the bus itself announces owners.
            let from_bus = message
                .header()
                .sender()
                .is_some_and(|sender| sender.as_str() == "org.freedesktop.DBus");
            let Ok((_, _, new_owner)) = message.body().deserialize::<(String, String, String)>()
            else {
                continue;
            };
            if from_bus && owner_sender.send(Event::AdapterOwner(new_owner)).is_err() {
                break;
            }
        }
    });
    let request_sender = sender.clone();
    std::thread::spawn(move || {
        for message in requests {
            let Ok(message) = message else { break };
            let Some(from) = message.header().sender().map(|sender| sender.to_string()) else {
                continue;
            };
            let Ok(body) = message
                .body()
                .deserialize::<(u64, String, String, u32, bool, bool)>()
            else {
                continue;
            };
            let Some(request) = parse_request(body) else {
                continue;
            };
            if request_sender
                .send(Event::Attention(from, request))
                .is_err()
            {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for message in endings {
            let Ok(message) = message else { break };
            let Some(from) = message.header().sender().map(|sender| sender.to_string()) else {
                continue;
            };
            let Ok((window, _reason)) = message.body().deserialize::<(u64, u32)>() else {
                continue;
            };
            if sender.send(Event::Ended(from, window)).is_err() {
                break;
            }
        }
    });

    let reply = match connection
        .request_name_with_flags(SERVICE_BUS_NAME, RequestNameFlags::DoNotQueue.into())
    {
        Ok(reply) => reply,
        Err(zbus::Error::NameTaken) => return Err(ServeError::AlreadyRunning),
        Err(error) => return Err(bus(error)),
    };
    if reply != RequestNameReply::PrimaryOwner && reply != RequestNameReply::AlreadyOwner {
        return Err(ServeError::AlreadyRunning);
    }

    let mut shell = BusShell::new(connection.clone());
    let mut owner = adapter_owner(&connection);
    let set_status = |change: &dyn Fn(&mut Status)| {
        if let Ok(mut status) = status.lock() {
            change(&mut status);
        }
    };
    let take_over = |relay: &mut Relay,
                     shell: &mut BusShell,
                     owner: &Option<String>,
                     log: &mut dyn FnMut(String)| {
        set_status(&|status| {
            status.adapter_present = owner.is_some();
            status.suppressed = false;
        });
        if owner.is_none() {
            for problem in relay.adapter_gone(shell) {
                log(problem);
            }
            log("the shell adapter is not on the bus; GNOME keeps its own handler".to_string());
            return;
        }
        let mut attempt = relay.adapter_appeared(shell);
        for wait in takeover_backoff() {
            if attempt.is_ok() {
                break;
            }
            std::thread::sleep(wait);
            attempt = relay.adapter_appeared(shell);
        }
        match attempt {
            Ok(()) => {
                set_status(&|status| {
                    status.suppressed = true;
                    status.problem.clear();
                });
                log("the shell adapter is present; GNOME's own handler is off".to_string());
            }
            Err(error) => {
                set_status(&|status| status.problem = error.clone());
                log(format!("the shell adapter did not take over: {error}"));
            }
        }
    };
    take_over(&mut relay, &mut shell, &owner, &mut log);

    // Whether a signal came from the extension. A sender that is not the
    // owner on record is checked against the bus once, because an owner
    // change and a signal from the new owner travel on different streams.
    let from_adapter = |from: &str, owner: &mut Option<String>, log: &mut dyn FnMut(String)| {
        if owner.as_deref() == Some(from) {
            return true;
        }
        *owner = adapter_owner(&connection);
        let trusted = owner.as_deref() == Some(from);
        if !trusted {
            log(format!(
                "ignored a signal from {from}, which is not the shell adapter"
            ));
        }
        trusted
    };

    for event in events {
        match event {
            Event::AdapterOwner(new_owner) => {
                owner = (!new_owner.is_empty()).then_some(new_owner);
                take_over(&mut relay, &mut shell, &owner, &mut log);
            }
            Event::Attention(from, request) => {
                if !from_adapter(&from, &mut owner, &mut log) {
                    continue;
                }
                let handled = relay.handle(&mut shell, &request);
                let action = match &handled.action {
                    ShellAction::Leave => "left the request on the window".to_string(),
                    ShellAction::MarkTaskbar(_) => "marked the taskbar icon".to_string(),
                    ShellAction::ShowBanner(_) => "showed GNOME's banner".to_string(),
                    ShellAction::Clear(_) => "cleared the request".to_string(),
                    ShellAction::Activate(_) => "brought the window forward".to_string(),
                };
                log(format!("{}: {action}", request.app_id));
                for problem in [
                    handled.failure,
                    handled.choices_problem,
                    handled.record_problem,
                ]
                .into_iter()
                .flatten()
                {
                    log(problem.clone());
                    set_status(&|status| status.problem = problem.clone());
                }
            }
            Event::Ended(from, window) => {
                if !from_adapter(&from, &mut owner, &mut log) {
                    continue;
                }
                if let Some(problem) = relay.ended(&mut shell, window) {
                    log(problem);
                }
            }
            Event::ChoicesChanged => {
                for problem in relay.choices_changed(&mut shell) {
                    log(problem);
                }
            }
            Event::Stop => {
                for problem in relay.adapter_gone(&mut shell) {
                    log(problem);
                }
                log("asked to stop; every taskbar mark was taken back".to_string());
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Reads the service's own status from another process: Better Manager's
/// page asks this. Calling it starts the service through D-Bus activation
/// when it is installed and not running.
pub fn query_status(connection: &Connection) -> Result<(bool, bool, u32, String), String> {
    connection
        .call_method(
            Some(SERVICE_BUS_NAME),
            SERVICE_OBJECT_PATH,
            Some(SERVICE_INTERFACE),
            "Status",
            &(),
        )
        .and_then(|reply| reply.body().deserialize::<(bool, bool, u32, String)>())
        .map_err(|error| error.to_string())
}

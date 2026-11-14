//! The session D-Bus surface.
//!
//! Bus name `org.betteros.Awake1`, object path `/org/betteros/Awake1`, on the
//! *session* bus — one Better Awake per logged-in user, holding what that user
//! asked for and nothing else. Requests and replies cross as JSON documents,
//! the shape ADR 0007 chose for the privileged daemon, so both local protocols
//! in this workspace are read the same way.
//!
//! Nothing here is authorized: the caller is already the same user as the
//! service, and asking polkit whether a user may keep their own screen on would
//! be theatre.

use std::sync::Arc;

use awake_ipc::{AwakeEvent, AwakeRequest, AwakeResponse, EventBody, MAX_REQUEST_BYTES};
use zbus::interface;
use zbus::object_server::SignalEmitter;

use crate::backend::InhibitorBackend;
use crate::engine::{AwakeEngine, EndedSession};
use crate::notify::{
    LidClosedStop, LowBatteryNotifier, LowBatteryStop, lid_notifying_tray_is_running,
    tray_is_running,
};

pub const BUS_NAME: &str = "org.betteros.Awake1";
pub const OBJECT_PATH: &str = "/org/betteros/Awake1";
pub const INTERFACE_NAME: &str = "org.betteros.Awake1";

pub struct AwakeDbusService<B: InhibitorBackend + 'static> {
    engine: Arc<AwakeEngine<B>>,
    notifier: Option<LowBatteryNotifier>,
}

impl<B: InhibitorBackend + 'static> AwakeDbusService<B> {
    pub fn new(engine: Arc<AwakeEngine<B>>) -> Self {
        Self {
            engine,
            notifier: None,
        }
    }

    /// Raises the low-battery notification for a stop found while answering a
    /// request, when no tray is running to raise it.
    pub fn with_notifier(mut self, notifier: LowBatteryNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }
}

#[interface(name = "org.betteros.Awake1")]
impl<B: InhibitorBackend + 'static> AwakeDbusService<B> {
    /// One request in, one reply out.
    ///
    /// A malformed document is refused as a typed rejection rather than a bus
    /// error, because "your request was wrong" is an answer the tray must be
    /// able to show, not a transport failure it has to guess about.
    async fn request(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        request_json: &str,
    ) -> String {
        if request_json.len() > MAX_REQUEST_BYTES {
            return rejection(awake_ipc::IpcError::PayloadTooLarge {
                bytes: request_json.len(),
                limit: MAX_REQUEST_BYTES,
            });
        }
        let request = match AwakeRequest::from_json(request_json) {
            Ok(request) => request,
            // Answered in the version the client spoke, so a legacy client
            // reads the refusal rather than a version error about it.
            Err(error) => {
                return rejection_for(error.to_string(), requested_version(request_json));
            }
        };
        let version = request.protocol_version;
        // A read changes nothing, so it must not make every other client redraw.
        // Listed rather than negated, so a request added later is treated as a
        // change until someone deliberately says it is not.
        let query_only = matches!(
            request.body,
            awake_ipc::RequestBody::QueryStatus
                | awake_ipc::RequestBody::QueryRules
                | awake_ipc::RequestBody::QueryHistory { .. }
                | awake_ipc::RequestBody::TestRule { .. }
        );

        let (response, ended) = self.engine.handle_reporting(request).await;

        // A session the service ended by itself while answering is announced
        // first, so a client reads why it ended before the status that no
        // longer lists it.
        announce_ended::<B>(&emitter, &ended, self.notifier.as_ref()).await;

        // A change every client must see, pushed once, carrying the whole
        // state so a client that missed an earlier signal is still correct.
        if !query_only {
            announce_status(&emitter, &self.engine).await;
        }

        response
            .to_json_for(version)
            .unwrap_or_else(|error| rejection_for(error.to_string(), version))
    }

    /// Carries every event document the service pushes, not only a status:
    /// a `SessionEnded` travels on it too, just before the status that no
    /// longer lists it.
    ///
    /// In protocol version 2's shape, for a window or tray installed before
    /// version 3 that is still running. A current client listens to
    /// [`Self::event`].
    #[zbus(signal)]
    async fn status_changed(emitter: &SignalEmitter<'_>, event_json: &str) -> zbus::Result<()>;

    /// The same events as `StatusChanged`, in the current protocol version.
    #[zbus(signal)]
    async fn event(emitter: &SignalEmitter<'_>, event_json: &str) -> zbus::Result<()>;

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        awake_ipc::PROTOCOL_VERSION
    }
}

/// One service tick, with what it changed pushed to the clients.
///
/// A tick is not answering anyone, so without this a session that expired, hit
/// its battery threshold, or was started by a rule would leave every open menu
/// showing what was there before. Each ended session is announced once, then
/// one status for the whole tick. A tick that changed neither the sessions nor
/// the refused rules pushes nothing. A low-battery stop is also raised as a
/// desktop notification through `notifier` when no tray is running.
///
/// Returns the sessions the tick ended.
pub async fn tick_and_announce<B: InhibitorBackend + 'static>(
    engine: &AwakeEngine<B>,
    emitter: &SignalEmitter<'_>,
    notifier: Option<&LowBatteryNotifier>,
) -> Vec<EndedSession> {
    let outcome = engine.tick().await;
    announce_ended::<B>(emitter, &outcome.ended, notifier).await;
    if outcome.changed {
        announce_status(emitter, engine).await;
    }
    outcome.ended
}

/// Releases everything and, while the service is still on the bus to say so,
/// tells the clients which sessions that ended.
pub async fn shutdown_and_announce<B: InhibitorBackend + 'static>(
    engine: &AwakeEngine<B>,
    emitter: &SignalEmitter<'_>,
) -> Vec<EndedSession> {
    let ended = engine.shutdown().await;
    if !ended.is_empty() {
        // Shutting down stops no session for low battery, so there is nothing
        // to notify.
        announce_ended::<B>(emitter, &ended, None).await;
        announce_status(emitter, engine).await;
    }
    ended
}

/// A signal that cannot be sent has no one to report to — the bus is the
/// channel the report would travel on — so a failure is dropped here, as the
/// status signal always has been.
///
/// A low-battery stop is raised as a notification by the service only when no
/// tray owns its name. The bus is asked before the signal goes out: a tray that
/// takes its name in between has already subscribed, so it hears the stop and
/// notifies too. Two notifications is the better way for that race to end
/// than none.
async fn announce_ended<B: InhibitorBackend + 'static>(
    emitter: &SignalEmitter<'_>,
    ended: &[EndedSession],
    notifier: Option<&LowBatteryNotifier>,
) {
    let stops = LowBatteryStop::among(ended);
    let lid_stops = LidClosedStop::among(ended);
    // Asked separately: a tray from before protocol version 3 owns the tray
    // name and raises the low-battery notification, but cannot read a
    // closed-lid stop, so that one is the service's unless a version 3 tray
    // has said it raises it.
    let battery_notifier = match notifier {
        Some(notifier) if !stops.is_empty() && !tray_is_running(emitter.connection()).await => {
            Some(notifier)
        }
        _ => None,
    };
    let lid_notifier = match notifier {
        Some(notifier)
            if !lid_stops.is_empty()
                && !lid_notifying_tray_is_running(emitter.connection()).await =>
        {
            Some(notifier)
        }
        _ => None,
    };

    for ended in ended {
        emit::<B>(emitter, &ended.event()).await;
    }

    if let Some(notifier) = battery_notifier {
        notifier.raise(emitter.connection(), stops);
    }
    if let Some(notifier) = lid_notifier {
        notifier.raise_lid(emitter.connection(), lid_stops);
    }
}

/// Pushes one event on both signals: the legacy shape first, then the current
/// one, so neither kind of client depends on the other's signal.
async fn emit<B: InhibitorBackend + 'static>(emitter: &SignalEmitter<'_>, event: &AwakeEvent) {
    if let Ok(document) = event.to_legacy_json() {
        let _ = AwakeDbusService::<B>::status_changed(emitter, &document).await;
    }
    if let Ok(document) = event.to_json() {
        let _ = AwakeDbusService::<B>::event(emitter, &document).await;
    }
}

async fn announce_status<B: InhibitorBackend + 'static>(
    emitter: &SignalEmitter<'_>,
    engine: &AwakeEngine<B>,
) {
    let event = AwakeEvent::new(EventBody::StatusChanged(Box::new(engine.status().await)));
    emit::<B>(emitter, &event).await;
}

fn rejection(error: awake_ipc::IpcError) -> String {
    rejection_for(error.to_string(), awake_ipc::PROTOCOL_VERSION)
}

/// The version a request that could not be read was stamped with, when it is
/// the legacy one; the current version otherwise.
fn requested_version(request_json: &str) -> u32 {
    serde_json::from_str::<serde_json::Value>(request_json)
        .ok()
        .and_then(|value| value.get("protocol_version")?.as_u64())
        .filter(|version| *version == u64::from(awake_ipc::LEGACY_PROTOCOL_VERSION))
        .map_or(awake_ipc::PROTOCOL_VERSION, |_| {
            awake_ipc::LEGACY_PROTOCOL_VERSION
        })
}

fn rejection_for(error_key: String, version: u32) -> String {
    AwakeResponse::rejected(error_key)
        .to_json_for(version)
        // Serializing a rejection cannot realistically fail, but a panic in the
        // service would end every session it holds, so the fallback is a
        // hand-built document rather than an unwrap.
        .unwrap_or_else(|_| {
            format!(
                r#"{{"protocol_version":{version},"body":{{"response":"rejected","error_key":"awake.ipc.error.malformed"}}}}"#
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_that_cannot_be_read_is_refused_in_the_version_it_spoke() {
        assert_eq!(
            requested_version(r#"{"protocol_version":2,"body":{"request":"reboot"}}"#),
            awake_ipc::LEGACY_PROTOCOL_VERSION
        );
        for document in [
            r#"{"protocol_version":3,"body":{}}"#,
            r#"{"protocol_version":7}"#,
            "not json",
        ] {
            assert_eq!(
                requested_version(document),
                awake_ipc::PROTOCOL_VERSION,
                "{document}"
            );
        }
        let legacy = rejection_for("x".to_string(), awake_ipc::LEGACY_PROTOCOL_VERSION);
        let legacy: serde_json::Value = serde_json::from_str(&legacy).unwrap();
        assert_eq!(legacy["protocol_version"], 2);
        assert_eq!(legacy["body"]["error_key"], "x");
    }

    #[test]
    fn a_rejection_is_always_a_readable_document() {
        let document = rejection(awake_ipc::IpcError::InvalidSessionId { session_id: 0 });
        let response = AwakeResponse::from_json(&document).unwrap();
        assert_eq!(
            response.body,
            awake_ipc::ResponseBody::Rejected {
                error_key: "awake.ipc.error.invalid_session_id:0".to_string()
            }
        );
    }
}

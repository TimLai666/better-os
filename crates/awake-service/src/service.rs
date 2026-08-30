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
use crate::engine::AwakeEngine;

pub const BUS_NAME: &str = "org.betteros.Awake1";
pub const OBJECT_PATH: &str = "/org/betteros/Awake1";
pub const INTERFACE_NAME: &str = "org.betteros.Awake1";

pub struct AwakeDbusService<B: InhibitorBackend + 'static> {
    engine: Arc<AwakeEngine<B>>,
}

impl<B: InhibitorBackend + 'static> AwakeDbusService<B> {
    pub fn new(engine: Arc<AwakeEngine<B>>) -> Self {
        Self { engine }
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
            Err(error) => return rejection(error),
        };
        let query_only = matches!(request.body, awake_ipc::RequestBody::QueryStatus);

        let response = self.engine.handle(request).await;

        // A change every client must see, pushed once, carrying the whole
        // state so a client that missed an earlier signal is still correct.
        if !query_only {
            let event = AwakeEvent::new(EventBody::StatusChanged(Box::new(
                self.engine.status().await,
            )));
            if let Ok(document) = event.to_json() {
                let _ = Self::status_changed(&emitter, &document).await;
            }
        }

        response
            .to_json()
            .unwrap_or_else(|error| rejection_string(error.to_string()))
    }

    #[zbus(signal)]
    async fn status_changed(emitter: &SignalEmitter<'_>, event_json: &str) -> zbus::Result<()>;

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        awake_ipc::PROTOCOL_VERSION
    }
}

fn rejection(error: awake_ipc::IpcError) -> String {
    rejection_string(error.to_string())
}

fn rejection_string(error_key: String) -> String {
    AwakeResponse::rejected(error_key.clone())
        .to_json()
        // Serializing a rejection cannot realistically fail, but a panic in the
        // service would end every session it holds, so the fallback is a
        // hand-built document rather than an unwrap.
        .unwrap_or_else(|_| {
            format!(
                r#"{{"protocol_version":{},"body":{{"response":"rejected","error_key":"awake.ipc.error.malformed"}}}}"#,
                awake_ipc::PROTOCOL_VERSION
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

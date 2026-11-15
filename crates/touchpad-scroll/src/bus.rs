//! The service's one interface on the system bus.
//!
//! Two methods, two doubles in, nothing free-form: `SetFactors` and
//! `Describe`. A call that sets is refused unless its caller is the person at
//! the machine (`touchpad_scroll::session_rule`), and the values are refused
//! unless they are valid scroll factors.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use touchpad_scroll::factors::{Factors, SharedFactors};
use touchpad_scroll::session_rule::{Decision, RefusalLog, SessionFacts, SessionState, decide};
use zbus::names::{BusName, UniqueName};
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, fdo, interface, message::Header};

pub struct ScrollService {
    pub factors: SharedFactors,
    pub state_file: PathBuf,
    pub device_name: String,
    pub refusals: Mutex<RefusalLog>,
}

#[interface(name = "org.betteros.TouchpadScroll1")]
impl ScrollService {
    /// Set both factors. They take effect at the next frame, and are kept for
    /// the next start.
    async fn set_factors(
        &self,
        vertical: f64,
        horizontal: f64,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<()> {
        let factors = Factors::new(vertical, horizontal).map_err(fdo::Error::InvalidArgs)?;
        let sender = header
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("the call has no sender".to_string()))?;
        let facts = session_facts(connection, sender)
            .await
            .map_err(fdo::Error::AccessDenied)?;
        if let Decision::Refused(why) = decide(&facts) {
            let log = self
                .refusals
                .lock()
                .map(|mut log| log.note(Instant::now()))
                .unwrap_or(None);
            if let Some(swallowed) = log {
                eprintln!(
                    "refused a factor change from uid {}: {why} ({swallowed} more refusals not logged)",
                    facts.uid
                );
            }
            return Err(fdo::Error::AccessDenied(why.to_string()));
        }
        let written = factors
            .store_if_changed(self.factors.get(), &self.state_file)
            .map_err(|error| {
                fdo::Error::Failed(format!("{}: {error}", self.state_file.display()))
            })?;
        if !written {
            // Nothing changes: no write, no log line.
            return Ok(());
        }
        self.factors.set(factors);
        eprintln!(
            "uid {} set vertical {} and horizontal {}",
            facts.uid, factors.vertical, factors.horizontal
        );
        Ok(())
    }

    /// The touchpad's name and the factors in force.
    async fn describe(&self) -> (String, f64, f64) {
        let factors = self.factors.get();
        (
            self.device_name.clone(),
            factors.vertical,
            factors.horizontal,
        )
    }
}

const LOGIND: &str = "org.freedesktop.login1";

async fn session_state(
    connection: &Connection,
    path: OwnedObjectPath,
) -> Result<SessionState, String> {
    let text = |error: zbus::Error| error.to_string();
    let session = zbus::Proxy::new(connection, LOGIND, path, "org.freedesktop.login1.Session")
        .await
        .map_err(text)?;
    Ok(SessionState {
        active: session.get_property("Active").await.map_err(text)?,
        remote: session.get_property("Remote").await.map_err(text)?,
    })
}

/// The calling process's own session, and the user's display session for a
/// process that is in none (see `touchpad_scroll::session_rule`).
async fn session_facts(
    connection: &Connection,
    sender: &UniqueName<'_>,
) -> Result<SessionFacts, String> {
    let text = |error: zbus::Error| error.to_string();
    let dbus = fdo::DBusProxy::new(connection).await.map_err(text)?;
    let caller = BusName::Unique(sender.to_owned());
    let uid = dbus
        .get_connection_unix_user(caller.clone())
        .await
        .map_err(|error| error.to_string())?;
    let pid = dbus
        .get_connection_unix_process_id(caller)
        .await
        .map_err(|error| error.to_string())?;
    let manager = zbus::Proxy::new(
        connection,
        LOGIND,
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(text)?;
    let mut facts = SessionFacts {
        uid,
        caller_session: None,
        display_session: None,
    };
    // "No session for this process" is an answer, not an error.
    if let Ok(path) = manager
        .call::<_, _, OwnedObjectPath>("GetSessionByPID", &(pid,))
        .await
    {
        facts.caller_session = Some(session_state(connection, path).await?);
        return Ok(facts);
    }
    let Ok(user_path) = manager
        .call::<_, _, OwnedObjectPath>("GetUser", &(uid,))
        .await
    else {
        return Ok(facts);
    };
    let user = zbus::Proxy::new(connection, LOGIND, user_path, "org.freedesktop.login1.User")
        .await
        .map_err(text)?;
    let (_, display): (String, OwnedObjectPath) =
        user.get_property("Display").await.map_err(text)?;
    if display.as_str() != "/" {
        facts.display_session = Some(session_state(connection, display).await?);
    }
    Ok(facts)
}

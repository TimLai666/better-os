//! `org.betteros.LoginSupervisor1`: the last record, read-only, for
//! `better-os-login-status`. One method, no argument, nothing it changes.
//! Served from its own thread, so a system bus that is down or hung never
//! holds up a stage.

use std::path::{Path, PathBuf};

use manager_ipc::boot_watch::{Records, latest_supervisor_event};

use crate::rules::{Loaded, State};

pub const NAME: &str = "org.betteros.LoginSupervisor1";
pub const PATH: &str = "/org/betteros/LoginSupervisor1";
pub const INTERFACE: &str = "org.betteros.LoginSupervisor1";

/// What `LastRecord` returns, as JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LastRecord {
    pub state: Option<State>,
    pub event: Option<manager_ipc::boot_watch::Event>,
}

impl LastRecord {
    /// Read from the files, as the service and the status command's own
    /// fallback both do.
    pub fn read(state: &Path, records: &Records) -> LastRecord {
        LastRecord {
            state: match crate::state::load(state) {
                Loaded::Found(state) => Some(*state),
                Loaded::Absent | Loaded::Unreadable => None,
            },
            event: latest_supervisor_event(records),
        }
    }
}

struct Service {
    state: PathBuf,
    records: Records,
}

#[zbus::interface(name = "org.betteros.LoginSupervisor1")]
impl Service {
    /// The state of this boot's sign-in path and the newest record, as
    /// JSON.
    fn last_record(&self) -> String {
        serde_json::to_string(&LastRecord::read(&self.state, &self.records))
            .unwrap_or_else(|_| "{}".to_string())
    }
}

/// Owns the name and serves it for as long as the process runs, trying
/// again every five seconds while the bus cannot be reached.
pub fn serve_in_background(state: PathBuf, records: Records) {
    std::thread::spawn(move || {
        loop {
            let built = zbus::blocking::connection::Builder::system()
                .and_then(|builder| builder.name(NAME))
                .and_then(|builder| {
                    builder.serve_at(
                        PATH,
                        Service {
                            state: state.clone(),
                            records: records.clone(),
                        },
                    )
                })
                .and_then(|builder| builder.build());
            match built {
                Ok(connection) => {
                    // The connection serves from zbus's own thread; this one
                    // only keeps it alive.
                    loop {
                        std::thread::park();
                        let _ = &connection;
                    }
                }
                Err(error) => {
                    eprintln!("{NAME} is not served yet: {error}");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
            }
        }
    });
}

/// Asks the running supervisor, within two seconds.
pub fn ask() -> Result<LastRecord, String> {
    let connection = zbus::blocking::connection::Builder::system()
        .map_err(|e| e.to_string())?
        .method_timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let reply = connection
        .call_method(Some(NAME), PATH, Some(INTERFACE), "LastRecord", &())
        .map_err(|e| e.to_string())?;
    let json: String = reply.body().deserialize().map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

//! Where the window's numbers come from.
//!
//! Better Monitor prefers the service and falls back to collecting in this
//! process, and the difference is never hidden: a window that is recording for
//! itself says so on every page, because history it is not recording is
//! history that will not be there when the user comes back to look for it.
//!
//! Both modes serve the same requests and return the same documents, because
//! both are the same engine — `monitor-service` owns the collectors either
//! way. The window has no code that reads `/proc`, and no page can tell which
//! side answered except by the note.
//!
//! The engine and the service client both speak tokio while GPUI speaks its
//! own executor, so the bridge is one thread with one current-thread runtime
//! and two executor-agnostic channels. Nothing on the render thread ever waits
//! on a bus or a disk.

use std::sync::Arc;
use std::time::Duration;

use monitor_collectors_linux::ProcessPrivacy;
use monitor_ipc::{
    ExportDocument, HistoryDocument, IncidentWindowDocument, IncidentsDocument,
    InventoryDiffDocument, InventoryDocument, StatusDocument,
};
use monitor_service::{MonitorClient, MonitorEngine, ServiceConfig};

/// How often the window asks what is happening now, and how often the embedded
/// engine takes a round. One second is above every counter metric's minimum
/// delta interval.
pub(crate) const POLL_INTERVAL_MILLIS: u64 = 1_000;

/// The largest history reply the window asks for. Well under the protocol's
/// own cap: a chart cannot usefully draw more.
pub(crate) const MAX_HISTORY_SAMPLES: u32 = 5_000;

/// How much history the History page asks for by default.
pub(crate) const DEFAULT_HISTORY_SECONDS: u64 = 900;

/// What the window asks for, beyond the periodic status.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LinkRequest {
    History {
        seconds: u64,
    },
    Incidents,
    Inventory,
    Mark {
        note: Option<String>,
        before_seconds: u64,
        after_seconds: u64,
        about_pid: Option<u32>,
    },
    /// Only the embedded engine can honour this. A running service collects
    /// under its own configuration, and a window must not be able to change
    /// what another user's session is recording.
    SetPrivacy(ProcessPrivacy),
}

/// What comes back.
#[derive(Debug)]
pub(crate) enum LinkUpdate {
    /// The current state, on every poll.
    Status(Box<StatusDocument>),
    /// The service could not be reached, so this window is collecting for
    /// itself. Sent once, before the first status.
    Embedded(String),
    /// Neither the service nor an embedded engine could be started. There is
    /// nothing to show and the window says exactly that.
    Unavailable(String),
    History(Box<HistoryDocument>),
    Incidents(Box<IncidentsDocument>),
    Inventory(Box<InventoryDocument>, Box<InventoryDiffDocument>),
    Marked(Box<IncidentWindowDocument>),
    #[allow(dead_code)]
    Export(Box<ExportDocument>),
    /// A request was refused or the transport failed. A stable key.
    Failed(String),
}

/// Start the bridge thread. Returns immediately.
pub(crate) fn spawn(
    requests: smol::channel::Receiver<LinkRequest>,
    updates: smol::channel::Sender<LinkUpdate>,
) {
    let started = std::thread::Builder::new()
        .name("monitor-link".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = updates.send_blocking(LinkUpdate::Unavailable(format!(
                        "monitor.gui.link.no_runtime:{error}"
                    )));
                    return;
                }
            };
            runtime.block_on(serve(requests, updates));
        });
    if let Err(error) = started {
        eprintln!("monitor-gui: the collection thread could not be started: {error}");
    }
}

/// Which side is answering.
enum Backend {
    Service(MonitorClient),
    Embedded(Arc<MonitorEngine>),
}

async fn serve(
    requests: smol::channel::Receiver<LinkRequest>,
    updates: smol::channel::Sender<LinkUpdate>,
) {
    let backend = match connect().await {
        Ok(client) => Backend::Service(client),
        Err(detail) => {
            // No service. Collect in this process instead, and say so before
            // the first number appears, so nothing is ever shown without the
            // caveat attached.
            match MonitorEngine::start(ServiceConfig::system()) {
                Ok(engine) => {
                    if updates.send(LinkUpdate::Embedded(detail)).await.is_err() {
                        return;
                    }
                    Backend::Embedded(engine)
                }
                Err(error) => {
                    let _ = updates
                        .send(LinkUpdate::Unavailable(format!("{detail} / {error}")))
                        .await;
                    return;
                }
            }
        }
    };

    let mut poll = tokio::time::interval(Duration::from_millis(POLL_INTERVAL_MILLIS));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = poll.tick() => {
                match &backend {
                    Backend::Service(client) => match client.status(true).await {
                        Ok(status) => {
                            if updates
                                .send(LinkUpdate::Status(Box::new(status)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(error) => {
                            // The service went away mid-session. The window is
                            // told rather than left frozen on the last numbers
                            // it happened to have.
                            let _ = updates
                                .send(LinkUpdate::Unavailable(error.to_string()))
                                .await;
                            return;
                        }
                    },
                    Backend::Embedded(engine) => {
                        if let Err(error) = engine.tick().await {
                            let _ = updates.send(LinkUpdate::Failed(error.to_string())).await;
                        }
                        let status = engine.status(true).await;
                        if updates
                            .send(LinkUpdate::Status(Box::new(status)))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            request = requests.recv() => {
                let Ok(request) = request else { break };
                let update = handle(&backend, request).await;
                if updates.send(update).await.is_err() {
                    break;
                }
            }
        }
    }

    // The window closed. An embedded engine has to flush the bucket it was
    // holding, or the last seconds before the window went away are lost.
    if let Backend::Embedded(engine) = backend {
        let _ = engine.shutdown().await;
    }
}

async fn connect() -> Result<MonitorClient, String> {
    let client = MonitorClient::connect()
        .await
        .map_err(|error| error.to_string())?;
    // A proxy proves nothing on its own; a property read proves the service is
    // on the bus and speaking this protocol version.
    match client.protocol_version().await {
        Ok(version) if version == monitor_ipc::PROTOCOL_VERSION => Ok(client),
        Ok(version) => Err(format!(
            "monitor.gui.link.protocol_version:{version}:{}",
            monitor_ipc::PROTOCOL_VERSION
        )),
        Err(error) => Err(error.to_string()),
    }
}

async fn handle(backend: &Backend, request: LinkRequest) -> LinkUpdate {
    match backend {
        Backend::Service(client) => handle_service(client, request).await,
        Backend::Embedded(engine) => handle_embedded(engine, request).await,
    }
}

async fn handle_service(client: &MonitorClient, request: LinkRequest) -> LinkUpdate {
    let now = monitor_service::now_unix_ms();
    match request {
        LinkRequest::History { seconds } => {
            let range = monitor_store::TimeRange::last(seconds, now);
            match client
                .history(range.from_unix_ms, range.to_unix_ms, MAX_HISTORY_SAMPLES)
                .await
            {
                Ok(document) => LinkUpdate::History(Box::new(document)),
                Err(error) => LinkUpdate::Failed(error.to_string()),
            }
        }
        LinkRequest::Incidents => match client.incidents().await {
            Ok(document) => LinkUpdate::Incidents(Box::new(document)),
            Err(error) => LinkUpdate::Failed(error.to_string()),
        },
        LinkRequest::Inventory => {
            let inventory = match client.inventory().await {
                Ok(document) => document,
                Err(error) => return LinkUpdate::Failed(error.to_string()),
            };
            let diff = match client.inventory_diff().await {
                Ok(document) => document,
                Err(error) => return LinkUpdate::Failed(error.to_string()),
            };
            LinkUpdate::Inventory(Box::new(inventory), Box::new(diff))
        }
        LinkRequest::Mark {
            note,
            before_seconds,
            after_seconds,
            about_pid,
        } => match client
            .mark(note, before_seconds, after_seconds, about_pid)
            .await
        {
            Ok(document) => LinkUpdate::Marked(Box::new(document)),
            Err(error) => LinkUpdate::Failed(error.to_string()),
        },
        // The service collects under its own configuration. Refusing here is
        // the honest answer; pretending the toggle worked would leave the user
        // believing command lines had stopped being collected.
        LinkRequest::SetPrivacy(_) => {
            LinkUpdate::Failed("monitor.gui.link.privacy_owned_by_service".to_string())
        }
    }
}

async fn handle_embedded(engine: &Arc<MonitorEngine>, request: LinkRequest) -> LinkUpdate {
    let now = monitor_service::now_unix_ms();
    match request {
        LinkRequest::History { seconds } => {
            let range = monitor_store::TimeRange::last(seconds, now);
            let slice = engine
                .with_store(|store| store.slice(range, MAX_HISTORY_SAMPLES as usize))
                .await;
            let resolution_seconds = engine.config().retention.resolution_seconds;
            LinkUpdate::History(Box::new(HistoryDocument {
                slice,
                resolution_seconds,
            }))
        }
        LinkRequest::Incidents => {
            let incidents = engine.with_store(|store| store.incidents().to_vec()).await;
            LinkUpdate::Incidents(Box::new(IncidentsDocument { incidents }))
        }
        LinkRequest::Inventory => {
            let (inventory, captures, diff) = engine
                .with_store(|store| {
                    (
                        store.latest_inventory().cloned(),
                        store.inventory_records().len() as u32,
                        store.latest_inventory_diff(),
                    )
                })
                .await;
            LinkUpdate::Inventory(
                Box::new(InventoryDocument {
                    inventory,
                    captures,
                }),
                Box::new(InventoryDiffDocument { diff }),
            )
        }
        LinkRequest::Mark {
            note,
            before_seconds,
            after_seconds,
            about_pid,
        } => {
            let window = monitor_store::IncidentWindow {
                before_seconds,
                after_seconds,
            };
            if !window.is_valid() {
                return LinkUpdate::Failed("monitor.ipc.error.invalid_window".to_string());
            }
            match engine.mark(note.as_deref(), window, about_pid).await {
                Ok(id) => {
                    let document = engine
                        .with_store(|store| {
                            store.incident_window(id).map(|(incident, slice)| {
                                IncidentWindowDocument {
                                    incident: incident.clone(),
                                    slice,
                                }
                            })
                        })
                        .await;
                    match document {
                        Some(document) => LinkUpdate::Marked(Box::new(document)),
                        None => {
                            LinkUpdate::Failed("monitor.ipc.error.unknown_incident".to_string())
                        }
                    }
                }
                Err(error) => LinkUpdate::Failed(error.to_string()),
            }
        }
        LinkRequest::SetPrivacy(privacy) => {
            engine.set_privacy(privacy).await;
            LinkUpdate::Status(Box::new(engine.status(true).await))
        }
    }
}

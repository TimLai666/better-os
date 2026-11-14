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
    ExportDocument, HistoryDocument, IncidentsDocument, InventoryDiffDocument, InventoryDocument,
    StatusDocument,
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
    /// The window stopped collecting for itself and now reads the service.
    Connected,
    /// Neither the service nor an embedded engine could be started. There is
    /// nothing to show and the window says exactly that.
    Unavailable(String),
    /// A History reply, its charts already worked out on this thread.
    History(Box<crate::stored::HistoryView>),
    Incidents(Box<IncidentsDocument>),
    Inventory(Box<InventoryDocument>, Box<InventoryDiffDocument>),
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
    spawn_with(requests, updates, None);
}

/// Start the bridge, asking for only the `top` busiest processes per resource
/// when `top` is set. The taskbar popup draws five names per resource, and a
/// thousand processes a second would cost it more than everything it shows.
pub(crate) fn spawn_with(
    requests: smol::channel::Receiver<LinkRequest>,
    updates: smol::channel::Sender<LinkUpdate>,
    top: Option<u32>,
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
            runtime.block_on(serve(requests, updates, top));
        });
    if let Err(error) = started {
        eprintln!("monitor-gui: the collection thread could not be started: {error}");
    }
}

/// Which side is answering.
enum Backend {
    Service(MonitorClient),
    Embedded(Embedded),
    /// Only for the moment between letting one go and taking the other.
    Closed,
}

async fn serve(
    requests: smol::channel::Receiver<LinkRequest>,
    updates: smol::channel::Sender<LinkUpdate>,
    top: Option<u32>,
) {
    let mut backend = match connect().await {
        Ok(client) => Backend::Service(client),
        Err(detail) => {
            // No service. Collect in this process instead, and say so before
            // the first number appears, so nothing is ever shown without the
            // caveat attached.
            match start_embedded() {
                Ok(embedded) => {
                    if updates.send(LinkUpdate::Embedded(detail)).await.is_err() {
                        return;
                    }
                    Backend::Embedded(embedded)
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
    let mut polls_since_check = 0u32;
    // While collecting here, the service taking its name is the cue to hand
    // over: it may be waiting for the store this window holds.
    let mut owner_changes = match &backend {
        Backend::Embedded(_) => service_owner_changes().await,
        _ => None,
    };

    let mut poll = tokio::time::interval(Duration::from_millis(POLL_INTERVAL_MILLIS));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = poll.tick() => {
                match &backend {
                    Backend::Service(client) => match match top {
                        Some(top) => client.summary(top).await,
                        None => client.status(true).await,
                    } {
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
                    Backend::Closed => {}
                    Backend::Embedded(embedded) => {
                        let engine = &embedded.engine;
                        if let Err(error) = engine.tick().await {
                            let _ = updates.send(LinkUpdate::Failed(error.to_string())).await;
                        }
                        let mut status = engine.status(true).await;
                        if let (Some(top), Some(round)) = (top, status.latest_round.as_mut()) {
                            *round = monitor_ipc::trim_round(round, top as usize);
                        }
                        if updates
                            .send(LinkUpdate::Status(Box::new(status)))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                // Collecting here while the service may come up: hand over to
                // it once Better Monitor is on. The store is let go first, so
                // a service waiting for it can open it.
                polls_since_check += 1;
                let due = match &backend {
                    Backend::Embedded(embedded) if polls_since_check >= HANDOVER_CHECK_POLLS => {
                        polls_since_check = 0;
                        handover_due(embedded).await
                    }
                    _ => false,
                };
                {
                    if due
                        && let Backend::Embedded(embedded) =
                            std::mem::replace(&mut backend, Backend::Closed)
                    {
                        embedded.finish().await;
                        match connect_with_retry().await {
                            Ok(client) => {
                                backend = Backend::Service(client);
                                if updates.send(LinkUpdate::Connected).await.is_err() {
                                    return;
                                }
                            }
                            Err(detail) => {
                                // The service did not come after all: collect
                                // here again.
                                match start_embedded() {
                                    Ok(embedded) => backend = Backend::Embedded(embedded),
                                    Err(error) => {
                                        let _ = updates
                                            .send(LinkUpdate::Unavailable(format!(
                                                "{detail} / {error}"
                                            )))
                                            .await;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            change = next_owner_change(&mut owner_changes) => {
                match change {
                    // Checked at the next poll, within a second.
                    Some(true) => polls_since_check = HANDOVER_CHECK_POLLS,
                    Some(false) => {}
                    None => owner_changes = None,
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
    if let Backend::Embedded(embedded) = backend {
        embedded.finish().await;
    }
}

/// How many polls pass between checks for whether to hand over to the
/// service: five seconds.
const HANDOVER_CHECK_POLLS: u32 = 5;

/// The engine this window collects with, and the separate store it opened
/// when it could not use the shared one.
pub(crate) struct Embedded {
    engine: Arc<MonitorEngine>,
    private_root: Option<std::path::PathBuf>,
    /// Whether Better Monitor was on when this window started collecting.
    component_was_on: bool,
}

impl Embedded {
    /// Flush, let go of the store, and remove a private store.
    async fn finish(self) {
        let _ = self.engine.shutdown().await;
        drop(self.engine);
        if let Some(root) = self.private_root {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// Where a window collecting for itself keeps its store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EmbeddedStore {
    /// The shared store: Better Monitor is off, so no service will want it.
    Shared,
    /// A store of this window's own, because the service may start at any
    /// moment and must find the shared store free.
    Private,
}

/// Decide where the embedded engine writes.
pub(crate) fn embedded_store(component_on: bool) -> EmbeddedStore {
    if component_on {
        EmbeddedStore::Private
    } else {
        EmbeddedStore::Shared
    }
}

/// The directory a window's private stores go in, under the person's own
/// runtime directory, which only they can open.
const PRIVATE_PARENT: &str = "better-monitor";
/// How a private store's directory is named: this, the window's process id,
/// a dash, and random letters, so a later window can tell whose it was.
const PRIVATE_PREFIX: &str = "better-monitor-window-";

/// Make a private store directory, readable by this user only (0700), with
/// a name nobody can guess beforehand: under `$XDG_RUNTIME_DIR` when there
/// is one, else in the temporary directory. Leftovers of windows of this
/// user that crashed are removed first.
pub(crate) fn create_private_root(
    runtime: Option<&std::path::Path>,
    temporary: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    let parent = runtime
        .filter(|runtime| runtime.is_absolute() && runtime.is_dir())
        .and_then(|runtime| {
            let parent = runtime.join(PRIVATE_PARENT);
            let made = std::fs::DirBuilder::new().mode(0o700).create(&parent);
            match made {
                Ok(()) => Some(parent),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    owned_directory(&parent).then_some(parent)
                }
                Err(_) => None,
            }
        })
        .unwrap_or_else(|| temporary.to_path_buf());
    sweep_private_roots(&parent, |pid| {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    });
    let directory = tempfile::Builder::new()
        .prefix(&format!("{PRIVATE_PREFIX}{}-", std::process::id()))
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(&parent)?;
    Ok(directory.keep())
}

/// The user this process runs as, from the owner of `/proc/self`.
fn own_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// A real directory (not a link) that this user owns.
fn owned_directory(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.file_type().is_dir() && Some(m.uid()) == own_uid())
}

/// Remove the private stores in `parent` that windows of this user left
/// when they ended without cleaning up: the process named in the directory's
/// name is gone. Another user's directories, links, and anything named
/// otherwise are left alone.
pub(crate) fn sweep_private_roots(parent: &std::path::Path, alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix(PRIVATE_PREFIX))
            .and_then(|rest| rest.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<u32>().ok())
        else {
            continue;
        };
        let path = entry.path();
        if pid != std::process::id() && !alive(pid) && owned_directory(&path) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

fn component_is_on() -> bool {
    crate::startup::component_on(
        std::path::Path::new(SERVICE_UNIT_PATH).exists(),
        crate::startup::unit_enabled(
            &crate::startup::RealSystemctl::from_environment(),
            SERVICE_UNIT,
        ),
    )
}

/// Start collecting in this window: on the shared store when Better Monitor
/// is off and nobody else has it open, otherwise on a private one.
fn start_embedded() -> Result<Embedded, monitor_store::StoreError> {
    let mut config = ServiceConfig::system();
    let on = component_is_on();
    if embedded_store(on) == EmbeddedStore::Shared {
        match MonitorEngine::start(config.clone()) {
            Ok(engine) => {
                return Ok(Embedded {
                    engine,
                    private_root: None,
                    component_was_on: on,
                });
            }
            Err(monitor_store::StoreError::Locked { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    let root =
        create_private_root(runtime.as_deref(), &std::env::temp_dir()).map_err(|source| {
            monitor_store::StoreError::Io {
                path: std::env::temp_dir(),
                source,
            }
        })?;
    config.store_root = root.clone();
    config.legacy_store_root = None;
    let engine = MonitorEngine::start(config).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&root);
    })?;
    Ok(Embedded {
        engine,
        private_root: Some(root),
        component_was_on: on,
    })
}

/// Whether to stop collecting here and use the service: its service is on
/// the bus, or Better Monitor has been switched on since this window started
/// collecting. A component that was on all along with no service answering
/// (a service that keeps failing) is not handed over to every few seconds,
/// which would throw the window's own minute of readings away each time.
pub(crate) fn handover(
    component_was_on: bool,
    component_on: bool,
    service_running: Option<bool>,
) -> bool {
    service_running == Some(true) || (component_on && !component_was_on)
}

async fn handover_due(embedded: &Embedded) -> bool {
    handover(
        embedded.component_was_on,
        component_is_on(),
        service_running().await,
    )
}

/// Connect, giving a service that is waiting for the store a few seconds.
async fn connect_with_retry() -> Result<MonitorClient, String> {
    let mut last = String::new();
    for _ in 0..10 {
        match connect().await {
            Ok(client) => return Ok(client),
            Err(detail) => last = detail,
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(last)
}

/// Where the package installs the service's user unit.
pub(crate) const SERVICE_UNIT: &str = "better-monitor.service";
pub(crate) const SERVICE_UNIT_PATH: &str = "/usr/lib/systemd/user/better-monitor.service";

/// What the window does about the service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServicePlan {
    /// Talk to it, starting it through D-Bus activation if it is not running.
    Connect,
    /// Better Monitor is switched off and the service is not running: do not
    /// start it, collect in this window instead.
    CollectHere,
}

/// Decide from whether the component is on and whether the service already
/// owns its bus name. A service that is running anyway is used even when the
/// component is off; what a switched-off component must never do is start.
pub(crate) fn service_plan(component_on: bool, service_running: Option<bool>) -> ServicePlan {
    if component_on || service_running == Some(true) {
        ServicePlan::Connect
    } else {
        ServicePlan::CollectHere
    }
}

/// Changes of who owns the service's bus name, on the session bus. `None`
/// when there is no bus to ask; the periodic check still runs.
async fn service_owner_changes() -> Option<zbus::fdo::NameOwnerChangedStream> {
    let connection = zbus::Connection::session().await.ok()?;
    let bus = zbus::fdo::DBusProxy::new(&connection).await.ok()?;
    bus.receive_name_owner_changed_with_args(&[(0, monitor_ipc::BUS_NAME)])
        .await
        .ok()
}

/// The next change of the service's name: `Some(true)` when it gained an
/// owner, `Some(false)` when it lost one, `None` when the stream ended. With
/// no stream it never resolves.
async fn next_owner_change(
    changes: &mut Option<zbus::fdo::NameOwnerChangedStream>,
) -> Option<bool> {
    use smol::stream::StreamExt as _;
    match changes {
        Some(stream) => {
            let signal = stream.next().await?;
            Some(signal.args().is_ok_and(|args| args.new_owner().is_some()))
        }
        None => std::future::pending().await,
    }
}

async fn service_running() -> Option<bool> {
    let connection = zbus::Connection::session().await.ok()?;
    let bus = zbus::fdo::DBusProxy::new(&connection).await.ok()?;
    let name = zbus::names::BusName::try_from(monitor_ipc::BUS_NAME).ok()?;
    bus.name_has_owner(name).await.ok()
}

async fn connect() -> Result<MonitorClient, String> {
    let on = crate::startup::component_on(
        std::path::Path::new(SERVICE_UNIT_PATH).exists(),
        crate::startup::unit_enabled(
            &crate::startup::RealSystemctl::from_environment(),
            SERVICE_UNIT,
        ),
    );
    if service_plan(on, service_running().await) == ServicePlan::CollectHere {
        return Err("monitor.gui.link.component_disabled".to_string());
    }
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
        Backend::Embedded(embedded) => handle_embedded(&embedded.engine, request).await,
        Backend::Closed => LinkUpdate::Failed("monitor.gui.link.handing_over".to_string()),
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
                Ok(document) => LinkUpdate::History(Box::new(crate::stored::HistoryView::build(
                    document,
                    range.to_unix_ms,
                    seconds,
                ))),
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
            LinkUpdate::History(Box::new(crate::stored::HistoryView::build(
                HistoryDocument {
                    slice,
                    resolution_seconds,
                },
                range.to_unix_ms,
                seconds,
            )))
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
        LinkRequest::SetPrivacy(privacy) => {
            engine.set_privacy(privacy).await;
            LinkUpdate::Status(Box::new(engine.status(true).await))
        }
    }
}

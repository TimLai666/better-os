//! The thread that talks to the storage layer.
//!
//! `monitor-gui` established this shape and it is copied deliberately: one
//! thread, a tokio runtime inside it, a `Backend` that is either the session
//! service or the same engine running in this process, and a window that is
//! told which one it got before it draws a single state.
//!
//! Two differences from Monitor, both forced by what storage is.
//!
//! **The embedded backend needs UDisks2.** The state machine can run here, but
//! the events it consumes come from a system service this process does not own.
//! When UDisks2 is unreachable as well, there is no third fallback and the
//! window reports [`CollectionMode::Unavailable`] rather than showing rows with
//! invented states.
//!
//! **An embedded backend is a worse promise, not an equal one.** Monitor's
//! embedded engine loses history when the window closes. Storage's loses the
//! tracked-operation signal for every write that another application makes and
//! this process never sees, which is why the note is drawn as a warning.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use storage_platform::UDisks2;
use storage_platform::model::PlatformEvent;
use storage_service::coordinator::Clock;
use storage_service::protocol::{DeviceReport, StateReport};
use storage_service::{PreferenceStore, StorageClient, StorageCoordinator};

use crate::devices::{CollectionMode, DeviceLink, DeviceNotice, UnsafeRemoval};

/// How often the link re-reads the inventory.
///
/// Device state is event-driven on both sides, so this is a safety net for a
/// missed signal rather than the mechanism. Two seconds is far below the
/// five-minute proof age the readiness rule allows and far above anything that
/// would show up as load.
const POLL_INTERVAL: Duration = Duration::from_millis(2_000);

/// What the window asks for.
enum Request {
    Mount(String),
    Eject(String),
    Refresh,
}

/// The production link.
pub struct StorageLink {
    requests: Sender<Request>,
    notices: Mutex<Receiver<DeviceNotice>>,
    mode: Arc<RwLock<CollectionMode>>,
    stop: Arc<AtomicBool>,
}

impl StorageLink {
    /// Starts the thread. Returns immediately; the mode is `Connecting` until
    /// the thread has decided.
    pub fn start() -> Self {
        let (requests, request_rx) = channel::<Request>();
        let (notice_tx, notices) = channel::<DeviceNotice>();
        let mode = Arc::new(RwLock::new(CollectionMode::Connecting));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_mode = mode.clone();
        let thread_stop = stop.clone();
        let started = std::thread::Builder::new()
            .name("files-storage-link".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        set_mode(
                            &thread_mode,
                            &notice_tx,
                            CollectionMode::Unavailable {
                                detail: error.to_string(),
                            },
                        );
                        return;
                    }
                };
                runtime.block_on(serve(request_rx, notice_tx, thread_mode, thread_stop));
            });
        if let Err(error) = started {
            *mode.write().expect("mode lock") = CollectionMode::Unavailable {
                detail: error.to_string(),
            };
        }

        Self {
            requests,
            notices: Mutex::new(notices),
            mode,
            stop,
        }
    }
}

impl DeviceLink for StorageLink {
    fn mode(&self) -> CollectionMode {
        self.mode.read().expect("mode lock").clone()
    }

    fn request_mount(&self, object_path: &str) {
        let _ = self.requests.send(Request::Mount(object_path.to_string()));
    }

    fn request_eject(&self, object_path: &str) {
        let _ = self.requests.send(Request::Eject(object_path.to_string()));
    }

    fn request_refresh(&self) {
        let _ = self.requests.send(Request::Refresh);
    }

    fn poll(&self) -> Vec<DeviceNotice> {
        let notices = self.notices.lock().expect("notice lock");
        let mut collected = Vec::new();
        loop {
            match notices.try_recv() {
                Ok(notice) => collected.push(notice),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        collected
    }
}

impl Drop for StorageLink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn set_mode(
    slot: &Arc<RwLock<CollectionMode>>,
    notices: &Sender<DeviceNotice>,
    mode: CollectionMode,
) {
    *slot.write().expect("mode lock") = mode.clone();
    let _ = notices.send(DeviceNotice::Mode(mode));
}

/// Which side is answering.
enum Backend {
    Service(StorageClient),
    Embedded {
        coordinator: StorageCoordinator<UDisks2>,
        events: tokio::sync::mpsc::UnboundedReceiver<PlatformEvent>,
    },
}

async fn serve(
    requests: Receiver<Request>,
    notices: Sender<DeviceNotice>,
    mode: Arc<RwLock<CollectionMode>>,
    stop: Arc<AtomicBool>,
) {
    let mut backend = match StorageClient::connect_verified().await {
        Ok(client) => {
            set_mode(&mode, &notices, CollectionMode::Service);
            Backend::Service(client)
        }
        Err(detail) => {
            // No session service. Run the same state machine here, and say so
            // before the first state appears so nothing is ever shown without
            // the caveat attached.
            match start_embedded().await {
                Ok(backend) => {
                    set_mode(
                        &mode,
                        &notices,
                        CollectionMode::InProcess {
                            detail: detail.to_string(),
                        },
                    );
                    backend
                }
                Err(error) => {
                    set_mode(
                        &mode,
                        &notices,
                        CollectionMode::Unavailable {
                            detail: format!("{detail} / {error}"),
                        },
                    );
                    return;
                }
            }
        }
    };

    let mut previous: Vec<DeviceReport> = Vec::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        // Requests first, so a click is acted on this tick rather than the
        // next one.
        let mut had_request = false;
        loop {
            match requests.try_recv() {
                Ok(request) => {
                    had_request = true;
                    handle(&mut backend, request, &notices).await;
                }
                Err(TryRecvError::Empty) => break,
                // The window is gone.
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if let Backend::Embedded {
            coordinator,
            events,
        } = &mut backend
        {
            while let Ok(event) = events.try_recv() {
                coordinator.handle_event(event).await;
            }
        }

        let reports = match &mut backend {
            Backend::Service(client) => match client.list_devices().await {
                Ok(reports) => reports,
                Err(error) => {
                    // The service went away mid-session. The window is told
                    // rather than left showing the last states it happened to
                    // have, which would be a readiness claim with nothing
                    // behind it.
                    set_mode(
                        &mode,
                        &notices,
                        CollectionMode::Unavailable {
                            detail: error.to_string(),
                        },
                    );
                    return;
                }
            },
            Backend::Embedded { coordinator, .. } => coordinator.reports(),
        };

        for notice in differences(&previous, &reports) {
            if notices.send(notice).is_err() {
                return;
            }
        }
        if reports != previous {
            if notices
                .send(DeviceNotice::Inventory(reports.clone()))
                .is_err()
            {
                return;
            }
            previous = reports;
        }

        if !had_request {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

async fn start_embedded() -> Result<Backend, String> {
    let udisks = UDisks2::connect().await.map_err(|e| e.to_string())?;
    let roots = storage_platform::Roots::system();
    let mut coordinator = StorageCoordinator::new(
        udisks.clone(),
        Arc::new(storage_platform::LinuxFlush),
        Arc::new(storage_platform::writeback::LinuxWriteback::new(
            roots.clone(),
        )),
        Arc::new(storage_platform::ProcOpenUse::new(roots)),
        PreferenceStore::from_default_path(),
        Clock::session(),
    )
    .map_err(|e| e.to_string())?;

    let (sender, events) = tokio::sync::mpsc::unbounded_channel();
    // Watching starts before the first inventory, so a device that arrives
    // during startup queues rather than being missed.
    udisks.watch(sender).await.map_err(|e| e.to_string())?;
    coordinator
        .refresh_inventory()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Backend::Embedded {
        coordinator,
        events,
    })
}

async fn handle(backend: &mut Backend, request: Request, notices: &Sender<DeviceNotice>) {
    match request {
        Request::Mount(object_path) => {
            let result = match backend {
                Backend::Service(client) => client
                    .mount(&object_path)
                    .await
                    .map(PathBuf::from)
                    .map_err(|e| e.to_string()),
                Backend::Embedded { coordinator, .. } => coordinator
                    .mount(&storage_core::DeviceHandle::new(object_path.clone()))
                    .await
                    .map_err(|e| e.to_string()),
            };
            let notice = match result {
                Ok(mount_point) => DeviceNotice::Mounted {
                    object_path,
                    mount_point,
                },
                Err(detail) => DeviceNotice::MountFailed {
                    object_path,
                    detail,
                },
            };
            let _ = notices.send(notice);
        }
        Request::Eject(object_path) => {
            let notice = match backend {
                Backend::Service(client) => match client.eject(&object_path).await {
                    Ok(report) => DeviceNotice::Ejected {
                        object_path,
                        unmounted: report.unmounted,
                        powered_off: report.powered_off,
                    },
                    Err(error) => DeviceNotice::EjectFailed {
                        object_path,
                        detail: error.to_string(),
                    },
                },
                Backend::Embedded { coordinator, .. } => match coordinator
                    .eject(&storage_core::DeviceHandle::new(object_path.clone()))
                    .await
                {
                    Ok(outcome) => DeviceNotice::Ejected {
                        object_path,
                        unmounted: outcome.unmounted,
                        powered_off: outcome.powered_off,
                    },
                    Err(error) => DeviceNotice::EjectFailed {
                        object_path,
                        detail: error.to_string(),
                    },
                },
            };
            let _ = notices.send(notice);
        }
        Request::Refresh => match backend {
            Backend::Service(client) => {
                let _ = client.refresh().await;
            }
            Backend::Embedded { coordinator, .. } => {
                let _ = coordinator.refresh_inventory().await;
            }
        },
    }
}

/// The devices that left between two inventories, and how they left.
///
/// A disconnect is derived here rather than being waited for as a signal,
/// because both backends report the inventory and only one of them emits a
/// signal. The unsafe-removal record travels with it: a device that was
/// removed mid-write says so in its final state, and that state has to reach
/// the window before the row is dropped.
fn differences(previous: &[DeviceReport], current: &[DeviceReport]) -> Vec<DeviceNotice> {
    previous
        .iter()
        .filter(|old| !current.iter().any(|now| now.object_path == old.object_path))
        .map(|gone| DeviceNotice::Disconnected {
            object_path: gone.object_path.clone(),
            unsafe_removal: unsafe_removal_of(&gone.state),
        })
        .collect()
}

fn unsafe_removal_of(state: &StateReport) -> Option<UnsafeRemoval> {
    match state {
        StateReport::Disconnected {
            unsafe_removal: Some(record),
        } => Some(UnsafeRemoval {
            previous_state: record.previous_state.clone(),
            unfinished_operations: record.unfinished_operations.clone(),
            recommend_filesystem_check: record.recommend_filesystem_check,
        }),
        // A device that vanished from the inventory while it was writing is an
        // unsafe removal even when no final state was recorded for it, which is
        // what an abrupt unplug looks like from this side.
        StateReport::Writing { reason, detail } => Some(UnsafeRemoval {
            previous_state: reason.clone(),
            unfinished_operations: if detail.is_empty() {
                Vec::new()
            } else {
                vec![detail.clone()]
            },
            recommend_filesystem_check: true,
        }),
        _ => None,
    }
}

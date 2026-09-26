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
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use storage_platform::UDisks2;
use storage_platform::model::PlatformEvent;
use storage_platform::traits::DeviceControl;
use storage_service::coordinator::Clock;
use storage_service::protocol::{DeviceReport, StateReport};
use storage_service::{PreferenceStore, StorageClient, StorageCoordinator};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::devices::{CollectionMode, DeviceLink, DeviceNotice, MountedDevice, UnsafeRemoval};
use crate::policy::PolicyRequest;

/// How often the link re-reads the inventory.
///
/// Device state is event-driven on both sides, so this is a safety net for a
/// missed signal rather than the mechanism. Two seconds is far below the
/// five-minute proof age the readiness rule allows and far above anything that
/// would show up as load.
const POLL_INTERVAL: Duration = Duration::from_millis(2_000);

/// How long a job waits for its started notice to reach the storage layer.
///
/// A D-Bus round trip is a millisecond. The wait is long only when the service
/// is busy flushing another job's writes, and past this the job goes ahead and
/// the failure is logged: a notice that arrives late still holds the device
/// for the rest of the job, and a job that never starts helps nobody.
const STARTED_WAIT: Duration = Duration::from_secs(2);

/// What the window, or a job's worker thread, asks for.
enum Request {
    Mount(String),
    Eject(String),
    Refresh,
    OperationStarted {
        object_path: String,
        operation: String,
        done: Sender<Result<(), String>>,
    },
    OperationCompleted {
        object_path: String,
        operation: String,
    },
    SetPolicy(PolicyRequest),
}

/// The production link.
pub struct StorageLink {
    requests: UnboundedSender<Request>,
    notices: Mutex<Receiver<DeviceNotice>>,
    mode: Arc<RwLock<CollectionMode>>,
    /// The mounted devices from the last inventory, for the job tracker.
    mounted: Arc<RwLock<Vec<MountedDevice>>>,
    stop: Arc<AtomicBool>,
}

impl StorageLink {
    /// Starts the thread. Returns immediately; the mode is `Connecting` until
    /// the thread has decided.
    pub fn start() -> Self {
        let (requests, request_rx) = unbounded_channel::<Request>();
        let (notice_tx, notices) = channel::<DeviceNotice>();
        let mode = Arc::new(RwLock::new(CollectionMode::Connecting));
        let mounted = Arc::new(RwLock::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_mode = mode.clone();
        let thread_mounted = mounted.clone();
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
                runtime.block_on(serve(
                    request_rx,
                    notice_tx,
                    thread_mode,
                    thread_mounted,
                    thread_stop,
                ));
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
            mounted,
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
        // A disconnected channel means the link thread has ended, which from a
        // caller's side is the same "nothing more is coming" as an empty one.
        while let Ok(notice) = notices.try_recv() {
            collected.push(notice);
        }
        collected
    }

    fn mounted_devices(&self) -> Vec<MountedDevice> {
        self.mounted.read().expect("mounted lock").clone()
    }

    fn operation_started(&self, object_path: &str, operation: &str) -> Result<(), String> {
        let (done, answer) = channel();
        self.requests
            .send(Request::OperationStarted {
                object_path: object_path.to_string(),
                operation: operation.to_string(),
                done,
            })
            .map_err(|_| "the storage link has stopped".to_string())?;
        match answer.recv_timeout(STARTED_WAIT) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
                "no answer within {} ms; the notice stays queued and is sent late",
                STARTED_WAIT.as_millis()
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err("the storage link stopped before answering".to_string())
            }
        }
    }

    fn request_policy(&self, request: PolicyRequest) {
        let _ = self.requests.send(Request::SetPolicy(request));
    }

    fn operation_completed(&self, object_path: &str, operation: &str) -> Result<(), String> {
        self.requests
            .send(Request::OperationCompleted {
                object_path: object_path.to_string(),
                operation: operation.to_string(),
            })
            .map_err(|_| "the storage link has stopped".to_string())
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
///
/// The embedded arm is boxed because it carries a whole coordinator and its
/// event receiver, and this enum lives for the life of the thread either way —
/// paying for the larger variant in the service case would be paying for
/// something that is never used. It is generic over the platform so a test can
/// drive the embedded arm with `storage-platform`'s fake.
enum Backend<C: DeviceControl> {
    Service(StorageClient),
    Embedded(Box<Embedded<C>>),
}

struct Embedded<C: DeviceControl> {
    coordinator: StorageCoordinator<C>,
    events: UnboundedReceiver<PlatformEvent>,
}

async fn serve(
    mut requests: UnboundedReceiver<Request>,
    notices: Sender<DeviceNotice>,
    mode: Arc<RwLock<CollectionMode>>,
    mounted: Arc<RwLock<Vec<MountedDevice>>>,
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
        loop {
            match requests.try_recv() {
                Ok(request) => handle(&mut backend, request, &notices).await,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                // Every sender is gone: the window and the job tracker both.
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return,
            }
        }

        if let Backend::Embedded(embedded) = &mut backend {
            while let Ok(event) = embedded.events.try_recv() {
                embedded.coordinator.handle_event(event).await;
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
            Backend::Embedded(embedded) => embedded.coordinator.reports(),
        };

        for notice in differences(&previous, &reports) {
            if notices.send(notice).is_err() {
                return;
            }
        }
        if reports != previous {
            *mounted.write().expect("mounted lock") = mounted_from(&reports);
            if notices
                .send(DeviceNotice::Inventory(reports.clone()))
                .is_err()
            {
                return;
            }
            previous = reports;
        }

        // Sleep until the next poll, or until something is asked for. A job
        // waiting on its started notice must not wait out a poll interval.
        match tokio::time::timeout(POLL_INTERVAL, requests.recv()).await {
            Ok(Some(request)) => handle(&mut backend, request, &notices).await,
            Ok(None) => return,
            Err(_) => {}
        }
    }
}

/// The mounted devices in an inventory.
fn mounted_from(reports: &[DeviceReport]) -> Vec<MountedDevice> {
    reports
        .iter()
        .filter_map(|report| {
            report
                .mount_point
                .as_ref()
                .map(|mount_point| MountedDevice {
                    object_path: report.object_path.clone(),
                    mount_point: PathBuf::from(mount_point),
                })
        })
        .collect()
}

async fn start_embedded() -> Result<Backend<UDisks2>, String> {
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
    Ok(Backend::Embedded(Box::new(Embedded {
        coordinator,
        events,
    })))
}

async fn handle<C: DeviceControl>(
    backend: &mut Backend<C>,
    request: Request,
    notices: &Sender<DeviceNotice>,
) {
    match request {
        Request::Mount(object_path) => {
            let result = match backend {
                Backend::Service(client) => client
                    .mount(&object_path)
                    .await
                    .map(PathBuf::from)
                    .map_err(|e| e.to_string()),
                Backend::Embedded(embedded) => embedded
                    .coordinator
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
                Backend::Embedded(embedded) => match embedded
                    .coordinator
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
            Backend::Embedded(embedded) => {
                let _ = embedded.coordinator.refresh_inventory().await;
            }
        },
        Request::OperationStarted {
            object_path,
            operation,
            done,
        } => {
            let result = match backend {
                Backend::Service(client) => client
                    .notify_operation_started(&object_path, &operation)
                    .await
                    .map_err(|error| error.to_string()),
                Backend::Embedded(embedded) => {
                    embedded
                        .coordinator
                        .operation_started(&storage_core::DeviceHandle::new(object_path), operation)
                        .await;
                    Ok(())
                }
            };
            // The job may have stopped waiting; a late answer has nobody to
            // go to, and the notice itself has still been delivered.
            let _ = done.send(result);
        }
        Request::OperationCompleted {
            object_path,
            operation,
        } => match backend {
            // The service flushes the filesystem before it answers, which can
            // take as long as the disk needs. The call runs as its own task so
            // this thread keeps serving the window meanwhile.
            Backend::Service(client) => {
                let client = client.clone();
                tokio::spawn(async move {
                    if let Err(error) = client
                        .notify_operation_completed(&object_path, &operation)
                        .await
                    {
                        eprintln!(
                            "better-files: the storage service was not told that {operation} finished on {object_path}: {error}"
                        );
                    }
                });
            }
            // The in-process engine flushes here, on this thread. It holds
            // the coordinator, so nothing else can run until it is done.
            Backend::Embedded(embedded) => {
                embedded
                    .coordinator
                    .operation_completed(&storage_core::DeviceHandle::new(object_path), operation)
                    .await;
            }
        },
        Request::SetPolicy(request) => {
            let object_path = request.object_path.clone();
            let result = match backend {
                Backend::Service(client) => client
                    .set_policy(
                        &request.object_path,
                        request.policy,
                        request.acknowledged_risks,
                    )
                    .await
                    .map_err(|error| match error {
                        storage_service::ClientError::Rejected(detail) => detail,
                        other => other.to_string(),
                    }),
                Backend::Embedded(embedded) => embedded
                    .coordinator
                    .set_policy(
                        &storage_core::DeviceHandle::new(request.object_path),
                        request.policy,
                        request.acknowledged_risks,
                    )
                    .await
                    .map_err(|error| error.to_string()),
            };
            let _ = notices.send(match result {
                Ok(()) => DeviceNotice::PolicyApplied { object_path },
                Err(detail) => DeviceNotice::PolicyRefused {
                    object_path,
                    detail,
                },
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use storage_core::{DeviceHandle, DeviceStateKind};
    use storage_platform::fake::{
        FakeDeviceControl, FakeFlush, FakeOpenUse, FakeWriteback, usb_stick,
    };

    const OBJECT: &str = "/org/freedesktop/UDisks2/block_devices/sdb1";

    fn kind(backend: &Backend<FakeDeviceControl>) -> DeviceStateKind {
        match backend {
            Backend::Embedded(embedded) => embedded
                .coordinator
                .report(&DeviceHandle::new(OBJECT))
                .expect("the device")
                .state
                .kind(),
            Backend::Service(_) => unreachable!("this test runs the embedded engine"),
        }
    }

    /// With no service, the engine in this process is the one that has to hear
    /// about Better Files' own writes, or its readiness answer would differ
    /// from the service's for the same copy.
    #[tokio::test]
    async fn the_in_process_engine_hears_the_operation_notices_a_service_would() {
        let directory = tempfile::tempdir().unwrap();
        let clock = Clock::manual();
        let mut coordinator = StorageCoordinator::new(
            FakeDeviceControl::new([usb_stick(OBJECT, "/dev/sdb1", "A1B2-C3D4")]),
            Arc::new(FakeFlush::default()),
            Arc::new(FakeWriteback::idle()),
            Arc::new(FakeOpenUse::idle()),
            PreferenceStore::at_path(directory.path().join("storage.json")),
            clock.clone(),
        )
        .unwrap();
        coordinator.refresh_inventory().await.unwrap();
        clock.advance(10);
        coordinator.mount(&DeviceHandle::new(OBJECT)).await.unwrap();
        let (_events_tx, events) = unbounded_channel();
        let mut backend = Backend::Embedded(Box::new(Embedded {
            coordinator,
            events,
        }));
        let (notices, _window) = channel();
        assert_eq!(kind(&backend), DeviceStateKind::ReadyToUnplug);

        let (done, answer) = channel();
        clock.advance(10);
        handle(
            &mut backend,
            Request::OperationStarted {
                object_path: OBJECT.to_string(),
                operation: "better-files:1:job-1".to_string(),
                done,
            },
            &notices,
        )
        .await;
        assert_eq!(
            answer.try_recv(),
            Ok(Ok(())),
            "the job was told to go ahead"
        );
        assert_eq!(kind(&backend), DeviceStateKind::Writing);

        clock.advance(10);
        handle(
            &mut backend,
            Request::OperationCompleted {
                object_path: OBJECT.to_string(),
                operation: "better-files:1:job-1".to_string(),
            },
            &notices,
        )
        .await;
        assert_eq!(kind(&backend), DeviceStateKind::ReadyToUnplug);
    }

    #[test]
    fn the_tracker_is_given_only_the_devices_that_are_mounted() {
        let report = |object_path: &str, mount_point: Option<&str>| DeviceReport {
            object_path: object_path.to_string(),
            device_path: "/dev/sdb1".to_string(),
            display_name: "USB".to_string(),
            identity: "uuid:x".to_string(),
            identity_confidence: "stable".to_string(),
            filesystem: None,
            mount_point: mount_point.map(str::to_string),
            policy: storage_core::RemovalPolicy::DirectRemoval,
            state: StateReport::Unknown {
                reason: "test".to_string(),
                detail: String::new(),
            },
        };
        assert_eq!(
            mounted_from(&[report("/a", Some("/media/a")), report("/b", None)]),
            vec![MountedDevice {
                object_path: "/a".to_string(),
                mount_point: PathBuf::from("/media/a"),
            }]
        );
    }

    async fn embedded_with_a_mounted_stick(
        store: &std::path::Path,
    ) -> (Backend<FakeDeviceControl>, Clock) {
        let clock = Clock::manual();
        let mut coordinator = StorageCoordinator::new(
            FakeDeviceControl::new([usb_stick(OBJECT, "/dev/sdb1", "A1B2-C3D4")]),
            Arc::new(FakeFlush::default()),
            Arc::new(FakeWriteback::idle()),
            Arc::new(FakeOpenUse::idle()),
            PreferenceStore::at_path(store),
            clock.clone(),
        )
        .unwrap();
        coordinator.refresh_inventory().await.unwrap();
        clock.advance(10);
        coordinator.mount(&DeviceHandle::new(OBJECT)).await.unwrap();
        // Nothing reads the platform events in these tests; `handle` is
        // driven directly.
        let (_, events) = unbounded_channel();
        (
            Backend::Embedded(Box::new(Embedded {
                coordinator,
                events,
            })),
            clock,
        )
    }

    fn performance(acknowledged: &[&str]) -> Request {
        Request::SetPolicy(PolicyRequest {
            object_path: OBJECT.to_string(),
            policy: storage_core::RemovalPolicy::Performance,
            acknowledged_risks: acknowledged.iter().map(|key| key.to_string()).collect(),
        })
    }

    /// With no service the in-process engine answers the policy request, and
    /// a refusal carries its reason back to the window.
    #[tokio::test]
    async fn the_in_process_engine_refuses_an_unacknowledged_request_and_applies_a_full_one() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path().join("storage.json");
        let (mut backend, _clock) = embedded_with_a_mounted_stick(&store).await;
        let (notices, window) = channel();

        handle(
            &mut backend,
            performance(&[storage_core::PERFORMANCE_RISK_KEYS[0]]),
            &notices,
        )
        .await;
        match window.try_recv() {
            Ok(DeviceNotice::PolicyRefused {
                object_path,
                detail,
            }) => {
                assert_eq!(object_path, OBJECT);
                assert!(detail.contains("acknowledgement"), "{detail}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(kind(&backend), DeviceStateKind::ReadyToUnplug);

        handle(
            &mut backend,
            performance(storage_core::PERFORMANCE_RISK_KEYS),
            &notices,
        )
        .await;
        assert_eq!(
            window.try_recv(),
            Ok(DeviceNotice::PolicyApplied {
                object_path: OBJECT.to_string(),
            })
        );
        assert_eq!(kind(&backend), DeviceStateKind::PerformanceMode);

        handle(
            &mut backend,
            Request::SetPolicy(PolicyRequest::direct_removal(OBJECT)),
            &notices,
        )
        .await;
        assert_eq!(
            window.try_recv(),
            Ok(DeviceNotice::PolicyApplied {
                object_path: OBJECT.to_string(),
            })
        );
        assert_ne!(kind(&backend), DeviceStateKind::PerformanceMode);
    }

    /// The window does not say an in-process choice lasts only while it is
    /// open, because it does not: the in-process engine writes the same
    /// preference file the service reads, and a later engine starts from it.
    #[tokio::test]
    async fn an_in_process_choice_outlives_the_window() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path().join("storage.json");
        {
            let (mut backend, _clock) = embedded_with_a_mounted_stick(&store).await;
            let (notices, _window) = channel();
            handle(
                &mut backend,
                performance(storage_core::PERFORMANCE_RISK_KEYS),
                &notices,
            )
            .await;
            assert_eq!(kind(&backend), DeviceStateKind::PerformanceMode);
        }
        let (backend, _clock) = embedded_with_a_mounted_stick(&store).await;
        assert_eq!(kind(&backend), DeviceStateKind::PerformanceMode);
    }
}

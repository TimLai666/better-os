//! Device state coordination, outside any GUI.
//!
//! The coordinator owns one `storage-core` state machine per connected device,
//! feeds it what `storage-platform` observes, carries out the effects the
//! machine asks for, and publishes typed reports to whoever is listening. It
//! holds no window and no user session assumptions beyond the session bus, so a
//! file manager can close and reopen without the device's state being lost or
//! re-derived from nothing.
//!
//! There is no polling loop anywhere in here. Work happens when a platform
//! event arrives, when a client says a file operation finished, or when a
//! machine asks for a refresh. An idle session with a stick plugged in runs no
//! timers at all.

use crate::protocol::{DeviceReport, StateReport};
use crate::store::{PreferenceStore, StoreError};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use storage_core::machine::ObservedSignals;
use storage_core::{
    DeviceEvent, DeviceHandle, DeviceIdentity, DeviceRegistry, Diagnostic, Effect, EvidencePolicy,
    PerformanceOptIn, PolicyError, PreferenceSet, RemovalPolicy, RestoreDefaultPlan, SignalStatus,
    Timestamp, Transition,
};
use storage_platform::traits::{
    DeviceControl, EjectOutcome, FlushBackend, OpenUseInspector, PlatformError, WritebackInspector,
};
use storage_platform::{PlatformDevice, PlatformEvent};
use thiserror::Error;
use tokio::sync::broadcast;

/// How many diagnostics are kept. Enough for a session's worth of unsafe
/// removals and flush failures without growing without bound.
const DIAGNOSTIC_CAPACITY: usize = 256;

/// How many rounds of machine-requested effects one event may cause. A flush
/// leads to an observation and an observation can lead to a flush, so the loop
/// is bounded rather than trusted to settle.
const EFFECT_BUDGET: u32 = 8;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("no device is known at {0}")]
    UnknownDevice(String),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The session clock. Monotonic in production; driven by hand in tests, so an
/// event sequence produces the same timestamps every run.
#[derive(Clone, Debug)]
pub enum Clock {
    Monotonic(Instant),
    Manual(Arc<Mutex<u64>>),
}

impl Clock {
    pub fn session() -> Self {
        Clock::Monotonic(Instant::now())
    }

    pub fn manual() -> Self {
        Clock::Manual(Arc::new(Mutex::new(0)))
    }

    /// Moves a manual clock forward. Does nothing to a monotonic one.
    pub fn advance(&self, millis: u64) {
        if let Clock::Manual(value) = self {
            *value.lock().expect("clock lock") += millis;
        }
    }

    pub fn now(&self) -> Timestamp {
        match self {
            Clock::Monotonic(origin) => Timestamp::from_duration(origin.elapsed()),
            Clock::Manual(value) => Timestamp::from_millis(*value.lock().expect("clock lock")),
        }
    }
}

pub struct StorageCoordinator<C: DeviceControl> {
    control: C,
    flush: Arc<dyn FlushBackend>,
    writeback: Arc<dyn WritebackInspector>,
    open_use: Arc<dyn OpenUseInspector>,
    registry: DeviceRegistry,
    preferences: PreferenceSet,
    store: PreferenceStore,
    clock: Clock,
    /// The last platform view of each connected device, so writeback can be
    /// inspected without another D-Bus round trip.
    known: BTreeMap<DeviceHandle, PlatformDevice>,
    diagnostics: VecDeque<Diagnostic>,
    updates: broadcast::Sender<DeviceReport>,
}

impl<C: DeviceControl> StorageCoordinator<C> {
    pub fn new(
        control: C,
        flush: Arc<dyn FlushBackend>,
        writeback: Arc<dyn WritebackInspector>,
        open_use: Arc<dyn OpenUseInspector>,
        store: PreferenceStore,
        clock: Clock,
    ) -> Result<Self, StoreError> {
        let loaded = store.load()?;
        let (updates, _) = broadcast::channel(256);
        Ok(Self {
            control,
            flush,
            writeback,
            open_use,
            registry: DeviceRegistry::new(EvidencePolicy::default()),
            preferences: loaded.preferences,
            store,
            clock,
            known: BTreeMap::new(),
            diagnostics: VecDeque::new(),
            updates,
        })
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DeviceReport> {
        self.updates.subscribe()
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    /// Every connected device, as clients see it.
    pub fn reports(&self) -> Vec<DeviceReport> {
        self.registry
            .iter()
            .map(|(handle, machine)| self.build_report(handle, machine))
            .collect()
    }

    pub fn report(&self, handle: &DeviceHandle) -> Option<DeviceReport> {
        self.registry
            .get(handle)
            .map(|machine| self.build_report(handle, machine))
    }

    /// Reads the whole inventory and reconciles it against what is held.
    ///
    /// Called once at start and after a UDisks2 restart. Not on a timer.
    pub async fn refresh_inventory(&mut self) -> Result<(), ServiceError> {
        let devices = self.control.enumerate().await?;
        let mut seen: Vec<DeviceHandle> = Vec::new();

        for device in devices {
            if !device.classify().is_external() {
                continue;
            }
            let handle = DeviceHandle::new(device.address.object_path.clone());
            seen.push(handle.clone());
            self.admit(handle, device).await;
        }

        let gone: Vec<DeviceHandle> = self
            .registry
            .iter()
            .map(|(handle, _)| handle.clone())
            .filter(|handle| !seen.contains(handle))
            .collect();
        for handle in gone {
            self.remove(&handle).await;
        }
        Ok(())
    }

    /// Handles one platform event.
    pub async fn handle_event(&mut self, event: PlatformEvent) {
        match event {
            PlatformEvent::Added(device) => {
                if device.classify().is_external() {
                    let handle = DeviceHandle::new(device.address.object_path.clone());
                    self.admit(handle, *device).await;
                }
            }
            PlatformEvent::Removed { address } => {
                self.remove(&DeviceHandle::new(address.object_path)).await;
            }
            PlatformEvent::MountChanged {
                address,
                mount_point,
            } => {
                let handle = DeviceHandle::new(address.object_path);
                if let Some(device) = self.known.get_mut(&handle) {
                    device.mount_point = mount_point.clone();
                }
                self.apply_mount(&handle, mount_point).await;
            }
            PlatformEvent::Changed { address } => {
                let handle = DeviceHandle::new(address.object_path.clone());
                if self.registry.get(&handle).is_none() {
                    return;
                }
                match self.control.read(&address).await {
                    Ok(device) => {
                        let mount_point = device.mount_point.clone();
                        self.known.insert(handle.clone(), device);
                        self.apply_mount(&handle, mount_point).await;
                    }
                    // The object is gone. That is a disconnect, whatever the
                    // signal said it was.
                    Err(_) => self.remove(&handle).await,
                }
            }
        }
    }

    /// Registers a device, or updates one already held.
    async fn admit(&mut self, handle: DeviceHandle, device: PlatformDevice) {
        let mount_point = device.mount_point.clone();
        let known_already = self.registry.get(&handle).is_some();
        self.known.insert(handle.clone(), device.clone());

        if !known_already {
            let identity = DeviceIdentity::from_evidence(device.identity_evidence());
            let now = self.clock.now();
            let transitions =
                self.registry
                    .connect(handle.clone(), identity, &self.preferences, now);
            for (affected, transition) in transitions {
                self.record(&affected, &transition);
            }
        }
        self.apply_mount(&handle, mount_point).await;
    }

    async fn apply_mount(&mut self, handle: &DeviceHandle, mount_point: Option<PathBuf>) {
        let now = self.clock.now();
        let currently = self
            .registry
            .get(handle)
            .and_then(|machine| machine.mount_point().map(PathBuf::from));
        let event = match (&mount_point, &currently) {
            (Some(new), current) if Some(new) != current.as_ref() => Some(DeviceEvent::Mounted {
                mount_point: new.to_string_lossy().to_string(),
            }),
            (None, Some(_)) => Some(DeviceEvent::Unmounted),
            (None, None)
                if self
                    .registry
                    .get(handle)
                    .is_some_and(|machine| !machine.is_mounted()) =>
            {
                // An unmounted device that has never been observed either way
                // needs to be told so, or it stays "not yet observed" forever.
                Some(DeviceEvent::Unmounted)
            }
            _ => None,
        };
        let Some(event) = event else {
            return;
        };
        if let Some(transition) = self.registry.apply(handle, event, now) {
            self.pump(handle, transition).await;
        }
    }

    async fn remove(&mut self, handle: &DeviceHandle) {
        let now = self.clock.now();
        let transitions = self.registry.disconnect(handle, now);
        for (affected, transition) in transitions {
            self.record(&affected, &transition);
        }
        self.known.remove(handle);
    }

    /// Tells every device that this process cannot vouch for the gap it just
    /// came back from.
    pub async fn notify_service_restarted(&mut self) {
        let now = self.clock.now();
        let transitions = self
            .registry
            .apply_to_all(DeviceEvent::ServiceRestarted, now);
        for (handle, transition) in transitions {
            self.pump(&handle, transition).await;
        }
    }

    /// Mount-on-open. The device stays mounted afterwards; leaving the folder
    /// does not unmount it.
    pub async fn mount(&mut self, handle: &DeviceHandle) -> Result<PathBuf, ServiceError> {
        let device = self
            .known
            .get(handle)
            .ok_or_else(|| ServiceError::UnknownDevice(handle.as_str().to_string()))?
            .clone();
        let mount_point = self.control.mount(&device.address).await?;
        if let Some(known) = self.known.get_mut(handle) {
            known.mount_point = Some(mount_point.clone());
        }
        self.apply_mount(handle, Some(mount_point.clone())).await;
        Ok(mount_point)
    }

    /// The explicit action, still supported and still required in Performance
    /// mode: unmount, then power the drive off where the platform allows it.
    pub async fn eject(&mut self, handle: &DeviceHandle) -> Result<EjectOutcome, ServiceError> {
        let device = self
            .known
            .get(handle)
            .ok_or_else(|| ServiceError::UnknownDevice(handle.as_str().to_string()))?
            .clone();
        let outcome = self.control.eject(&device.address).await?;
        if let Some(known) = self.known.get_mut(handle) {
            known.mount_point = None;
        }
        self.apply_mount(handle, None).await;
        Ok(outcome)
    }

    /// Changes a device's policy and persists it.
    ///
    /// Performance mode is refused unless the request carries an
    /// acknowledgement of every declared risk, and refused for a device that
    /// can only be named by its current kernel path.
    pub async fn set_policy(
        &mut self,
        handle: &DeviceHandle,
        policy: RemovalPolicy,
        acknowledged_risks: Vec<String>,
    ) -> Result<(), ServiceError> {
        let identity = self
            .registry
            .get(handle)
            .ok_or_else(|| ServiceError::UnknownDevice(handle.as_str().to_string()))?
            .identity()
            .clone();

        match policy {
            RemovalPolicy::Performance => {
                self.preferences.set_performance(
                    &identity,
                    PerformanceOptIn::acknowledging(acknowledged_risks),
                )?;
            }
            RemovalPolicy::DirectRemoval => self.preferences.set_direct_removal(&identity),
        }
        self.store.save(&self.preferences)?;

        let now = self.clock.now();
        if let Some(transition) =
            self.registry
                .apply(handle, DeviceEvent::PolicyChanged(policy), now)
        {
            self.pump(handle, transition).await;
        }
        Ok(())
    }

    /// Returns every device to Direct Removal and persists the result. This is
    /// what uninstalling the component runs, and what proves the plan was
    /// empty afterwards.
    pub async fn restore_defaults(&mut self) -> Result<RestoreDefaultPlan, ServiceError> {
        let plan = self.preferences.restore_defaults();
        self.store.save(&self.preferences)?;
        let handles: Vec<DeviceHandle> = self
            .registry
            .iter()
            .map(|(handle, _)| handle.clone())
            .collect();
        let now = self.clock.now();
        for handle in handles {
            if let Some(transition) = self.registry.apply(
                &handle,
                DeviceEvent::PolicyChanged(RemovalPolicy::DirectRemoval),
                now,
            ) {
                self.pump(&handle, transition).await;
            }
        }
        Ok(plan)
    }

    /// A file operation started. Called by Better Files, and by any future
    /// Better Copy, through the typed IPC surface.
    pub async fn operation_started(&mut self, handle: &DeviceHandle, operation: String) {
        let now = self.clock.now();
        if let Some(transition) =
            self.registry
                .apply(handle, DeviceEvent::OperationStarted { operation }, now)
        {
            self.pump(handle, transition).await;
        }
    }

    /// A file operation finished. The flush that follows is filesystem-scoped
    /// and happens here, once per operation, rather than per written file.
    pub async fn operation_completed(&mut self, handle: &DeviceHandle, operation: String) {
        let report = self.run_flush(handle).await;
        let now = self.clock.now();
        let Some(report) = report else {
            // Nothing mounted to flush. Still clear the operation.
            if let Some(transition) = self.registry.apply(
                handle,
                DeviceEvent::OperationCompleted {
                    operation,
                    flush: storage_core::FlushOutcome::Unsupported {
                        detail: "the volume is not mounted".to_string(),
                    },
                },
                now,
            ) {
                self.pump(handle, transition).await;
            }
            return;
        };
        if let Some(transition) = self.registry.apply(
            handle,
            DeviceEvent::OperationCompleted {
                operation,
                flush: report.into_outcome(now),
            },
            now,
        ) {
            self.pump(handle, transition).await;
        }
    }

    /// Reads the platform signals for one device and feeds them in.
    pub async fn observe(&mut self, handle: &DeviceHandle) {
        if let Some(transition) = self.observe_once(handle).await {
            self.pump(handle, transition).await;
        }
    }

    async fn observe_once(&mut self, handle: &DeviceHandle) -> Option<Transition> {
        let device = self.known.get(handle)?.clone();
        let mount_point = self
            .registry
            .get(handle)
            .and_then(|machine| machine.mount_point().map(PathBuf::from));

        let writeback_inspector = self.writeback.clone();
        let open_use_inspector = self.open_use.clone();
        let probe_device = device.clone();
        let probe_mount = mount_point.clone();
        // File I/O, so it runs off the async runtime's threads.
        let (writeback, open_writers) = tokio::task::spawn_blocking(move || {
            let writeback = writeback_inspector.pending(&probe_device);
            let open_writers = match &probe_mount {
                Some(mount) => open_use_inspector.open_writers(mount),
                None => SignalStatus::Unavailable {
                    detail: "the volume is not mounted".to_string(),
                },
            };
            (writeback, open_writers)
        })
        .await
        .ok()?;

        let now = self.clock.now();
        self.registry.apply(
            handle,
            DeviceEvent::SignalsObserved(ObservedSignals {
                at: now,
                mounted: mount_point.is_some(),
                writeback,
                open_writers,
            }),
            now,
        )
    }

    /// Flushes exactly one filesystem. Never `sync(2)`, and never the device
    /// the machine did not ask about.
    async fn run_flush(&mut self, handle: &DeviceHandle) -> Option<storage_platform::FlushReport> {
        let mount_point = self
            .registry
            .get(handle)
            .and_then(|machine| machine.mount_point().map(PathBuf::from))?;
        let backend = self.flush.clone();
        tokio::task::spawn_blocking(move || backend.flush_filesystem(&mount_point))
            .await
            .ok()
    }

    async fn flush_once(&mut self, handle: &DeviceHandle) -> Option<Transition> {
        let now = self.clock.now();
        let starting = self
            .registry
            .apply(handle, DeviceEvent::FlushStarted, now)?;
        self.record(handle, &starting);

        let report = self.run_flush(handle).await?;
        let now = self.clock.now();
        self.registry.apply(
            handle,
            match report.into_outcome(now) {
                storage_core::FlushOutcome::Completed(verification) => {
                    DeviceEvent::FlushCompleted(verification)
                }
                storage_core::FlushOutcome::Failed { detail } => {
                    DeviceEvent::FlushFailed { detail }
                }
                storage_core::FlushOutcome::Unsupported { detail } => {
                    DeviceEvent::FlushUnsupported { detail }
                }
            },
            now,
        )
    }

    /// Records a transition and carries out the effects it asked for, with a
    /// bounded number of follow-on rounds.
    async fn pump(&mut self, handle: &DeviceHandle, transition: Transition) {
        let mut queue = VecDeque::new();
        queue.push_back(transition);
        let mut budget = EFFECT_BUDGET;

        while let Some(transition) = queue.pop_front() {
            self.record(handle, &transition);
            if budget == 0 {
                continue;
            }
            budget -= 1;
            for effect in &transition.effects {
                match effect {
                    Effect::RequestSignalRefresh => {
                        if let Some(next) = self.observe_once(handle).await {
                            queue.push_back(next);
                        }
                    }
                    Effect::RequestFilesystemFlush => {
                        if let Some(next) = self.flush_once(handle).await {
                            queue.push_back(next);
                        }
                    }
                    // The service holds no navigation or sidebar state. The
                    // report published below is what tells a client to drop
                    // theirs.
                    Effect::ReleaseMountState => {}
                }
            }
        }
    }

    fn record(&mut self, handle: &DeviceHandle, transition: &Transition) {
        for diagnostic in &transition.diagnostics {
            if self.diagnostics.len() == DIAGNOSTIC_CAPACITY {
                self.diagnostics.pop_front();
            }
            self.diagnostics.push_back(diagnostic.clone());
        }
        if !transition.changed {
            return;
        }
        let report = match self.registry.get(handle) {
            Some(machine) => self.build_report(handle, machine),
            // A disconnected device has already left the registry, so its final
            // report — the one carrying an unsafe-removal record — is built
            // from the transition itself.
            None => DeviceReport {
                object_path: handle.as_str().to_string(),
                device_path: String::new(),
                display_name: String::new(),
                identity: transition
                    .diagnostics
                    .first()
                    .map(|diagnostic| diagnostic.identity.to_string())
                    .unwrap_or_default(),
                identity_confidence: String::new(),
                filesystem: None,
                mount_point: None,
                policy: RemovalPolicy::DirectRemoval,
                state: StateReport::from_state(&transition.state),
            },
        };
        let _ = self.updates.send(report);
    }

    fn build_report(
        &self,
        handle: &DeviceHandle,
        machine: &storage_core::DeviceMachine,
    ) -> DeviceReport {
        let identity = machine.identity();
        DeviceReport {
            object_path: handle.as_str().to_string(),
            device_path: identity.device_path().to_string(),
            display_name: identity.display_name(),
            identity: identity.key().to_string(),
            identity_confidence: format!("{:?}", identity.confidence()).to_lowercase(),
            filesystem: self
                .known
                .get(handle)
                .and_then(|device| device.block.id_type.clone()),
            mount_point: machine.mount_point().map(str::to_string),
            policy: machine.policy(),
            state: StateReport::from_state(machine.state()),
        }
    }
}

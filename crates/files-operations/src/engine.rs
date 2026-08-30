//! The job engine: a worker pool that owns its jobs.
//!
//! Issue #6's rule is one sentence — "do not tie file operations to one
//! window's lifetime" — and this module is what makes it true rather than
//! aspirational.
//!
//! A [`JobHandle`] is a receipt, not an owner. It carries a job identifier and
//! an event stream, it implements no `Drop`, and nothing in the engine watches
//! whether one still exists. Dropping every handle to a running copy does
//! exactly nothing to the copy. The engine is the owner, and only
//! [`JobEngine::cancel`] stops a job.
//!
//! ## Concurrency policy
//!
//! - **Jobs run in parallel, up to the worker count.** The default is two.
//!   More would help a job whose bottleneck is a network share and hurt every
//!   job whose bottleneck is a spinning disk, where two interleaved copies cost
//!   more in seeks than they gain in overlap.
//! - **Items within a job run in order, one at a time.** Reading and writing
//!   one file at a time is what makes throughput measurable, conflict
//!   decisions ordered, and the operation log a sequence rather than a
//!   transcript of interleaved threads.
//! - **A job holds no lock while it touches the filesystem.** The shared state
//!   is locked to read a control request or publish progress and released
//!   immediately, so pausing a job never waits for its current write to
//!   finish, only for the current chunk.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::conflict::{Conflict, ConflictDecision, ConflictPolicy, Resolution};
use crate::error::OperationError;
use crate::exec::{self, ItemOutcome, JobControl};
use crate::log::{LogEvent, OperationLog};
use crate::plan::PlanItem;
use crate::policy::FailurePolicy;
use crate::progress::{ItemProgress, Progress, RateEstimator, RemainingTime, Throughput};
use crate::spec::{JobSpec, OperationKind};
use crate::state::JobState;
use crate::store::{ItemRecord, ItemStatus, JobRecord, JobStore, RECORD_SCHEMA_VERSION};

/// A job's identity, unique within one engine.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JobId(u64);

impl JobId {
    pub fn value(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "job-{}", self.0)
    }
}

/// Something a job did, pushed to whoever is listening.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobEvent {
    Queued(JobId),
    Started(JobId),
    /// Aggregate progress moved. Sent on every item and every chunk of a large
    /// file; a consumer that redraws at 60 Hz coalesces them itself.
    Progress(JobId, Progress),
    ItemFinished {
        id: JobId,
        path: PathBuf,
        outcome: ItemOutcome,
    },
    /// The job needs a decision and has stopped until it gets one.
    ConflictRaised(JobId, Conflict),
    StateChanged(JobId, JobState),
    Finished(JobId, JobState),
}

impl JobEvent {
    pub fn job(&self) -> JobId {
        match self {
            JobEvent::Queued(id)
            | JobEvent::Started(id)
            | JobEvent::Progress(id, _)
            | JobEvent::ItemFinished { id, .. }
            | JobEvent::ConflictRaised(id, _)
            | JobEvent::StateChanged(id, _)
            | JobEvent::Finished(id, _) => *id,
        }
    }
}

/// What the outside world has asked a running job to do.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ControlRequest {
    #[default]
    None,
    Pause,
    Cancel,
    CancelWithRollback,
}

/// Everything a consumer can see about a job, taken at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: OperationKind,
    pub state: JobState,
    pub progress: Progress,
    pub item: ItemProgress,
    pub current: Option<PathBuf>,
    pub throughput: Throughput,
    pub remaining: RemainingTime,
    /// The conflict the job is parked on, if it is parked on one.
    pub conflict: Option<Conflict>,
    pub failures: Vec<(PathBuf, OperationError)>,
    pub checksums: Vec<(PathBuf, String)>,
    pub log: OperationLog,
}

struct JobShared {
    state: JobState,
    control: ControlRequest,
    progress: Progress,
    item: ItemProgress,
    current: Option<PathBuf>,
    conflicts: ConflictPolicy,
    pending_conflict: Option<Conflict>,
    decision: Option<ConflictDecision>,
    log: OperationLog,
    items: Vec<ItemRecord>,
    estimator: RateEstimator,
    started: Instant,
    bytes_before_item: u64,
    last_persisted: Option<Instant>,
    checksums: Vec<(PathBuf, String)>,
    failures: Vec<(PathBuf, OperationError)>,
}

struct Job {
    id: JobId,
    kind: OperationKind,
    spec: JobSpec,
    shared: Mutex<JobShared>,
    signal: Condvar,
    listeners: Mutex<Vec<Sender<JobEvent>>>,
}

impl Job {
    fn publish(&self, event: JobEvent) {
        let mut listeners = self.listeners.lock().expect("job listeners");
        listeners.retain(|listener| listener.send(event.clone()).is_ok());
    }

    fn set_state(&self, shared: &mut JobShared, next: JobState) {
        if shared.state == next {
            return;
        }
        debug_assert!(
            shared.state.can_transition_to(next),
            "illegal transition {:?} -> {next:?}",
            shared.state
        );
        let from = shared.state;
        shared.state = next;
        let at = shared.started.elapsed().as_millis() as u64;
        shared
            .log
            .push(at, None, LogEvent::StateChanged { from, to: next });
        self.publish(JobEvent::StateChanged(self.id, next));
    }

    fn snapshot(&self, shared: &JobShared) -> JobSnapshot {
        JobSnapshot {
            id: self.id,
            kind: self.kind,
            state: shared.state,
            progress: shared.progress,
            item: shared.item.clone(),
            current: shared.current.clone(),
            throughput: shared.estimator.throughput(),
            remaining: shared.estimator.remaining(&shared.progress),
            conflict: shared.pending_conflict.clone(),
            failures: shared.failures.clone(),
            checksums: shared.checksums.clone(),
            log: shared.log.clone(),
        }
    }

    fn record(&self, shared: &JobShared) -> JobRecord {
        JobRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            id: self.id.0,
            kind: self.kind,
            state: shared.state,
            progress: shared.progress,
            items: shared.items.clone(),
            log: shared.log.clone(),
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0),
            checksums: shared
                .checksums
                .iter()
                .map(|(path, digest)| (path.to_string_lossy().into_owned(), digest.clone()))
                .collect(),
        }
    }
}

/// How the engine is configured.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// How many jobs may run at once.
    pub workers: usize,
    /// Where records are written. `None` runs the engine in memory, which is
    /// what a test that does not care about recovery wants.
    pub store: Option<JobStore>,
    /// Standing conflict answers every job starts with.
    pub conflicts: ConflictPolicy,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            workers: 2,
            store: None,
            conflicts: ConflictPolicy::new(),
        }
    }
}

struct Inner {
    jobs: Mutex<HashMap<JobId, Arc<Job>>>,
    queue: Mutex<VecDeque<JobId>>,
    ready: Condvar,
    stopping: AtomicBool,
    next_id: AtomicU64,
    store: Option<JobStore>,
    defaults: ConflictPolicy,
}

/// The engine.
///
/// Dropping it waits for running jobs to finish, cancelling any that are parked
/// on a pause or a conflict, because a parked job has nobody left to answer it.
/// It never cancels a job that is making progress.
pub struct JobEngine {
    inner: Arc<Inner>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

/// A receipt for a submitted job.
///
/// It implements no `Drop`. Dropping every handle to a job does not pause it,
/// cancel it, or make the engine forget it; the job runs to completion and its
/// record stays readable. That is the whole point of the type, and
/// `tests/job_outlives_handle.rs` proves it.
pub struct JobHandle {
    id: JobId,
    events: Receiver<JobEvent>,
}

impl std::fmt::Debug for JobHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JobHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl JobHandle {
    pub fn id(&self) -> JobId {
        self.id
    }

    /// The event stream. Dropping the handle drops the receiver, and the
    /// engine simply stops sending — it does not stop working.
    pub fn events(&self) -> &Receiver<JobEvent> {
        &self.events
    }

    /// Blocks for the next event, or `None` once the job can send no more.
    pub fn next_event(&self) -> Option<JobEvent> {
        self.events.recv().ok()
    }
}

impl JobEngine {
    pub fn new(config: EngineConfig) -> Self {
        let inner = Arc::new(Inner {
            jobs: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            stopping: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            store: config.store,
            defaults: config.conflicts,
        });
        let mut workers = Vec::new();
        for index in 0..config.workers.max(1) {
            let inner = Arc::clone(&inner);
            workers.push(
                std::thread::Builder::new()
                    .name(format!("files-operations-{index}"))
                    .spawn(move || worker_loop(inner))
                    .expect("a worker thread"),
            );
        }
        Self {
            inner,
            workers: Mutex::new(workers),
        }
    }

    /// Accepts a job and queues it.
    ///
    /// The spec is validated first, so something that could never work — a
    /// destination inside its own source, a name with a separator in it — is
    /// refused here rather than becoming a job that fails a moment later.
    pub fn submit(&self, spec: JobSpec) -> Result<JobHandle, OperationError> {
        spec.validate()?;
        if let crate::spec::Operation::PermanentDelete { .. } = spec.operation {
            // The confirmation is a type that cannot be forged or
            // deserialized, so its presence in the spec is the check. Stating
            // it here keeps the requirement visible at the entry point.
            debug_assert!(matches!(
                spec.operation,
                crate::spec::Operation::PermanentDelete { .. }
            ));
        }
        let id = JobId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let mut conflicts = self.inner.defaults.clone();
        for (kind, resolution) in standing_answers(&spec.conflicts) {
            conflicts.remember(kind, resolution);
        }
        let job = Arc::new(Job {
            id,
            kind: spec.kind(),
            spec,
            shared: Mutex::new(JobShared {
                state: JobState::Queued,
                control: ControlRequest::None,
                progress: Progress::default(),
                item: ItemProgress::default(),
                current: None,
                conflicts,
                pending_conflict: None,
                decision: None,
                log: OperationLog::default(),
                items: Vec::new(),
                estimator: RateEstimator::default(),
                started: Instant::now(),
                bytes_before_item: 0,
                last_persisted: None,
                checksums: Vec::new(),
                failures: Vec::new(),
            }),
            signal: Condvar::new(),
            listeners: Mutex::new(Vec::new()),
        });
        let (sender, receiver) = channel();
        job.listeners.lock().expect("job listeners").push(sender);
        self.inner
            .jobs
            .lock()
            .expect("engine jobs")
            .insert(id, Arc::clone(&job));
        self.persist(&job);
        job.publish(JobEvent::Queued(id));
        self.inner.queue.lock().expect("engine queue").push_back(id);
        self.inner.ready.notify_one();
        Ok(JobHandle {
            id,
            events: receiver,
        })
    }

    /// A second event stream for a job somebody else submitted.
    pub fn subscribe(&self, id: JobId) -> Option<Receiver<JobEvent>> {
        let job = self.job(id)?;
        let (sender, receiver) = channel();
        job.listeners.lock().expect("job listeners").push(sender);
        Some(receiver)
    }

    pub fn snapshot(&self, id: JobId) -> Option<JobSnapshot> {
        let job = self.job(id)?;
        let shared = job.shared.lock().expect("job state");
        Some(job.snapshot(&shared))
    }

    pub fn state(&self, id: JobId) -> Option<JobState> {
        Some(self.snapshot(id)?.state)
    }

    pub fn jobs(&self) -> Vec<JobSnapshot> {
        let jobs = self.inner.jobs.lock().expect("engine jobs");
        let mut snapshots: Vec<JobSnapshot> = jobs
            .values()
            .map(|job| {
                let shared = job.shared.lock().expect("job state");
                job.snapshot(&shared)
            })
            .collect();
        snapshots.sort_by_key(|snapshot| snapshot.id);
        snapshots
    }

    /// Asks a job to stop at its next item or chunk boundary.
    ///
    /// Refused for an operation that cannot pause usefully; see
    /// [`OperationKind::supports_pause`].
    pub fn pause(&self, id: JobId) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        if !job.kind.supports_pause() {
            return false;
        }
        let mut shared = job.shared.lock().expect("job state");
        if shared.state.is_terminal() {
            return false;
        }
        shared.control = ControlRequest::Pause;
        job.signal.notify_all();
        true
    }

    pub fn resume(&self, id: JobId) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        let mut shared = job.shared.lock().expect("job state");
        if shared.state != JobState::Paused {
            return false;
        }
        shared.control = ControlRequest::None;
        job.signal.notify_all();
        true
    }

    pub fn cancel(&self, id: JobId) -> bool {
        self.request_cancel(id, ControlRequest::Cancel)
    }

    /// Cancels and undoes what the job created, for the operations that can.
    pub fn cancel_with_rollback(&self, id: JobId) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        if !job.kind.supports_rollback() {
            return false;
        }
        self.request_cancel(id, ControlRequest::CancelWithRollback)
    }

    fn request_cancel(&self, id: JobId, request: ControlRequest) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        let mut shared = job.shared.lock().expect("job state");
        if shared.state.is_terminal() {
            return false;
        }
        shared.control = request;
        if shared.state == JobState::Queued {
            // Nothing has picked it up, so it settles here.
            job.set_state(&mut shared, JobState::Cancelled);
            let record = job.record(&shared);
            drop(shared);
            self.write_record(&record);
            job.publish(JobEvent::Finished(id, JobState::Cancelled));
            return true;
        }
        job.signal.notify_all();
        true
    }

    /// Answers the conflict a job is parked on.
    pub fn resolve(&self, id: JobId, decision: ConflictDecision) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        let mut shared = job.shared.lock().expect("job state");
        if shared.pending_conflict.is_none() {
            return false;
        }
        shared.decision = Some(decision);
        job.signal.notify_all();
        true
    }

    /// Re-queues the items that failed.
    ///
    /// The failed items are re-planned rather than resumed, which is why the
    /// state goes back to `Queued`: a retry after the user freed disk space or
    /// fixed a mode has to look at the filesystem again, not replay a decision
    /// it made before the fix.
    pub fn retry_failed(&self, id: JobId) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        let mut shared = job.shared.lock().expect("job state");
        if !shared.state.is_terminal() {
            return false;
        }
        let retryable: Vec<usize> = shared
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.status == ItemStatus::Failed)
            .map(|(index, _)| index)
            .collect();
        if retryable.is_empty() {
            return false;
        }
        for index in retryable {
            shared.items[index].status = ItemStatus::Pending;
            shared.items[index].error = None;
        }
        shared.progress.items_failed = 0;
        shared.failures.clear();
        shared.pending_conflict = None;
        shared.control = ControlRequest::None;
        job.set_state(&mut shared, JobState::Queued);
        drop(shared);
        self.inner.queue.lock().expect("engine queue").push_back(id);
        self.inner.ready.notify_one();
        true
    }

    /// Waits for a job to reach a terminal state.
    ///
    /// Returns `None` on timeout, which a test must treat as a failure rather
    /// than as "probably fine".
    pub fn wait(&self, id: JobId, timeout: Duration) -> Option<JobSnapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            let snapshot = self.snapshot(id)?;
            if snapshot.state.is_terminal() {
                return Some(snapshot);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Waits until a job reaches a state the predicate accepts.
    pub fn wait_for(
        &self,
        id: JobId,
        timeout: Duration,
        accept: impl Fn(&JobSnapshot) -> bool,
    ) -> Option<JobSnapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            let snapshot = self.snapshot(id)?;
            if accept(&snapshot) {
                return Some(snapshot);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn job(&self, id: JobId) -> Option<Arc<Job>> {
        self.inner
            .jobs
            .lock()
            .expect("engine jobs")
            .get(&id)
            .cloned()
    }

    fn persist(&self, job: &Arc<Job>) {
        let shared = job.shared.lock().expect("job state");
        let record = job.record(&shared);
        drop(shared);
        self.write_record(&record);
    }

    fn write_record(&self, record: &JobRecord) {
        if let Some(store) = &self.inner.store {
            let _ = store.write(record);
        }
    }
}

impl Drop for JobEngine {
    fn drop(&mut self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        // A job parked on a pause or a conflict has nobody left to answer it,
        // so it is cancelled rather than left to hold a worker forever. A job
        // that is running is left alone and finishes.
        let jobs: Vec<Arc<Job>> = self
            .inner
            .jobs
            .lock()
            .expect("engine jobs")
            .values()
            .cloned()
            .collect();
        for job in jobs {
            let mut shared = job.shared.lock().expect("job state");
            if matches!(
                shared.state,
                JobState::Paused | JobState::WaitingOnConflict | JobState::Queued
            ) {
                shared.control = ControlRequest::Cancel;
            }
            job.signal.notify_all();
        }
        self.inner.ready.notify_all();
        let workers = std::mem::take(&mut *self.workers.lock().expect("engine workers"));
        for worker in workers {
            let _ = worker.join();
        }
    }
}

fn standing_answers(policy: &ConflictPolicy) -> Vec<(crate::conflict::ConflictKind, Resolution)> {
    use crate::conflict::ConflictKind;
    let mut answers = Vec::new();
    for kind in [
        ConflictKind::Exists,
        ConflictKind::CaseConflict,
        ConflictKind::Permission,
        ConflictKind::NoSpace,
    ] {
        let probe = Conflict {
            kind,
            source: None,
            destination: PathBuf::new(),
            existing: None,
        };
        if let Some(resolution) = policy.answer(&probe) {
            answers.push((kind, resolution));
        }
    }
    answers
}

// --- The worker ----------------------------------------------------------

fn worker_loop(inner: Arc<Inner>) {
    loop {
        let next = {
            let mut queue = inner.queue.lock().expect("engine queue");
            loop {
                if let Some(id) = queue.pop_front() {
                    break Some(id);
                }
                if inner.stopping.load(Ordering::SeqCst) {
                    break None;
                }
                let (guard, _) = inner
                    .ready
                    .wait_timeout(queue, Duration::from_millis(50))
                    .expect("engine queue");
                queue = guard;
            }
        };
        let Some(id) = next else { return };
        let job = inner.jobs.lock().expect("engine jobs").get(&id).cloned();
        let Some(job) = job else { continue };
        run_job(&inner, job);
    }
}

fn run_job(inner: &Arc<Inner>, job: Arc<Job>) {
    {
        let mut shared = job.shared.lock().expect("job state");
        if shared.state.is_terminal() {
            return;
        }
        if shared.control == ControlRequest::Cancel
            || shared.control == ControlRequest::CancelWithRollback
        {
            job.set_state(&mut shared, JobState::Cancelled);
            finish(inner, &job, &mut shared, JobState::Cancelled);
            return;
        }
        shared.started = Instant::now();
        shared.estimator = RateEstimator::default();
        job.set_state(&mut shared, JobState::Running);
    }
    job.publish(JobEvent::Started(job.id));

    // A retry run reuses the item list it already has; a first run plans.
    let is_retry = {
        let shared = job.shared.lock().expect("job state");
        !shared.items.is_empty()
    };
    let plan = exec::build_plan(&job.spec.operation, &job.spec.policy);
    if !is_retry {
        let mut shared = job.shared.lock().expect("job state");
        shared.items = plan
            .items
            .iter()
            .filter(|item| item.kind.counts_as_item())
            .map(|item| ItemRecord {
                source: item.source.clone(),
                destination: item.destination.clone(),
                status: ItemStatus::Pending,
                bytes: item.bytes,
                error: None,
            })
            .collect();
        shared.progress = Progress {
            items_total: plan.total_items(),
            bytes_total: plan.total_bytes(),
            ..Progress::default()
        };
        let at = shared.started.elapsed().as_millis() as u64;
        let planned = LogEvent::Planned {
            items: shared.progress.items_total,
            bytes: shared.progress.bytes_total,
        };
        shared.log.push(at, None, planned);
        // Sources the walk could not read are failures, recorded before any
        // work starts so the totals never shrink silently.
        for (path, error) in &plan.unreadable {
            shared.failures.push((path.clone(), error.clone()));
            shared.progress.items_failed += 1;
            let at = shared.started.elapsed().as_millis() as u64;
            shared.log.push(
                at,
                Some(path.clone()),
                LogEvent::ItemFailed {
                    error: error.clone(),
                },
            );
        }
    } else {
        let mut shared = job.shared.lock().expect("job state");
        shared.progress.items_done = shared
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Done)
            .count() as u64;
        shared.progress.items_skipped = shared
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Skipped)
            .count() as u64;
    }
    persist(inner, &job);

    let mut cancelled = false;
    let mut rollback_requested = false;
    let mut item_index = 0usize;

    for item in &plan.items {
        if item.kind.counts_as_item() {
            let should_run = {
                let shared = job.shared.lock().expect("job state");
                shared
                    .items
                    .get(item_index)
                    .map(|record| record.status == ItemStatus::Pending)
                    .unwrap_or(true)
            };
            if !should_run {
                item_index += 1;
                continue;
            }
        }
        let outcome = run_item(inner, &job, item);
        match outcome {
            Err(error) => {
                cancelled = true;
                let mut shared = job.shared.lock().expect("job state");
                rollback_requested = shared.control == ControlRequest::CancelWithRollback;
                let at = shared.started.elapsed().as_millis() as u64;
                let path = error.path().map(std::path::Path::to_path_buf);
                shared.log.push(at, path, LogEvent::ItemFailed { error });
                break;
            }
            Ok(outcome) => {
                let stop = record_outcome(&job, item, item_index, outcome);
                if item.kind.counts_as_item() {
                    item_index += 1;
                }
                if stop {
                    let shared = job.shared.lock().expect("job state");
                    rollback_requested =
                        job.spec.policy.on_failure == FailurePolicy::StopAndRollback;
                    drop(shared);
                    break;
                }
            }
        }
        persist(inner, &job);
    }

    let mut shared = job.shared.lock().expect("job state");
    shared.current = None;
    shared.item = ItemProgress::default();
    let final_state = if cancelled {
        if rollback_requested && job.kind.supports_rollback() {
            JobState::RolledBack
        } else {
            JobState::Cancelled
        }
    } else if rollback_requested && job.kind.supports_rollback() {
        JobState::RolledBack
    } else if shared.progress.items_failed > 0 {
        JobState::Failed
    } else {
        JobState::Completed
    };
    if final_state == JobState::RolledBack {
        let created = shared.log.created_paths();
        drop(shared);
        let mut control = WorkerControl {
            job: Arc::clone(&job),
            item_bytes_base: 0,
        };
        exec::rollback(&created, &mut control);
        shared = job.shared.lock().expect("job state");
    }
    job.set_state(&mut shared, final_state);
    finish(inner, &job, &mut shared, final_state);
}

fn finish(
    inner: &Arc<Inner>,
    job: &Arc<Job>,
    shared: &mut MutexGuard<'_, JobShared>,
    state: JobState,
) {
    shared.last_persisted = Some(Instant::now());
    let record = job.record(shared);
    if let Some(store) = &inner.store {
        let _ = store.write(&record);
    }
    job.publish(JobEvent::Finished(job.id, state));
}

/// How often a running job's record is rewritten.
///
/// The record holds every item, so writing it after every item is quadratic:
/// a 100,000-file copy would write 100,000 records averaging tens of megabytes
/// each, and the benchmark measured that cost at ten times the copy itself.
///
/// Throttling trades a little recovery precision for a job that finishes. What
/// is preserved is the property that matters: a record on disk is always a
/// complete document that says the job was running and which items it had. An
/// item that finished in the last quarter second comes back marked pending, so
/// a resubmitted job re-copies it — conservative in the safe direction, and the
/// conflict model already covers a destination that is unexpectedly there.
///
/// State changes and the final state are written immediately regardless.
const PERSIST_INTERVAL: Duration = Duration::from_millis(250);

fn persist(inner: &Arc<Inner>, job: &Arc<Job>) {
    let Some(store) = &inner.store else {
        return;
    };
    let mut shared = job.shared.lock().expect("job state");
    let now = Instant::now();
    if shared
        .last_persisted
        .is_some_and(|last| now.duration_since(last) < PERSIST_INTERVAL)
    {
        return;
    }
    shared.last_persisted = Some(now);
    let record = job.record(&shared);
    drop(shared);
    let _ = store.write(&record);
}

fn run_item(
    _inner: &Arc<Inner>,
    job: &Arc<Job>,
    item: &PlanItem,
) -> Result<ItemOutcome, OperationError> {
    let base = {
        let mut shared = job.shared.lock().expect("job state");
        shared.current = Some(item.source.clone());
        shared.item = ItemProgress {
            bytes_total: item.bytes,
            bytes_done: 0,
        };
        shared.bytes_before_item = shared.progress.bytes_done;
        // A directory epilogue is bookkeeping rather than work the user asked
        // for, so it does not get a start and a completion line. What it
        // actually does — restoring a mode, removing a moved-from directory —
        // still logs through the executor.
        if item.kind.counts_as_item() {
            let at = shared.started.elapsed().as_millis() as u64;
            shared.log.push(
                at,
                Some(item.source.clone()),
                LogEvent::ItemStarted { bytes: item.bytes },
            );
        }
        shared.progress.bytes_done
    };
    let mut control = WorkerControl {
        job: Arc::clone(job),
        item_bytes_base: base,
    };
    exec::execute_item(&job.spec.operation, item, &job.spec.policy, &mut control)
}

/// Records an item's outcome. Returns whether the job should stop.
fn record_outcome(job: &Arc<Job>, item: &PlanItem, index: usize, outcome: ItemOutcome) -> bool {
    let mut shared = job.shared.lock().expect("job state");
    let at = shared.started.elapsed().as_millis() as u64;
    let counts = item.kind.counts_as_item();
    let mut stop = false;
    match &outcome {
        ItemOutcome::Done { bytes, verified } => {
            if counts {
                shared.progress.items_done += 1;
                if let Some(record) = shared.items.get_mut(index) {
                    record.status = ItemStatus::Done;
                }
            }
            let base = shared.bytes_before_item;
            shared.progress.bytes_done = base + bytes;
            if counts {
                shared.log.push(
                    at,
                    Some(item.source.clone()),
                    LogEvent::ItemCompleted {
                        bytes: *bytes,
                        verified: *verified,
                    },
                );
            }
        }
        ItemOutcome::Skipped(reason) => {
            if counts {
                shared.progress.items_skipped += 1;
                if let Some(record) = shared.items.get_mut(index) {
                    record.status = ItemStatus::Skipped;
                }
            }
            shared.log.push(
                at,
                Some(item.source.clone()),
                LogEvent::ItemSkipped { reason: *reason },
            );
        }
        ItemOutcome::Failed(error) => {
            if counts {
                shared.progress.items_failed += 1;
                if let Some(record) = shared.items.get_mut(index) {
                    record.status = ItemStatus::Failed;
                    record.error = Some(error.clone());
                }
            }
            shared.failures.push((item.source.clone(), error.clone()));
            shared.log.push(
                at,
                Some(item.source.clone()),
                LogEvent::ItemFailed {
                    error: error.clone(),
                },
            );
            stop = job.spec.policy.on_failure != FailurePolicy::Continue;
        }
    }
    let elapsed = shared.started.elapsed();
    let bytes = shared.progress.bytes_done;
    let items = shared.progress.settled_items();
    shared.estimator.observe(elapsed, bytes, items);
    let progress = shared.progress;
    drop(shared);
    job.publish(JobEvent::ItemFinished {
        id: job.id,
        path: item.source.clone(),
        outcome,
    });
    job.publish(JobEvent::Progress(job.id, progress));
    stop
}

/// The executor's view of a live job.
struct WorkerControl {
    job: Arc<Job>,
    item_bytes_base: u64,
}

impl JobControl for WorkerControl {
    fn checkpoint(&mut self) -> Result<(), OperationError> {
        let mut shared = self.job.shared.lock().expect("job state");
        loop {
            match shared.control {
                ControlRequest::Cancel | ControlRequest::CancelWithRollback => {
                    return Err(OperationError::Cancelled {
                        path: shared.current.clone().unwrap_or_default(),
                    });
                }
                ControlRequest::Pause => {
                    if shared.state != JobState::Paused {
                        self.job.set_state(&mut shared, JobState::Paused);
                    }
                    shared = self.job.signal.wait(shared).expect("job signal");
                }
                ControlRequest::None => {
                    if shared.state == JobState::Paused {
                        self.job.set_state(&mut shared, JobState::Running);
                    }
                    return Ok(());
                }
            }
        }
    }

    fn item_bytes(&mut self, done: u64) {
        let mut shared = self.job.shared.lock().expect("job state");
        shared.item.bytes_done = done;
        shared.progress.bytes_done = self.item_bytes_base + done;
        let elapsed = shared.started.elapsed();
        let bytes = shared.progress.bytes_done;
        let items = shared.progress.settled_items();
        shared.estimator.observe(elapsed, bytes, items);
        let progress = shared.progress;
        drop(shared);
        self.job.publish(JobEvent::Progress(self.job.id, progress));
    }

    fn resolve(&mut self, conflict: Conflict) -> Result<Resolution, OperationError> {
        let mut shared = self.job.shared.lock().expect("job state");
        if let Some(standing) = shared.conflicts.answer(&conflict) {
            let at = shared.started.elapsed().as_millis() as u64;
            shared.log.push(
                at,
                Some(conflict.destination.clone()),
                LogEvent::ConflictResolved {
                    resolution: standing,
                    standing: true,
                },
            );
            return Ok(standing);
        }
        let at = shared.started.elapsed().as_millis() as u64;
        shared.log.push(
            at,
            Some(conflict.destination.clone()),
            LogEvent::ConflictRaised {
                conflict: conflict.clone(),
            },
        );
        shared.pending_conflict = Some(conflict.clone());
        shared.decision = None;
        self.job.set_state(&mut shared, JobState::WaitingOnConflict);
        drop(shared);
        self.job
            .publish(JobEvent::ConflictRaised(self.job.id, conflict.clone()));

        let mut shared = self.job.shared.lock().expect("job state");
        let decision = loop {
            if matches!(
                shared.control,
                ControlRequest::Cancel | ControlRequest::CancelWithRollback
            ) {
                shared.pending_conflict = None;
                return Err(OperationError::Cancelled {
                    path: conflict.destination.clone(),
                });
            }
            if let Some(decision) = shared.decision.take() {
                break decision;
            }
            shared = self.job.signal.wait(shared).expect("job signal");
        };
        shared.conflicts.apply(&conflict, decision);
        shared.pending_conflict = None;
        let at = shared.started.elapsed().as_millis() as u64;
        shared.log.push(
            at,
            Some(conflict.destination.clone()),
            LogEvent::ConflictResolved {
                resolution: decision.resolution,
                standing: decision.scope == crate::conflict::ResolutionScope::ApplyToRemaining,
            },
        );
        self.job.set_state(&mut shared, JobState::Running);
        Ok(decision.resolution)
    }

    fn log(&mut self, path: Option<PathBuf>, event: LogEvent) {
        let mut shared = self.job.shared.lock().expect("job state");
        let at = shared.started.elapsed().as_millis() as u64;
        shared.log.push(at, path, event);
    }

    fn checksum(&mut self, path: PathBuf, digest: String) {
        let mut shared = self.job.shared.lock().expect("job state");
        shared.checksums.push((path, digest));
    }
}

//! Running previews somewhere other than the render thread.
//!
//! One worker thread, one request at a time, newest wins. That is the whole
//! policy, and it is the right one for a preview pane: holding arrow-down for a
//! second produces sixty requests and the user wants the sixtieth. A queue
//! would spend the thread on fifty-nine previews nobody will see, and a thread
//! pool would spend fifty-nine threads on them.
//!
//! The window never blocks. [`PreviewService::request`] returns immediately
//! after cancelling whatever was in flight, and [`PreviewService::poll`] takes
//! whatever has arrived. A result whose id is not the current one is dropped by
//! the caller, because a preview of the previous selection is not a preview.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{CancelToken, Preview, PreviewEngine, PreviewRequest};

/// Identifies one request, so a late answer to an abandoned question is
/// recognizable rather than merely stale.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    pub fn value(self) -> u64 {
        self.0
    }
}

/// One finished generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewOutcome {
    pub id: RequestId,
    pub request: PreviewRequest,
    /// `None` when the generation was cancelled. A cancelled preview has no
    /// content and no reason: the question was withdrawn, not answered.
    pub preview: Option<Preview>,
}

struct Job {
    id: RequestId,
    request: PreviewRequest,
    cancel: CancelToken,
}

/// The worker, its channels, and the token for whatever is in flight.
pub struct PreviewService {
    next_id: AtomicU64,
    jobs: Sender<Job>,
    results: Mutex<Receiver<PreviewOutcome>>,
    /// The token of the newest request. Held so a new request can cancel it.
    in_flight: Mutex<Option<CancelToken>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for PreviewService {
    fn default() -> Self {
        Self::new(Arc::new(PreviewEngine::default()))
    }
}

impl PreviewService {
    pub fn new(engine: Arc<PreviewEngine>) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (result_tx, results) = channel::<PreviewOutcome>();
        let worker = std::thread::Builder::new()
            .name("files-preview".to_string())
            .spawn(move || {
                // Ends when the sender drops, which is when the service does.
                while let Ok(job) = job_rx.recv() {
                    if job.cancel.is_cancelled() {
                        // Superseded while it was queued. Nothing is read.
                        let _ = result_tx.send(PreviewOutcome {
                            id: job.id,
                            request: job.request,
                            preview: None,
                        });
                        continue;
                    }
                    let preview = engine.preview(&job.request, &job.cancel).ok();
                    if result_tx
                        .send(PreviewOutcome {
                            id: job.id,
                            request: job.request,
                            preview,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("preview worker thread");
        Self {
            next_id: AtomicU64::new(1),
            jobs,
            results: Mutex::new(results),
            in_flight: Mutex::new(None),
            worker: Some(worker),
        }
    }

    /// Asks for a preview, cancelling whatever was in flight.
    ///
    /// Returns the id the answer will carry. This does no I/O and takes no
    /// lock the worker holds, so calling it from a frame is free.
    pub fn request(&self, request: PreviewRequest) -> RequestId {
        let id = RequestId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        {
            let mut in_flight = self.in_flight.lock().expect("preview token lock");
            if let Some(previous) = in_flight.replace(cancel.clone()) {
                previous.cancel();
            }
        }
        // A closed channel means the worker died, which the next poll reports
        // as no results rather than as a panic in a frame.
        let _ = self.jobs.send(Job {
            id,
            request,
            cancel,
        });
        id
    }

    /// Cancels whatever is in flight without asking for anything new. This is
    /// what closing the preview pane does.
    pub fn cancel_in_flight(&self) {
        if let Some(token) = self.in_flight.lock().expect("preview token lock").take() {
            token.cancel();
        }
    }

    /// Takes every finished outcome. Never blocks.
    pub fn poll(&self) -> Vec<PreviewOutcome> {
        let results = self.results.lock().expect("preview result lock");
        let mut outcomes = Vec::new();
        loop {
            match results.try_recv() {
                Ok(outcome) => outcomes.push(outcome),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        outcomes
    }

    /// Blocks until one outcome arrives. Tests only: no frame calls this.
    pub fn wait(&self) -> Option<PreviewOutcome> {
        let results = self.results.lock().expect("preview result lock");
        results.recv().ok()
    }
}

impl Drop for PreviewService {
    fn drop(&mut self) {
        self.cancel_in_flight();
        // Dropping the job sender ends the worker's loop. Joining keeps a
        // half-finished decode from outliving the engine it borrows.
        let (dead, _) = channel();
        let jobs = std::mem::replace(&mut self.jobs, dead);
        drop(jobs);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

//! Cancellation, shared with whoever asked for the preview.
//!
//! The same shape as `files_core::listing::CancellationToken`, kept here rather
//! than imported so `files-preview` does not depend on the listing crate for
//! one atomic. Both are a flag a producer checks between units of work; neither
//! can interrupt a decoder mid-call, which is why the units have to be small
//! and why the size limits matter.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A generation was abandoned. Nothing partial comes back with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cancelled;

/// A flag both the requester and the worker hold.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// A token that is already set, for testing the cancelled path.
    pub fn cancelled() -> Self {
        let token = Self::new();
        token.cancel();
        token
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// `Err(Cancelled)` when the flag is set, so a provider's inner loop is one
    /// `?` rather than an `if` that is easy to forget.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

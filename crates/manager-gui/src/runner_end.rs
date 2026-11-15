//! Making sure the window always learns that a transaction's background work
//! has ended, however it ended (ticket 105).
//!
//! The window keeps the work's task while it runs, and `transaction_running()`
//! reads that. The runner can end four ways: the privileged service cannot be
//! reached, `TransactionRunner::run` returns its result, it returns an error
//! part way, or the work panics. Only the second reported anything, so the
//! window used to think a transaction was still running after the other
//! three — and after a successful one too, because nothing cleared the task.
//! [`EndsOnce`] sends one "ended" message on every path, panics included.

/// What the background work tells the window.
pub(crate) enum WorkerEvent<E> {
    /// Something the runner reported.
    Runner(E),
    /// The work is over, whatever happened.
    Ended,
}

/// Calls its function once when dropped: at the end of the work, on an
/// early return, and while unwinding from a panic.
pub(crate) struct EndsOnce<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> EndsOnce<F> {
    pub(crate) fn new(ended: F) -> Self {
        Self(Some(ended))
    }
}

impl<F: FnOnce()> Drop for EndsOnce<F> {
    fn drop(&mut self) {
        if let Some(ended) = self.0.take() {
            ended();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EndsOnce;
    use std::cell::Cell;

    /// The work as the window runs it: connect, run, report — with the guard
    /// taken first, as `run_real_transaction` does.
    fn work(connects: bool, run: Result<(), ()>, ended: &Cell<u32>) -> Result<(), ()> {
        let _ends = EndsOnce::new(|| ended.set(ended.get() + 1));
        if !connects {
            return Ok(());
        }
        run
    }

    #[test]
    fn every_way_the_work_ends_is_reported_exactly_once() {
        for (connects, run) in [(false, Ok(())), (true, Ok(())), (true, Err(()))] {
            let ended = Cell::new(0);
            let _ = work(connects, run, &ended);
            assert_eq!(ended.get(), 1, "connects {connects}, run {run:?}");
        }
    }

    #[test]
    fn a_panic_in_the_work_is_still_reported() {
        let ended = std::sync::atomic::AtomicU32::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ends = EndsOnce::new(|| {
                ended.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
            panic!("the runner panicked");
        }));
        assert!(result.is_err());
        assert_eq!(ended.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

//! Startup tracing, so the overlay's open time is measurable from the shipped
//! binary rather than only from a test that reimplements it.
//!
//! Two stages are marked, and they are different claims:
//!
//! - `shell-ready` — the window callback has returned. The overlay entity
//!   exists, the search row holds focus, and the model a first frame would draw
//!   is complete. The application library is *not* in it yet; it is still being
//!   read.
//! - `library-ready` — the background read finished and the applications are in
//!   the model. This is the first frame that could show the library.
//!
//! Nothing is printed unless [`TRACE_VARIABLE`] is `1`, so the headless launch
//! smoke still expects silence. The line shape is fixed because a benchmark
//! parses it; see [`trace_line`].

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Set this to `1` to make the binary report its own startup timing on stderr.
pub const TRACE_VARIABLE: &str = "BETTER_LAUNCHER_TRACE_STARTUP";

/// The prefix every traced line carries, so a parser can ignore anything else
/// the process writes.
pub const TRACE_PREFIX: &str = "better-launcher: trace";

/// The window is on screen with a focused search row and no library yet.
pub const STAGE_SHELL_READY: &str = "shell-ready";
/// The application library has been read and is in the model.
pub const STAGE_LIBRARY_READY: &str = "library-ready";

static STARTED: OnceLock<Instant> = OnceLock::new();
static ENABLED: OnceLock<bool> = OnceLock::new();

/// Records the moment the process reached `main`. Calling it twice keeps the
/// first moment, which is the one that means anything.
pub fn begin() {
    let _ = STARTED.set(Instant::now());
}

pub fn is_enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var(TRACE_VARIABLE).as_deref() == Ok("1"))
}

/// One traced line. Milliseconds with three decimals, and a free-form detail
/// that is never allowed to contain a newline.
pub fn trace_line(stage: &str, elapsed: Duration, detail: &str) -> String {
    format!(
        "{TRACE_PREFIX} stage={stage} ms={:.3} detail={}",
        elapsed.as_secs_f64() * 1000.0,
        detail.replace('\n', " ")
    )
}

/// Reports a stage, if tracing was asked for and [`begin`] ran.
pub fn mark(stage: &str, detail: &str) {
    if !is_enabled() {
        return;
    }
    let Some(started) = STARTED.get() else {
        return;
    };
    eprintln!("{}", trace_line(stage, started.elapsed(), detail));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traced_line_is_parseable_and_stays_on_one_line() {
        let line = trace_line(
            STAGE_LIBRARY_READY,
            Duration::from_micros(123_456),
            "applications=5000\nsecond line",
        );
        assert_eq!(
            line,
            "better-launcher: trace stage=library-ready ms=123.456 detail=applications=5000 second line"
        );
        assert!(line.starts_with(TRACE_PREFIX));
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn marking_before_begin_prints_nothing_rather_than_a_wrong_number() {
        // `is_enabled` is memoized per process and the trace variable is not
        // set under `cargo test`, so this asserts the quiet path the launch
        // smoke depends on.
        assert!(!is_enabled());
        mark(STAGE_SHELL_READY, "detail");
    }
}

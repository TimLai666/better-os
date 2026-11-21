//! A recording's life (spec R1–R8): its states, its time as the indicator
//! shows it, what ends it, and how often a frame is taken.

use std::time::Duration;

use crate::words::NoticeKind;

/// GNOME's frame rate (`DEFAULT_FRAMERATE`, screencastService.js 28).
pub const FRAMES_PER_SECOND: u32 = 30;

/// The time between frames.
pub fn frame_interval() -> Duration {
    Duration::from_nanos(1_000_000_000 / u64::from(FRAMES_PER_SECOND))
}

/// The indicator's time, `m:ss` (`remoteAccess.js` `_updateLabel`:
/// `'%d:%02d'.format(minutes, seconds)`), counting whole seconds.
pub fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Why a recording ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ending {
    /// The person stopped it.
    Stopped,
    /// The encoder could not be built or would not start.
    FailedToStart(String),
    /// Writing failed because the disk is full.
    OutOfSpace,
    /// Anything else after it started: the compositor stopped the capture,
    /// the output went away, the encoder failed.
    Failed(String),
}

impl Ending {
    /// What the notification says (R7, R8).
    pub fn notice(&self) -> NoticeKind {
        match self {
            Ending::Stopped => NoticeKind::RecordingSaved,
            Ending::FailedToStart(reason) => NoticeKind::RecordingFailedToStart(reason.clone()),
            Ending::OutOfSpace => NoticeKind::RecordingOutOfSpace,
            Ending::Failed(reason) => NoticeKind::RecordingEnded(reason.clone()),
        }
    }

    /// Whether a file worth keeping may exist: a recording that never
    /// started has none, and its empty file is removed.
    pub fn keeps_file(&self) -> bool {
        !matches!(self, Ending::FailedToStart(_))
    }
}

/// Whether an I/O error text from the encoder means the disk is full. GStreamer
/// reports `GST_RESOURCE_ERROR_NO_SPACE_LEFT` from `filesink` with the C
/// library's message for `ENOSPC`.
pub fn is_out_of_space(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("no space left") || message.contains("no_space_left")
}

#[cfg(test)]
mod tests {
    use super::{Ending, elapsed_label, frame_interval, is_out_of_space};
    use crate::words::NoticeKind;
    use std::time::Duration;

    #[test]
    fn the_time_reads_as_gnomes_indicator() {
        assert_eq!(elapsed_label(Duration::ZERO), "0:00");
        assert_eq!(elapsed_label(Duration::from_millis(9_999)), "0:09");
        assert_eq!(elapsed_label(Duration::from_secs(61)), "1:01");
        assert_eq!(elapsed_label(Duration::from_secs(3_600)), "60:00");
    }

    #[test]
    fn thirty_frames_a_second() {
        assert_eq!(frame_interval(), Duration::from_nanos(33_333_333));
    }

    #[test]
    fn each_ending_says_which() {
        assert_eq!(Ending::Stopped.notice(), NoticeKind::RecordingSaved);
        assert_eq!(Ending::OutOfSpace.notice(), NoticeKind::RecordingOutOfSpace);
        assert!(!Ending::FailedToStart("x".into()).keeps_file());
        assert!(Ending::Failed("x".into()).keeps_file());
        assert!(is_out_of_space("Error while writing to file: No space left on device"));
        assert!(!is_out_of_space("Internal data stream error"));
    }
}

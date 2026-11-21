//! Better Screenshot's pixels (engineering/specs/screenshot.md, "The
//! compositor"): the outputs and windows as they are when the interface
//! opens, each copied with and without the pointer; the clipboard; and the
//! recording. All of it goes through the [`Capture`] trait, which the window
//! crate uses and which [`fake::FakeCapture`] stands in for in tests.
//!
//! The real one, [`wayland::WaylandCapture`], is a Wayland client of its own:
//! GPUI binds none of the capture protocols and writes only text to the
//! clipboard (`gpui_linux` `wayland/client.rs` `write_to_clipboard`).

pub mod clipboard;
pub mod fake;
pub mod png;
pub mod protocols;
pub mod recorder;
pub mod wayland;

use screenshot_core::compose::Frame;
use screenshot_core::geometry::Rect;

/// One output.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputInfo {
    /// The compositor's name (`wl_output.name`, `WINIT-0`, `eDP-1`), which
    /// is also how GPUI names a display.
    pub name: String,
    /// Where it is, in logical pixels (`xdg_output`).
    pub logical: Rect,
}

/// One window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub title: String,
    pub app_id: String,
    /// The output it is on (its first, when it spans several), by name.
    pub output: Option<String>,
    /// It has the keyboard (`zcosmic_toplevel_handle_v1` state `activated`).
    pub activated: bool,
    /// Its width in logical pixels, when the compositor says
    /// (`zcosmic_toplevel_handle_v1.geometry`). Its picture is its buffer,
    /// which a client drawing at another scale makes larger or smaller.
    pub logical_width: Option<i32>,
}

/// A picture copied twice: as the screen shows it without the pointer, and
/// with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub plain: Frame,
    /// `None` when the compositor gave no second copy.
    pub with_pointer: Option<Frame>,
}

impl Shot {
    /// The copy to use for the pointer switch.
    pub fn frame(&self, pointer: bool) -> &Frame {
        if pointer {
            self.with_pointer.as_ref().unwrap_or(&self.plain)
        } else {
            &self.plain
        }
    }
}

/// What to copy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Want {
    /// The windows of the current workspaces, each on its own.
    pub windows: bool,
    /// Only the focused window, for Alt+Print.
    pub focused_window_only: bool,
}

/// Everything as it was at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub outputs: Vec<(OutputInfo, Shot)>,
    pub windows: Vec<(WindowInfo, Shot)>,
    /// The output the pointer is on, by its place in `outputs`, when the
    /// compositor says.
    pub pointer_output: Option<usize>,
    /// The outputs left out, each with why: no logical place, or no picture
    /// (an output that is off or changing mode).
    pub skipped: Vec<String>,
}

/// One output's copies, as they came back.
#[derive(Debug)]
pub struct OutputCopy {
    pub info: OutputInfo,
    pub plain: Result<Frame, CaptureError>,
    pub with_pointer: Option<Frame>,
    pub pointer_here: bool,
}

/// The outputs that gave a picture, the reasons for those that did not, and
/// where the pointer is among the kept ones. One output that is off or in
/// the middle of a mode change does not take the others' pictures away; only
/// when none gives a picture is it an error.
/// The kept outputs with their pictures, the reasons for the left-out ones,
/// and the pointer's output among the kept.
pub type Assembled = (Vec<(OutputInfo, Shot)>, Vec<String>, Option<usize>);

pub fn assemble_outputs(copies: Vec<OutputCopy>) -> Result<Assembled, CaptureError> {
    let mut outputs = Vec::new();
    let mut skipped = Vec::new();
    let mut pointer = None;
    for copy in copies {
        match copy.plain {
            Ok(plain) => {
                if copy.pointer_here {
                    pointer = Some(outputs.len());
                }
                outputs.push((
                    copy.info,
                    Shot {
                        plain,
                        with_pointer: copy.with_pointer,
                    },
                ));
            }
            Err(error) => skipped.push(format!("{}: {error}", copy.info.name)),
        }
    }
    if outputs.is_empty() {
        return Err(if skipped.is_empty() {
            CaptureError::NoOutputs
        } else {
            CaptureError::Failed(skipped.join("; "))
        });
    }
    Ok((outputs, skipped, pointer))
}

#[cfg(test)]
mod tests {
    use super::{CaptureError, OutputCopy, OutputInfo, assemble_outputs};
    use screenshot_core::compose::Frame;
    use screenshot_core::geometry::Rect;

    fn copy(name: &str, x: i32, plain: Result<Frame, CaptureError>, pointer: bool) -> OutputCopy {
        OutputCopy {
            info: OutputInfo {
                name: name.into(),
                logical: Rect::new(x, 0, 10, 10),
            },
            plain,
            with_pointer: None,
            pointer_here: pointer,
        }
    }

    #[test]
    fn an_output_without_a_picture_is_skipped_and_the_rest_are_kept() {
        let (outputs, skipped, pointer) = assemble_outputs(vec![
            copy("A", 0, Err(CaptureError::Timeout), false),
            copy("B", 10, Ok(Frame::filled(10, 10, [0, 0, 0, 255])), true),
        ])
        .unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0.name, "B");
        assert_eq!(pointer, Some(0));
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].starts_with("A: "));
    }

    #[test]
    fn no_picture_at_all_is_an_error_that_says_why() {
        let error = assemble_outputs(vec![copy(
            "A",
            0,
            Err(CaptureError::Failed("Stopped".into())),
            false,
        )])
        .unwrap_err();
        assert!(error.to_string().contains("A: "));
        assert!(matches!(
            assemble_outputs(Vec::new()),
            Err(CaptureError::NoOutputs)
        ));
    }
}

/// Why nothing was copied.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no Wayland compositor to talk to: {0}")]
    Connect(String),
    #[error("the compositor does not offer {0}")]
    Missing(&'static str),
    #[error("the compositor refused the copy: {0}")]
    Failed(String),
    #[error("the compositor did not answer in time")]
    Timeout,
    #[error("the Wayland connection failed: {0}")]
    Protocol(String),
    #[error("no outputs")]
    NoOutputs,
}

/// Where the pixels come from.
pub trait Capture {
    /// Copies the outputs, and the windows when asked.
    fn snapshot(&mut self, want: Want) -> Result<Snapshot, CaptureError>;
}

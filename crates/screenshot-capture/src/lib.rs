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

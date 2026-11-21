//! A capture with no compositor behind it, for tests: it answers with the
//! snapshot it was given, or with the error it was given.

use crate::{Capture, CaptureError, Snapshot, Want};

/// What the fake answers.
#[derive(Debug, Default)]
pub struct FakeCapture {
    pub snapshot: Snapshot,
    /// When set, every call fails with this message.
    pub refuse: Option<String>,
    /// Every `Want` it was asked for, in order.
    pub asked: Vec<Want>,
}

impl FakeCapture {
    pub fn answering(snapshot: Snapshot) -> FakeCapture {
        FakeCapture {
            snapshot,
            ..FakeCapture::default()
        }
    }
}

impl Capture for FakeCapture {
    fn snapshot(&mut self, want: Want) -> Result<Snapshot, CaptureError> {
        self.asked.push(want);
        if let Some(reason) = &self.refuse {
            return Err(CaptureError::Failed(reason.clone()));
        }
        let mut snapshot = self.snapshot.clone();
        if want.focused_window_only {
            snapshot.outputs.clear();
            snapshot.windows.retain(|(window, _)| window.activated);
            if snapshot.windows.is_empty() {
                return Err(CaptureError::Failed("no window has the keyboard".into()));
            }
        } else if !want.windows {
            snapshot.windows.clear();
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::FakeCapture;
    use crate::{Capture, OutputInfo, Shot, Snapshot, Want, WindowInfo};
    use screenshot_core::compose::Frame;
    use screenshot_core::geometry::Rect;

    fn snapshot() -> Snapshot {
        let shot = Shot {
            plain: Frame::filled(2, 2, [0, 0, 0, 255]),
            with_pointer: None,
        };
        Snapshot {
            outputs: vec![(
                OutputInfo {
                    name: "WINIT-0".into(),
                    logical: Rect::new(0, 0, 2, 2),
                },
                shot.clone(),
            )],
            windows: vec![
                (
                    WindowInfo {
                        title: "a".into(),
                        app_id: "a".into(),
                        output: Some("WINIT-0".into()),
                        activated: false,
                        logical_width: None,
                    },
                    shot.clone(),
                ),
                (
                    WindowInfo {
                        title: "b".into(),
                        app_id: "b".into(),
                        output: Some("WINIT-0".into()),
                        activated: true,
                        logical_width: None,
                    },
                    shot,
                ),
            ],
            pointer_output: Some(0),
            skipped: Vec::new(),
        }
    }

    #[test]
    fn it_answers_what_was_asked() {
        let mut fake = FakeCapture::answering(snapshot());
        assert!(fake.snapshot(Want::default()).unwrap().windows.is_empty());
        let all = fake
            .snapshot(Want {
                windows: true,
                ..Want::default()
            })
            .unwrap();
        assert_eq!(all.windows.len(), 2);
        let focused = fake
            .snapshot(Want {
                focused_window_only: true,
                ..Want::default()
            })
            .unwrap();
        assert_eq!(focused.windows.len(), 1);
        assert_eq!(focused.windows[0].0.title, "b");
        assert!(focused.outputs.is_empty());
        assert_eq!(fake.asked.len(), 3);
    }

    #[test]
    fn a_refusal_is_an_error() {
        let mut fake = FakeCapture {
            refuse: Some("denied".into()),
            ..FakeCapture::default()
        };
        assert!(fake.snapshot(Want::default()).is_err());
    }
}

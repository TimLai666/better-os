//! The interface's state (spec C1, C11–C13, P1–P4, K2, A1): which type is
//! chosen, picture or recording, the pointer, which screen and window, what
//! is available, and what each key does. GNOME keeps this in the toggle
//! buttons of `ScreenshotUI` (`js/ui/screenshot.js` 1063–2205); here it is
//! one value the views draw.

use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect};

/// What is captured.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Selection,
    Screen,
    Window,
}

/// A picture or a recording.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Picture,
    Recording,
}

/// Why the interface opened (GNOME's `UIMode`, 1052).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Purpose {
    /// Print Screen: a picture first, recording available.
    #[default]
    Picture,
    /// Ctrl+Shift+Alt+R: recording first.
    Recording,
    /// The portal: a picture only, the recording button unavailable.
    PictureOnly,
}

/// A direction for the arrow keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// A key the interface reacts to, as the window reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Enter,
    Space,
    Escape,
    Arrow(Direction),
    /// A letter, lowercase.
    Letter(char),
    Other,
}

/// What a key does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Capture,
    Close,
    Choose(Kind),
    TogglePointer,
    ToggleMode,
    Navigate(Direction),
    Nothing,
}

/// The choices that outlive one opening: GNOME keeps the type and the
/// pointer switch for as long as the shell runs; Better Screenshot keeps them
/// in its state file with the area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    pub kind: Kind,
    pub show_pointer: bool,
}

/// The interface, open.
#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub kind: Kind,
    pub mode: Mode,
    pub purpose: Purpose,
    pub show_pointer: bool,
    /// Whether this machine can record (GStreamer's encoder is there).
    pub recording_supported: bool,
    /// Whether any output has a window to take.
    pub has_windows: bool,
    /// The chosen output, by its place in the output list.
    pub screen: usize,
    /// The chosen window, by its place in the window list.
    pub window: Option<usize>,
}

impl Interface {
    /// The interface as it opens (`open` 1565–1606): the kept type and
    /// pointer, Selection when the kept type is Window and Window is not
    /// available, recording mode for [`Purpose::Recording`] when recording
    /// is supported.
    pub fn open(
        kept: Kept,
        purpose: Purpose,
        recording_supported: bool,
        has_windows: bool,
        screen: usize,
        window: Option<usize>,
    ) -> Interface {
        let mut interface = Interface {
            kind: kept.kind,
            mode: Mode::Picture,
            purpose,
            show_pointer: kept.show_pointer,
            recording_supported,
            has_windows,
            screen,
            window,
        };
        if purpose == Purpose::Recording && recording_supported {
            interface.mode = Mode::Recording;
        }
        if !interface.window_available() && interface.kind == Kind::Window {
            interface.kind = Kind::Selection;
        }
        interface
    }

    /// What is kept for next time.
    pub fn kept(&self) -> Kept {
        Kept {
            kind: self.kind,
            show_pointer: self.show_pointer,
        }
    }

    /// The Window button can be pressed (`_syncWindowButtonSensitivity`
    /// 1476): there are windows and this is not a recording.
    pub fn window_available(&self) -> bool {
        self.has_windows && self.mode == Mode::Picture
    }

    /// The recording button is shown (`_syncCastButton` 1467).
    pub fn recording_shown(&self) -> bool {
        self.recording_supported
    }

    /// The recording button can be pressed.
    pub fn recording_available(&self) -> bool {
        self.recording_supported && self.purpose != Purpose::PictureOnly
    }

    /// Chooses a type. Window is refused while it is not available.
    pub fn choose(&mut self, kind: Kind) -> bool {
        if kind == Kind::Window && !self.window_available() {
            return false;
        }
        self.kind = kind;
        true
    }

    /// Picture or recording. Recording cannot take a window, so the type
    /// falls back to Selection (`_onCastButtonToggled` 1839–1841).
    pub fn set_mode(&mut self, mode: Mode) -> bool {
        if mode == Mode::Recording && !self.recording_available() {
            return false;
        }
        self.mode = mode;
        if mode == Mode::Recording && self.kind == Kind::Window {
            self.kind = Kind::Selection;
        }
        true
    }

    /// What a key does (`vfunc_key_press_event` 2140–2204). Ctrl+C captures
    /// before C chooses Screen; W and V do nothing while their buttons are
    /// unavailable.
    pub fn command_for(&self, key: Key, ctrl: bool) -> Command {
        match key {
            Key::Enter | Key::Space => Command::Capture,
            Key::Letter('c') if ctrl => Command::Capture,
            Key::Escape => Command::Close,
            Key::Letter('s') => Command::Choose(Kind::Selection),
            Key::Letter('c') => Command::Choose(Kind::Screen),
            Key::Letter('w') if self.window_available() => Command::Choose(Kind::Window),
            Key::Letter('p') => Command::TogglePointer,
            Key::Letter('v') if self.recording_available() => Command::ToggleMode,
            Key::Arrow(direction) => Command::Navigate(direction),
            _ => Command::Nothing,
        }
    }

    /// Applies a command that changes the interface, and says whether it
    /// did. Capture and Close are the window's to carry out.
    pub fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Choose(kind) => self.choose(kind),
            Command::TogglePointer => {
                self.show_pointer = !self.show_pointer;
                true
            }
            Command::ToggleMode => {
                let next = match self.mode {
                    Mode::Picture => Mode::Recording,
                    Mode::Recording => Mode::Picture,
                };
                self.set_mode(next)
            }
            Command::Capture | Command::Close | Command::Navigate(_) | Command::Nothing => false,
        }
    }
}

/// The nearest rectangle in a direction from the current one, as St's
/// `navigate_focus` moves focus between the screen buttons or the window
/// pictures: candidates must lie that way from the current one's centre, and
/// the closest by distance between centres, the direction's axis counting
/// double off-line, wins.
pub fn navigate(rects: &[Rect], current: Option<usize>, direction: Direction) -> Option<usize> {
    let Some(current) = current.filter(|index| *index < rects.len()) else {
        return (!rects.is_empty()).then_some(0);
    };
    let from: Point = rects[current].center();
    let mut best: Option<(usize, f64)> = None;
    for (index, rect) in rects.iter().enumerate() {
        if index == current {
            continue;
        }
        let to = rect.center();
        let (along, across) = match direction {
            Direction::Left => (from.x - to.x, (to.y - from.y).abs()),
            Direction::Right => (to.x - from.x, (to.y - from.y).abs()),
            Direction::Up => (from.y - to.y, (to.x - from.x).abs()),
            Direction::Down => (to.y - from.y, (to.x - from.x).abs()),
        };
        if along <= 0.0 {
            continue;
        }
        let score = along + 2.0 * across;
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((index, score));
        }
    }
    best.map(|(index, _)| index).or(Some(current))
}

#[cfg(test)]
mod tests {
    use super::{Command, Direction, Interface, Kept, Key, Kind, Mode, Purpose, navigate};
    use crate::geometry::Rect;

    fn open(purpose: Purpose) -> Interface {
        Interface::open(Kept::default(), purpose, true, true, 0, Some(0))
    }

    #[test]
    fn it_opens_on_selection_in_picture_mode_with_the_pointer_off() {
        let interface = open(Purpose::Picture);
        assert_eq!(interface.kind, Kind::Selection);
        assert_eq!(interface.mode, Mode::Picture);
        assert!(!interface.show_pointer);
        assert!(interface.recording_available());
    }

    #[test]
    fn the_recording_key_opens_in_recording_mode_when_it_can() {
        assert_eq!(open(Purpose::Recording).mode, Mode::Recording);
        let unsupported =
            Interface::open(Kept::default(), Purpose::Recording, false, true, 0, None);
        assert_eq!(unsupported.mode, Mode::Picture);
        assert!(!unsupported.recording_shown());
    }

    #[test]
    fn the_portal_gets_picture_only() {
        let mut interface = open(Purpose::PictureOnly);
        assert!(interface.recording_shown());
        assert!(!interface.recording_available());
        assert!(!interface.set_mode(Mode::Recording));
        assert_eq!(
            interface.command_for(Key::Letter('v'), false),
            Command::Nothing
        );
    }

    #[test]
    fn a_kept_window_choice_falls_back_to_selection_without_windows() {
        let kept = Kept {
            kind: Kind::Window,
            show_pointer: true,
        };
        let without = Interface::open(kept, Purpose::Picture, true, false, 0, None);
        assert_eq!(without.kind, Kind::Selection);
        assert!(without.show_pointer);
        let with = Interface::open(kept, Purpose::Picture, true, true, 0, Some(1));
        assert_eq!(with.kind, Kind::Window);
        // Recording cannot take a window.
        let recording = Interface::open(kept, Purpose::Recording, true, true, 0, Some(1));
        assert_eq!(recording.kind, Kind::Selection);
    }

    #[test]
    fn recording_mode_takes_the_window_choice_away() {
        let mut interface = open(Purpose::Picture);
        assert!(interface.choose(Kind::Window));
        assert!(interface.set_mode(Mode::Recording));
        assert_eq!(interface.kind, Kind::Selection);
        assert!(!interface.window_available());
        assert!(!interface.choose(Kind::Window));
        assert_eq!(
            interface.command_for(Key::Letter('w'), false),
            Command::Nothing
        );
        interface.set_mode(Mode::Picture);
        assert!(interface.window_available());
    }

    #[test]
    fn every_key_does_what_gnome_does() {
        let interface = open(Purpose::Picture);
        assert_eq!(interface.command_for(Key::Enter, false), Command::Capture);
        assert_eq!(interface.command_for(Key::Space, false), Command::Capture);
        assert_eq!(
            interface.command_for(Key::Letter('c'), true),
            Command::Capture
        );
        assert_eq!(interface.command_for(Key::Escape, false), Command::Close);
        assert_eq!(
            interface.command_for(Key::Letter('s'), false),
            Command::Choose(Kind::Selection)
        );
        assert_eq!(
            interface.command_for(Key::Letter('c'), false),
            Command::Choose(Kind::Screen)
        );
        assert_eq!(
            interface.command_for(Key::Letter('w'), false),
            Command::Choose(Kind::Window)
        );
        assert_eq!(
            interface.command_for(Key::Letter('p'), false),
            Command::TogglePointer
        );
        assert_eq!(
            interface.command_for(Key::Letter('v'), false),
            Command::ToggleMode
        );
        assert_eq!(
            interface.command_for(Key::Arrow(Direction::Left), false),
            Command::Navigate(Direction::Left)
        );
        assert_eq!(
            interface.command_for(Key::Letter('x'), false),
            Command::Nothing
        );
        assert_eq!(interface.command_for(Key::Other, false), Command::Nothing);
    }

    #[test]
    fn toggles_flip_and_choices_stick() {
        let mut interface = open(Purpose::Picture);
        assert!(interface.apply(Command::TogglePointer));
        assert!(interface.show_pointer);
        assert!(interface.apply(Command::ToggleMode));
        assert_eq!(interface.mode, Mode::Recording);
        assert!(interface.apply(Command::ToggleMode));
        assert_eq!(interface.mode, Mode::Picture);
        assert!(interface.apply(Command::Choose(Kind::Screen)));
        assert_eq!(interface.kept().kind, Kind::Screen);
        assert!(!interface.apply(Command::Capture));
    }

    #[test]
    fn arrows_move_to_the_nearest_rectangle_that_way() {
        // Two outputs side by side, and a third below the first.
        let rects = [
            Rect::new(0, 0, 100, 100),
            Rect::new(100, 0, 100, 100),
            Rect::new(0, 100, 100, 100),
        ];
        assert_eq!(navigate(&rects, Some(0), Direction::Right), Some(1));
        assert_eq!(navigate(&rects, Some(1), Direction::Left), Some(0));
        assert_eq!(navigate(&rects, Some(0), Direction::Down), Some(2));
        assert_eq!(navigate(&rects, Some(2), Direction::Up), Some(0));
        // Nothing that way: stay.
        assert_eq!(navigate(&rects, Some(0), Direction::Left), Some(0));
        // Nothing chosen yet: the first.
        assert_eq!(navigate(&rects, None, Direction::Left), Some(0));
        assert_eq!(navigate(&[], None, Direction::Left), None);
    }
}

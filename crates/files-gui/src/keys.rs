//! Every keystroke the window understands, decided without a window.
//!
//! Issue #6 asks for keyboard-only operation "from the architecture stage", so
//! the mapping from a keystroke to a command is a pure function over a key
//! name and three modifier flags. That is what lets the keyboard coverage be a
//! test rather than a manual pass: the test asks this function what `Ctrl+H`
//! does, and gets the same answer the window gets.
//!
//! Nothing here touches the model. A command is an intention; carrying it out
//! is the window's job, and a command the current location cannot honour is
//! refused there with a reason rather than being absent from this table.

/// What one keystroke asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    // Navigation
    GoBack,
    GoForward,
    GoToParent,
    Reload,
    FocusPathField,

    // Tabs
    NewTab,
    CloseTab,
    RestoreClosedTab,
    NextTab,
    PreviousTab,

    // View
    ToggleHidden,
    ToggleViewMode,
    LargerItems,
    SmallerItems,

    // Selection and movement
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PageUp,
    PageDown,
    MoveToStart,
    MoveToEnd,
    SelectAll,
    ClearSelection,
    /// Extend the selection to the entry the movement lands on.
    ExtendUp,
    ExtendDown,

    // Opening
    Open,

    // Operations
    NewFolder,
    Rename,
    Copy,
    Cut,
    Paste,
    Duplicate,
    MoveToTrash,
    DeletePermanently,
    RestoreFromTrash,
    ToggleOperations,

    // Sidebar
    /// Move the focused bookmark one place earlier. The keyboard-accessible
    /// alternative to dragging, which Issue #6 requires.
    MoveBookmarkUp,
    MoveBookmarkDown,
    RemoveBookmark,

    /// A printable character, for type-ahead.
    TypeAhead(char),
}

/// The modifier state, in the three flags that decide anything here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        control: false,
        shift: false,
        alt: false,
    };

    pub fn control() -> Self {
        Self {
            control: true,
            ..Self::NONE
        }
    }

    pub fn control_shift() -> Self {
        Self {
            control: true,
            shift: true,
            alt: false,
        }
    }

    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Self::NONE
        }
    }

    pub fn alt() -> Self {
        Self {
            alt: true,
            ..Self::NONE
        }
    }

    fn bare(self) -> bool {
        !self.control && !self.alt
    }
}

/// Which pane the keyboard is in.
///
/// A few keys mean different things in the sidebar than in the content area —
/// `Alt+Up` reorders a bookmark there and goes to the parent folder here — so
/// focus is an argument rather than something this table guesses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Focus {
    #[default]
    Content,
    Sidebar,
}

/// Maps one keystroke.
///
/// `key` is GPUI's key name: a single character for a printable key, and a
/// word such as `enter`, `backspace`, `up`, or `f2` otherwise.
pub fn command_for(
    key: &str,
    modifiers: Modifiers,
    key_char: Option<&str>,
    focus: Focus,
) -> Option<Command> {
    let Modifiers {
        control,
        shift,
        alt,
    } = modifiers;

    if focus == Focus::Sidebar {
        match key {
            // The keyboard-accessible alternative to dragging a bookmark,
            // which Issue #6 requires alongside the drag.
            "up" if alt => return Some(Command::MoveBookmarkUp),
            "down" if alt => return Some(Command::MoveBookmarkDown),
            "delete" if modifiers.bare() && !shift => return Some(Command::RemoveBookmark),
            "f2" => return Some(Command::Rename),
            "enter" if modifiers.bare() && !shift => return Some(Command::Open),
            _ => {}
        }
    }

    match key {
        "left" if alt => return Some(Command::GoBack),
        "right" if alt => return Some(Command::GoForward),
        "up" if alt && !shift => return Some(Command::GoToParent),
        "backspace" if modifiers.bare() && !shift => return Some(Command::GoToParent),
        "l" if control => return Some(Command::FocusPathField),
        "r" if control => return Some(Command::Reload),
        "f5" => return Some(Command::Reload),
        "t" if control && !shift => return Some(Command::NewTab),
        "w" if control && !shift => return Some(Command::CloseTab),
        "t" if control && shift => return Some(Command::RestoreClosedTab),
        "tab" if control && !shift => return Some(Command::NextTab),
        "tab" if control && shift => return Some(Command::PreviousTab),
        "h" if control => return Some(Command::ToggleHidden),
        "1" if control => return Some(Command::ToggleViewMode),
        "=" | "+" if control => return Some(Command::LargerItems),
        "-" if control => return Some(Command::SmallerItems),
        "a" if control => return Some(Command::SelectAll),
        "c" if control => return Some(Command::Copy),
        "x" if control => return Some(Command::Cut),
        "v" if control => return Some(Command::Paste),
        "d" if control => return Some(Command::Duplicate),
        "n" if control && shift => return Some(Command::NewFolder),
        "o" if control => return Some(Command::ToggleOperations),
        "z" if control => return Some(Command::RestoreFromTrash),
        "f2" => return Some(Command::Rename),
        "delete" if shift && modifiers.bare() => return Some(Command::DeletePermanently),
        "delete" if modifiers.bare() => return Some(Command::MoveToTrash),
        "enter" if modifiers.bare() && !shift => return Some(Command::Open),
        "escape" => return Some(Command::ClearSelection),
        _ => {}
    }

    if modifiers.bare() {
        match key {
            "up" if shift => return Some(Command::ExtendUp),
            "down" if shift => return Some(Command::ExtendDown),
            "up" => return Some(Command::MoveUp),
            "down" => return Some(Command::MoveDown),
            "left" => return Some(Command::MoveLeft),
            "right" => return Some(Command::MoveRight),
            "pageup" => return Some(Command::PageUp),
            "pagedown" => return Some(Command::PageDown),
            "home" => return Some(Command::MoveToStart),
            "end" => return Some(Command::MoveToEnd),
            _ => {}
        }
        // Type-ahead is last, so a printable key that means something else
        // has already been claimed above.
        if let Some(character) = printable(key, key_char) {
            return Some(Command::TypeAhead(character));
        }
    }
    None
}

/// The character a keystroke would have typed, when it typed one.
fn printable(key: &str, key_char: Option<&str>) -> Option<char> {
    let source = key_char.unwrap_or(key);
    let mut characters = source.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        // A named key such as `enter`, not a character.
        return None;
    }
    (!character.is_control()).then_some(character)
}

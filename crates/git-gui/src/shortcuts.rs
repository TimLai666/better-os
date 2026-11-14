//! The buttons that are always at the toolbar's right end while a
//! repository is open, as GitHub Desktop's "No local changes" page offers
//! them: open the repository in the editor, show it in Better Files, open a
//! terminal there, and view it on GitHub when it is hosted there.

use crate::keys::Action;

/// One of the toolbar's shortcut buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shortcut {
    Editor,
    Files,
    Terminal,
    GitHub,
}

impl Shortcut {
    /// What pressing it does; the keyboard shortcut is the action's.
    pub fn action(self) -> Action {
        match self {
            Shortcut::Editor => Action::OpenInEditor,
            Shortcut::Files => Action::ShowInFiles,
            Shortcut::Terminal => Action::OpenInTerminal,
            Shortcut::GitHub => Action::ViewOnGitHub,
        }
    }
}

/// The buttons shown, in order. View on GitHub only for a repository on
/// GitHub or GitHub Enterprise.
pub fn shown(hosted: bool) -> Vec<Shortcut> {
    let mut buttons = vec![Shortcut::Editor, Shortcut::Files, Shortcut::Terminal];
    if hosted {
        buttons.push(Shortcut::GitHub);
    }
    buttons
}

/// Roughly how wide a line of Body text is, in pixels: Latin letters about
/// 8px, CJK characters a full 14px em. Generous on purpose: the cost of
/// guessing wide is icons a little early, the cost of guessing narrow is a
/// toolbar that pushes Settings off the window.
pub fn text_width(text: &str) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { 8.0 } else { 14.0 })
        .sum()
}

/// One of the toolbar's drop-down items (repository, worktree, branch) at
/// the width it starts at: 16px padding, a 16px icon, 12px gaps, the wider
/// of its two lines, a 12px chevron, and the item's own 200 to 300px
/// limits. A narrow window shrinks it from there, with an ellipsis.
pub fn item_width(caption: &str, value: &str) -> f32 {
    (16.0 + 16.0 + 12.0 + text_width(caption).max(text_width(value)) + 12.0 + 12.0 + 16.0)
        .clamp(200.0, 300.0)
}

/// What the rest of the toolbar takes besides the drop-down items: the
/// sync button (at least 200px, room for "Last fetched {n} minutes ago"),
/// the checks item when there is one, and the 48px Settings button.
const SYNC_WIDTH: f32 = 280.0;
const CHECKS_WIDTH: f32 = 300.0;
const SETTINGS_WIDTH: f32 = 48.0;

/// A shortcut button with its label: 11px padding each side, a 16px icon,
/// an 8px gap, the label, and 4px to the next button.
fn labelled_width(label: &str) -> f32 {
    11.0 + 16.0 + 8.0 + text_width(label) + 11.0 + 4.0
}

/// Whether the shortcut buttons drop their labels: when, with labels, the
/// toolbar would be wider than the window with its drop-down items at the
/// width they start at. Settings is never pushed off, and the buttons, once
/// only icons, are never pushed off either: the items shrink instead.
pub fn compact<'a>(
    window_width: f32,
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
    labels: &[String],
    checks: bool,
) -> bool {
    let fixed = items
        .into_iter()
        .map(|(caption, value)| item_width(caption, value))
        .sum::<f32>()
        + SYNC_WIDTH
        + if checks { CHECKS_WIDTH } else { 0.0 }
        + SETTINGS_WIDTH;
    let buttons = 17.0 + labels.iter().map(|l| labelled_width(l)).sum::<f32>();
    fixed + buttons > window_width
}

/// Roughly how wide a line of Caption text (12px) is: Latin letters about
/// 6.5px, CJK characters a full 12px em. Generous, as [`text_width`] is.
pub fn caption_width(text: &str) -> f32 {
    text.chars()
        .map(|c| if c.is_ascii() { 6.5 } else { 12.0 })
        .sum()
}

/// How narrow a drop-down item gets with its caption shown: the caption
/// whole, with the item's padding, icon, gaps, and chevron. Only the value
/// line shrinks (ellipsis) down to here.
pub fn item_min_width(caption: &str) -> f32 {
    (16.0 + 16.0 + 12.0 + caption_width(caption) + 12.0 + 12.0 + 16.0).max(ITEM_MIN_BARE)
}

/// How narrow a drop-down item gets with its caption hidden: its icon, a
/// few letters before the ellipsis, and its chevron.
pub const ITEM_MIN_BARE: f32 = 112.0;
/// How narrow the sync button gets: its icon and "Fetch origin".
pub const SYNC_MIN: f32 = 152.0;
/// The checks item at its narrowest: its icon, a few letters, the re-run
/// button as an icon, and the pull request's number.
const CHECKS_MIN: f32 = 180.0;

/// The shortcut buttons as icons: 16px of padding and border, then 32px
/// buttons 4px apart.
fn icons_width(buttons: usize) -> f32 {
    17.0 + 36.0 * buttons as f32
}

/// Whether the drop-down items keep their captions ("Current repository")
/// in a window this wide: when every item fits at [`item_min_width`] beside
/// the narrowest sync button, the checks item, the shortcut buttons as
/// icons, and Settings. Otherwise the captions are hidden and the items
/// shrink to [`ITEM_MIN_BARE`], so no caption is ever cut.
pub fn captions_fit<'a>(
    window_width: f32,
    captions: impl IntoIterator<Item = &'a str>,
    buttons: usize,
    checks: bool,
) -> bool {
    let cells: f32 = captions.into_iter().map(item_min_width).sum();
    cells
        + SYNC_MIN
        + if checks { CHECKS_WIDTH } else { 0.0 }
        + icons_width(buttons)
        + SETTINGS_WIDTH
        <= window_width
}

/// The toolbar at its narrowest, captions hidden and the shortcut buttons
/// as icons: what the minimum window width has to hold.
pub fn narrowest(items: usize, buttons: usize, checks: bool) -> f32 {
    ITEM_MIN_BARE * items as f32
        + SYNC_MIN
        + if checks { CHECKS_MIN } else { 0.0 }
        + icons_width(buttons)
        + SETTINGS_WIDTH
}

/// The editor's name for "Open in {editor}": the command from Settings, or
/// the first editor found, under the name people know it by. `None` when
/// the editor is the desktop's default handler, which has no name.
pub fn editor_name(setting: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let program = match setting.split_whitespace().next() {
        Some(program) => program.to_string(),
        None => crate::settings::EDITORS
            .iter()
            .find(|e| exists(e))?
            .to_string(),
    };
    let file = std::path::Path::new(&program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(program);
    let known = match file.as_str() {
        "code" => "Visual Studio Code",
        "codium" => "VSCodium",
        "zed" | "zeditor" => "Zed",
        "subl" => "Sublime Text",
        "gnome-text-editor" => "GNOME Text Editor",
        "gedit" => "gedit",
        "kate" => "Kate",
        "mousepad" => "Mousepad",
        "xdg-open" => return None,
        other => return Some(other.to_string()),
    };
    Some(known.to_string())
}

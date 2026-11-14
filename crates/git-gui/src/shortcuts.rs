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

/// One of the toolbar's drop-down items (repository, branch): 16px padding,
/// a 16px icon, 12px gaps, the wider of its two lines, a 12px chevron, and
/// the item's own 200 to 300px limits.
pub fn item_width(caption: &str, value: &str) -> f32 {
    (16.0 + 16.0 + 12.0 + text_width(caption).max(text_width(value)) + 12.0 + 12.0 + 16.0)
        .clamp(200.0, 300.0)
}

/// What the rest of the toolbar takes besides the two drop-down items: the
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
/// toolbar would be wider than the window. Settings is never pushed off.
pub fn compact(
    window_width: f32,
    items: [(&str, &str); 2],
    labels: &[String],
    checks: bool,
) -> bool {
    let fixed = items
        .iter()
        .map(|(caption, value)| item_width(caption, value))
        .sum::<f32>()
        + SYNC_WIDTH
        + if checks { CHECKS_WIDTH } else { 0.0 }
        + SETTINGS_WIDTH;
    let buttons = 17.0 + labels.iter().map(|l| labelled_width(l)).sum::<f32>();
    fixed + buttons > window_width
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

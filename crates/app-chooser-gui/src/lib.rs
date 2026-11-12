//! The reusable Better App Chooser surface.
//!
//! `AppChooser` is a plain GPUI view drawn as a WinUI content dialog: a
//! title naming the file, a search box, the recommended apps with their own
//! icons, "Other apps" folded away, an "Always use this app" check box, and
//! Open and Cancel. Better Files embeds it and subscribes to
//! [`ChooserEvent`]; the binary in this crate opens it in a dialog-sized
//! window of its own.
//!
//! Everything the surface decides comes from `app-chooser-core` (which apps,
//! in what order, why) and from the plain model below (what the list shows,
//! where the keyboard goes, how tall the dialog is, which sentence explains
//! an app). The chooser renders, localizes, and routes the actions; it owns
//! no ranking rule and no association logic of its own.

pub mod chooser;
pub mod cli;
pub mod i18n;
#[cfg(test)]
mod tests;

use app_catalog_core::DesktopId;
use app_chooser_core::{ChooserSections, Compatibility, RankedApplication};

pub use chooser::{AppChooser, ChooserEvent, ChooserMode, ChooserTarget};
pub use i18n::Locale;

/// The dialog's width: inside WinUI's 320 to 548px content dialog range,
/// wide enough for a long app name and the check box sentence in zh-TW.
pub const DIALOG_WIDTH: f32 = 480.0;
/// The tallest the dialog grows before its list scrolls.
pub const MAX_DIALOG_HEIGHT: f32 = 640.0;
/// The window's minimum size: a short list, the check box, and the buttons.
pub const MIN_WINDOW_WIDTH: f32 = 400.0;
pub const MIN_WINDOW_HEIGHT: f32 = 360.0;

/// One line of the app list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Line<'a> {
    App(&'a RankedApplication),
    /// The "Other apps" row that folds the rest away, with how many it
    /// holds.
    OtherApps {
        count: usize,
        expanded: bool,
    },
}

impl Line<'_> {
    pub fn app(&self) -> Option<&RankedApplication> {
        match self {
            Line::App(app) => Some(app),
            Line::OtherApps { .. } => None,
        }
    }
}

/// Whether a name answers a search: a word of it begins with what was
/// typed, case and accents aside.
pub fn name_matches(name: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let name = name.to_lowercase();
    name.starts_with(&query)
        || name
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word.starts_with(&query))
        || (!query.is_ascii() && name.contains(&query))
}

/// What the list shows. With nothing typed: the recommended apps, then the
/// "Other apps" row, then, once it is opened, every other app. A machine
/// with nothing recommended opens it. With something typed: every app whose
/// name answers it, recommended first, and no folding.
pub fn lines<'a>(sections: &'a ChooserSections, query: &str, expanded: bool) -> Vec<Line<'a>> {
    if !query.trim().is_empty() {
        return sections
            .iter()
            .filter(|app| name_matches(&app.display_name, query))
            .map(Line::App)
            .collect();
    }
    let mut lines: Vec<Line<'a>> = sections.recommended.iter().map(Line::App).collect();
    let others = sections.other.len() + sections.all.len();
    if others > 0 {
        let expanded = expanded || sections.recommended.is_empty();
        lines.push(Line::OtherApps {
            count: others,
            expanded,
        });
        if expanded {
            lines.extend(sections.other.iter().chain(&sections.all).map(Line::App));
        }
    }
    lines
}

/// The app selected when the dialog opens: the current default, or the
/// first recommended app, or the first other one. Opening with something
/// selected is what keeps the layout still and the Open button usable.
pub fn initial_selection(sections: &ChooserSections) -> Option<DesktopId> {
    sections
        .iter()
        .find(|app| app.is_default)
        .or_else(|| sections.recommended.first())
        .or_else(|| sections.other.first())
        .or_else(|| sections.all.first())
        .map(|app| app.desktop_id.clone())
}

/// The app that stays selected as the list changes: the selected one while
/// it is still in the list, else the first app in it, else none. A search
/// that matches nothing leaves nothing selected, so Open cannot open an app
/// the person can no longer see.
pub fn visible_selection(lines: &[Line<'_>], selected: Option<&DesktopId>) -> Option<DesktopId> {
    let visible = |id: &DesktopId| {
        lines
            .iter()
            .any(|line| line.app().is_some_and(|app| &app.desktop_id == id))
    };
    selected.filter(|id| visible(id)).cloned().or_else(|| {
        lines
            .iter()
            .find_map(|line| line.app().map(|app| app.desktop_id.clone()))
    })
}

/// What the check box calls the kind of file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileKind {
    /// shared-mime-info's own description of the type, in the person's
    /// language: "Tar archive (gzip-compressed)".
    Described(String),
    /// The file's extension, when no description in the person's language
    /// exists.
    Extension(String),
    Unknown,
}

/// The `<comment>` a shared-mime-info type file (`mime/text/plain.xml`) gives
/// for `language` (`zh_TW`), or its untranslated one for `None`. Only that
/// exact language counts: an English sentence inside a Chinese one reads
/// worse than the extension.
pub fn mime_comment(xml: &str, language: Option<&str>) -> Option<String> {
    let mut rest = xml;
    while let Some(start) = rest.find("<comment") {
        rest = &rest[start + "<comment".len()..];
        let close = rest.find('>')?;
        let attributes = &rest[..close];
        let body = &rest[close + 1..];
        let end = body.find("</comment>")?;
        let text = body[..end].trim();
        let lang = attributes
            .split("xml:lang=\"")
            .nth(1)
            .and_then(|value| value.split('"').next());
        if lang == language && !text.is_empty() {
            return Some(
                text.replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'"),
            );
        }
        rest = &body[end..];
    }
    None
}

/// What the check box names: the type's description in the person's
/// language, else the file's extension.
pub fn file_kind(description: Option<String>, file_name: &str) -> FileKind {
    match (description, extension(file_name)) {
        (Some(description), _) => FileKind::Described(description),
        (None, Some(extension)) => FileKind::Extension(extension),
        (None, None) => FileKind::Unknown,
    }
}

/// Where Up, Down, Home, and End take the keyboard in the list. The ends
/// stop rather than wrap, as a WinUI list does.
pub fn step(
    lines: &[Line<'_>],
    current: Option<usize>,
    key: better_ui::fluent::Key,
) -> Option<usize> {
    better_ui::fluent::step_index(current, lines.len(), key, false, |_| false)
}

/// Why an app is in the list, in the words the "Why?" button shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    /// It opens this kind of file now.
    CurrentDefault,
    /// It says it opens this kind of file.
    Declares,
    /// It says it opens a broader kind that includes this one.
    Related,
    /// It says it opens files of this general kind.
    Wildcard,
    /// The person chose it for this kind of file before.
    UserAssociated,
    /// The person opened this kind of file with it before.
    PreviouslyUsed,
    /// It says nothing about this kind of file.
    NotDeclared,
}

pub fn reason(app: &RankedApplication) -> Reason {
    if app.is_default {
        return Reason::CurrentDefault;
    }
    match app.compatibility {
        Compatibility::Declares => Reason::Declares,
        Compatibility::DeclaresRelatedType { .. } => Reason::Related,
        Compatibility::DeclaresWildcard { .. } => Reason::Wildcard,
        Compatibility::UserAssociated => Reason::UserAssociated,
        Compatibility::PreviouslyUsed => Reason::PreviouslyUsed,
        Compatibility::NotDeclared => Reason::NotDeclared,
    }
}

/// The file's extension as a person types it, `.txt`, or `None` for a name
/// without one (or a hidden file such as `.bashrc`).
pub fn extension(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty() && !extension.contains(' '))
        .then(|| format!(".{}", extension.to_lowercase()))
}

/// The dialog's parts, in pixels, as `chooser.rs` draws them.
pub mod metrics {
    /// The titlebar a standalone dialog window draws.
    pub const TITLE_BAR: f32 = 32.0;
    /// 24px of padding above, the Subtitle title, 12px, the search box, 12px.
    pub const HEADER: f32 = 24.0 + 28.0 + 12.0 + 32.0 + 12.0;
    /// One list row.
    pub const ROW: f32 = 36.0;
    /// 12px, the check box, 24px of padding below.
    pub const CHECK_BOX: f32 = 12.0 + 32.0 + 24.0;
    /// The button row: 24px padding around a 32px button, and its divider.
    pub const BUTTONS: f32 = 24.0 + 32.0 + 24.0 + 1.0;
    /// An info bar's room, when one is showing.
    pub const NOTICE: f32 = 64.0;
    /// The 4px the list keeps around its rows for the focus ring, top and
    /// bottom.
    pub const LIST_PADDING: f32 = 8.0;
}

/// The most the list may take before it scrolls, so the title, the search
/// box, the check box, and the buttons always stay in the dialog.
pub fn list_height_limit(title_bar: bool, notice: bool) -> f32 {
    let mut fixed = metrics::HEADER + metrics::CHECK_BOX + metrics::BUTTONS + metrics::LIST_PADDING;
    if title_bar {
        fixed += metrics::TITLE_BAR;
    }
    if notice {
        fixed += metrics::NOTICE;
    }
    (MAX_DIALOG_HEIGHT - fixed).max(metrics::ROW * 2.0)
}

/// How tall the dialog is for this many list lines: everything it holds,
/// up to [`MAX_DIALOG_HEIGHT`], where the list starts scrolling. A dialog
/// sized to its content is what the audit asked for in place of a full
/// window around two rows.
pub fn dialog_height(lines: usize, title_bar: bool, notice: bool) -> f32 {
    let mut height =
        metrics::HEADER + metrics::CHECK_BOX + metrics::BUTTONS + metrics::LIST_PADDING;
    height += metrics::ROW * lines.max(2) as f32;
    if title_bar {
        height += metrics::TITLE_BAR;
    }
    if notice {
        height += metrics::NOTICE;
    }
    height.clamp(MIN_WINDOW_HEIGHT, MAX_DIALOG_HEIGHT)
}

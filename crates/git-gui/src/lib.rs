//! Better Git: the window, and the decisions behind it.
//!
//! [`session`] and the modules beside it hold every decision and have no
//! GPUI in them; they are tested without a display. The window ([`run`])
//! only draws the session and hands its git jobs and GitHub requests to
//! background threads, so no `git` process or network request runs on the
//! thread that paints.
//!
//! git itself is reached through `git_platform::Git`. The window uses
//! `SystemGit`; tests give the session a fake or a real `git` in a
//! temporary repository. GitHub is reached through `github_api`; tests use
//! a local fake server.

pub mod branches;
pub mod changes;
pub mod clone;
pub mod commit;
pub mod conflicts;
pub mod dialog_check;
pub mod dialogs;
pub mod github;
pub mod graph;
pub mod history;
pub mod i18n;
pub mod identity;
pub mod keys;
pub mod live;
pub mod notifications;
pub mod phrases;
pub mod publish;
pub mod recent;
pub mod remote;
pub mod repositories;
pub mod restart;
pub mod session;
pub mod settings;
pub mod shortcuts;
pub mod trust;
pub mod worktree_links;

mod app;
mod commit_menu;
mod graph_pane;
mod live_window;
mod render;
mod render_parts;
mod theme;

#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::path::PathBuf;

use better_ui::command_line::{Invocation, WindowCommandLine, WindowOption};

use crate::i18n::Locale;

pub use app::run;

/// Which appearance the window opens in. The desktop's unless asked, as
/// every Better OS window (ADR 0019).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Appearance {
    /// The desktop's, and it keeps following the desktop while open.
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    /// In the order the settings page lists them: the default first.
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    /// The stored setting, or the default when it is empty or a word this
    /// version does not know.
    pub fn stored(setting: &str) -> Self {
        Self::from_argument(setting).unwrap_or_default()
    }

    /// The same choice in the shared kit, which paints and follows it.
    pub fn fluent(self) -> better_ui::fluent::theme::Appearance {
        use better_ui::fluent::theme::Appearance as Fluent;
        match self {
            Self::System => Fluent::System,
            Self::Light => Fluent::Light,
            Self::Dark => Fluent::Dark,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    pub fn from_argument(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

/// The appearance the window opens in: the command line's when it named one,
/// else the stored one. Only the stored one is ever saved; a `--appearance`
/// is for this window and is never written into the settings.
pub fn opening_appearance(command_line: Option<Appearance>, stored: &str) -> Appearance {
    command_line.unwrap_or_else(|| Appearance::stored(stored))
}

/// What the window opens with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartOptions {
    /// The repository to open, from the first argument that is not a flag.
    pub path: Option<PathBuf>,
    pub locale: Locale,
    /// `None` keeps the stored choice.
    pub appearance: Option<Appearance>,
}

/// What to do with the command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Start {
    Open(StartOptions),
    /// Print to stdout and exit successfully.
    Print(String),
    /// Print to stderr and exit with status 2.
    Refuse(String),
}

const VALUED: [&str; 2] = ["--lang", "--appearance"];

pub const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-git",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[
        WindowOption::Valued("--lang"),
        WindowOption::Valued("--appearance"),
    ],
    refusal,
};

fn refusal(other: &str) -> String {
    format!(
        "better-git：不接受「{other}」這個參數。用法：better-git [--lang zh-TW|en-US] \
         [--appearance dark|light|system] [儲存庫路徑]。\n\
         better-git: takes no argument \"{other}\". Usage: better-git [--lang zh-TW|en-US] \
         [--appearance dark|light|system] [repository path]."
    )
}

const HELP: &str = "\
better-git — Better Git

用法 / Usage:
  better-git                            開啟視窗 / open the window
  better-git <路徑 / path>               開啟那個儲存庫 / open that repository
  better-git -- <路徑 / path>            路徑以 - 開頭時 / for a path starting with -
  better-git --lang zh-TW|en-US         以指定語言開啟 / open in that language
  better-git --appearance dark|light|system
                                        以指定外觀開啟 / open in that appearance
  better-git --version                  顯示版本 / print the version
  better-git --help                     顯示這段說明 / print this help";

/// Reads the arguments, without the program name.
///
/// Expected results:
/// - `--help`, `--version`, and anything that is not a declared flag are
///   answered by `better_ui::command_line`, left to right.
/// - The first argument that does not start with `-`, or anything after
///   `--`, is the repository path, taken as the bytes given.
/// - A second path is refused, and so is a `--lang` or `--appearance` value
///   the window does not know. A flag with no value keeps its default.
pub fn invocation(arguments: &[OsString]) -> Start {
    let mut flags: Vec<String> = Vec::new();
    let mut paths: Vec<OsString> = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            paths.extend(arguments.by_ref().cloned());
            break;
        }
        let text = argument.to_string_lossy();
        if text.starts_with('-') && text.len() > 1 {
            flags.push(text.to_string());
            if VALUED.contains(&text.as_ref())
                && let Some(value) = arguments.next()
            {
                flags.push(value.to_string_lossy().into_owned());
            }
        } else {
            paths.push(argument.clone());
        }
    }

    match COMMAND_LINE.invocation(&flags) {
        Invocation::OpenWindow => {}
        Invocation::Print(text) => return Start::Print(text),
        Invocation::Refuse(text) => return Start::Refuse(text),
    }
    if let Some(extra) = paths.get(1) {
        return Start::Refuse(refusal(&extra.to_string_lossy()));
    }

    let mut options = StartOptions {
        path: paths.into_iter().next().map(PathBuf::from),
        locale: Locale::System,
        appearance: None,
    };
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let (name, value) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag.as_str(), None),
        };
        let value = match value {
            Some(value) => Some(value),
            None if VALUED.contains(&name) => flags.next().cloned(),
            None => None,
        };
        let Some(value) = value else { continue };
        match name {
            "--lang" => match Locale::from_argument(&value) {
                Some(locale) => options.locale = locale,
                None => return Start::Refuse(refusal(&format!("--lang {value}"))),
            },
            "--appearance" => match Appearance::from_argument(&value) {
                Some(appearance) => options.appearance = Some(appearance),
                None => return Start::Refuse(refusal(&format!("--appearance {value}"))),
            },
            _ => {}
        }
    }
    Start::Open(options)
}

/// Where the recent repositories are kept for this user.
pub fn recent_file() -> Option<PathBuf> {
    recent::default_path()
}

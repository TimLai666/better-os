//! The person's choices, kept in `$XDG_CONFIG_HOME/better-os/git.json`, and
//! starting the programs Better Git hands a repository to: the external
//! editor, the terminal, and Better Files.
//!
//! A program is started with an argument vector, never a shell: a command
//! the person typed is split on whitespace, and the path is the last
//! argument.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    /// `system`, `light`, or `dark`. A file without it, or with a word this
    /// version does not know, follows the desktop (ADR 0019). The field is
    /// written with every save, so a stored `dark` is kept: it cannot be told
    /// apart from one written as the old default. A value that is not a
    /// word (a number, `null`) reads as `system` rather than making the whole
    /// file unreadable, which the next save would then overwrite.
    #[serde(deserialize_with = "word_or_system")]
    pub appearance: String,
    /// `system`, `en-US`, or `zh-TW`.
    pub language: String,
    /// The editor's command; empty picks the first one found.
    pub editor: String,
    /// The terminal's command; empty picks the first one found.
    pub terminal: String,
    #[serde(default = "yes")]
    pub confirm_discard: bool,
    #[serde(default = "yes")]
    pub confirm_force_push: bool,
    #[serde(default = "yes")]
    pub confirm_remove_repository: bool,
    #[serde(default = "yes")]
    pub confirm_undo_commit: bool,
    #[serde(default = "yes")]
    pub background_fetch: bool,
    pub split_diff: bool,
    pub hide_whitespace: bool,
    /// Where clones go; empty is `~/Projects`.
    pub clone_folder: String,
    /// The GitHub host signed in to (`github.com` or an Enterprise host),
    /// so the window knows which keyring item to read. Not a secret.
    pub account_host: String,
    /// The OAuth App client ID that host's sign-in used, when it is not
    /// this build's (an Enterprise server's own app). Not a secret.
    pub account_client_id: String,
    /// The Graph tab's filter: `all`, `local`, or `current`.
    pub graph_scope: String,
    #[serde(default = "yes")]
    pub graph_remotes: bool,
    #[serde(default = "yes")]
    pub graph_tags: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: "system".into(),
            language: "system".into(),
            editor: String::new(),
            terminal: String::new(),
            confirm_discard: true,
            confirm_force_push: true,
            confirm_remove_repository: true,
            confirm_undo_commit: true,
            background_fetch: true,
            split_diff: false,
            hide_whitespace: false,
            clone_folder: String::new(),
            account_host: String::new(),
            account_client_id: String::new(),
            graph_scope: "all".into(),
            graph_remotes: true,
            graph_tags: true,
        }
    }
}

/// The stored appearance word, or `system` when the value is not a string.
fn word_or_system<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Field {
        Word(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Field::deserialize(deserializer)? {
        Field::Word(word) => word,
        Field::Other(_) => "system".to_string(),
    })
}

impl Settings {
    pub fn default_file() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("better-os/git.json"))
    }

    /// Reads the file; missing or unreadable is the defaults.
    pub fn load(file: Option<&Path>) -> Settings {
        file.and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: Option<&Path>) -> std::io::Result<()> {
        let Some(file) = file else { return Ok(()) };
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial = file.with_extension("json.partial");
        std::fs::write(
            &partial,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(partial, file)
    }

    /// The folder new clones go in.
    pub fn clone_folder(&self, home: Option<&Path>) -> Option<PathBuf> {
        if !self.clone_folder.trim().is_empty() {
            return Some(PathBuf::from(self.clone_folder.trim()));
        }
        home.map(|h| h.join("Projects"))
    }
}

/// Editors looked for, in order, when none is set.
pub const EDITORS: [&str; 9] = [
    "code",
    "codium",
    "zed",
    "subl",
    "gnome-text-editor",
    "gedit",
    "kate",
    "mousepad",
    "xdg-open",
];

/// Terminals looked for, in order, when none is set.
pub const TERMINALS: [&str; 7] = [
    "gnome-terminal",
    "kgx",
    "konsole",
    "xfce4-terminal",
    "tilix",
    "alacritty",
    "x-terminal-emulator",
];

/// Whether a program is on `PATH` (or is a path that exists).
pub fn on_path(program: &str, path_var: Option<&std::ffi::OsStr>) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    path_var
        .map(|paths| std::env::split_paths(paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

/// The argument vector that starts a program for a path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub path_arg: Option<PathBuf>,
    pub working_dir: PathBuf,
}

fn split(command: &str) -> Option<(String, Vec<String>)> {
    let mut words = command.split_whitespace().map(str::to_string);
    let program = words.next()?;
    Some((program, words.collect()))
}

/// The editor for a file or folder.
pub fn editor_launch(
    settings: &Settings,
    target: &Path,
    exists: impl Fn(&str) -> bool,
) -> Option<Launch> {
    let (program, args) = match split(&settings.editor) {
        Some(found) => found,
        None => (EDITORS.iter().find(|e| exists(e))?.to_string(), Vec::new()),
    };
    Some(Launch {
        program,
        args,
        path_arg: Some(target.to_path_buf()),
        working_dir: if target.is_dir() {
            target.to_path_buf()
        } else {
            target.parent().map(Path::to_path_buf).unwrap_or_default()
        },
    })
}

/// The terminal, started in a folder: the known ones get their own
/// working-directory flag; every one starts with the folder as its working
/// directory.
pub fn terminal_launch(
    settings: &Settings,
    folder: &Path,
    exists: impl Fn(&str) -> bool,
) -> Option<Launch> {
    let (program, mut args) = match split(&settings.terminal) {
        Some(found) => found,
        None => (
            TERMINALS.iter().find(|t| exists(t))?.to_string(),
            Vec::new(),
        ),
    };
    let name = Path::new(&program)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let flag = match name.as_str() {
        "gnome-terminal" | "kgx" | "xfce4-terminal" | "tilix" => {
            Some(format!("--working-directory={}", folder.display()))
        }
        "konsole" => {
            args.push("--workdir".into());
            Some(folder.display().to_string())
        }
        "alacritty" => {
            args.push("--working-directory".into());
            Some(folder.display().to_string())
        }
        _ => None,
    };
    args.extend(flag);
    Some(Launch {
        program,
        args,
        path_arg: None,
        working_dir: folder.to_path_buf(),
    })
}

/// Better Files on the folder, or the desktop's file manager when Better
/// Files is not installed.
pub fn files_launch(folder: &Path, exists: impl Fn(&str) -> bool) -> Launch {
    let program = if exists("better-files") {
        "better-files"
    } else {
        "xdg-open"
    };
    Launch {
        program: program.into(),
        args: Vec::new(),
        path_arg: Some(folder.to_path_buf()),
        working_dir: folder.to_path_buf(),
    }
}

/// A URL in the browser.
pub fn browser_launch(url: &str) -> Launch {
    Launch {
        program: "xdg-open".into(),
        args: vec![url.to_string()],
        path_arg: None,
        working_dir: PathBuf::from("/"),
    }
}

impl Launch {
    /// Starts the program and does not wait for it. git's variables are
    /// dropped so the program sees no Better Git internals.
    pub fn spawn(&self) -> std::io::Result<()> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(path) = &self.path_arg {
            command.arg(path);
        }
        if self.working_dir.is_dir() {
            command.current_dir(&self.working_dir);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut child| {
                // Reaped from its own thread so it never lingers as a zombie.
                std::thread::spawn(move || child.wait());
            })
    }
}

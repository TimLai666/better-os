//! Plugins, as folio defines them (`docs/plugins.md`): a folder with a
//! `manifest.json` and commands. Better Files never loads a plugin's code;
//! it runs its commands, without a shell, and reads the JSON they print.
//!
//! What is supported: `requires`, `poll` (items, places, available
//! actions), a sidebar `section` of the items, item `open` (a path, a GVfs
//! mount, or a command), `actions` in the right-click menu and under Share
//! (with `to: items`, `under`, `types`, `files`, `single`, `busy`,
//! `detach`), their results (`notice`, `copy`, `path`, `error`), `details`
//! rows, and `i18n/<code>.json` catalogs. A manifest written for folio
//! (`"folio": 1`) loads as it is.
//!
//! Trust is folio's: a plugin's folder and manifest, and a command of its
//! own and the folders it is in, must belong to the person (or to root) and
//! be writable by no one else; otherwise the plugin is skipped. A command
//! runs in the plugin's folder in a process group of its own, which is
//! killed whole when it ends or times out (a poll has 10 s, details 2 s).
//! Output past 1 MB is refused.
//!
//! Plugins live in `$XDG_DATA_HOME/better-os/files/plugins/<id>/` (yours,
//! which wins) and `/usr/share/better-os/files/plugins/<id>/` (packaged), or
//! in `$BETTER_FILES_PLUGINS_PATH` in place of the packaged folder.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Output past this is refused.
pub const OUTPUT_LIMIT: usize = 1024 * 1024;
pub const POLL_TIMEOUT: Duration = Duration::from_secs(10);
pub const DETAILS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Manifest {
    #[serde(default)]
    pub folio: Option<u32>,
    #[serde(default, rename = "better-files")]
    pub better_files: Option<u32>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub poll: Option<Poll>,
    #[serde(default)]
    pub section: Option<Section>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub details: Option<Details>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Poll {
    pub command: Vec<String>,
    #[serde(default = "default_every")]
    pub every: u64,
}

fn default_every() -> u64 {
    15
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Section {
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub menu: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub under: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub files: bool,
    #[serde(default)]
    pub single: bool,
    #[serde(default)]
    pub busy: Option<String>,
    #[serde(default)]
    pub detach: bool,
}

impl Action {
    pub fn in_share(&self) -> bool {
        self.menu.as_deref() == Some("share")
    }

    pub fn to_items(&self) -> bool {
        self.to.as_deref() == Some("items")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Details {
    pub command: Vec<String>,
    #[serde(default)]
    pub under: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
}

/// Text a command prints: a string, or an English template and its values.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Text {
    Plain(String),
    Template(Vec<String>),
}

impl Text {
    /// The text in the plugin's catalog's words, values filled in.
    pub fn render(&self, catalog: &BTreeMap<String, String>) -> String {
        match self {
            Text::Plain(text) => catalog.get(text).cloned().unwrap_or_else(|| text.clone()),
            Text::Template(parts) => {
                let Some((template, values)) = parts.split_first() else {
                    return String::new();
                };
                let mut out = catalog
                    .get(template)
                    .cloned()
                    .unwrap_or_else(|| template.clone());
                for (index, value) in values.iter().enumerate().rev() {
                    out = out.replace(&format!("%{}", index + 1), value);
                }
                out
            }
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct PollOutput {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub actions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub subtitle: Option<Text>,
    #[serde(default)]
    pub open: Option<Open>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Open {
    Path { path: String },
    Mount { mount: String },
    Run { run: Vec<String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Place {
    pub path: String,
    #[serde(default)]
    pub status: Option<Text>,
    #[serde(default)]
    pub tone: Option<String>,
}

/// What an action or a run command printed.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ActionResult {
    #[serde(default)]
    pub notice: Option<Text>,
    #[serde(default)]
    pub copy: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub error: Option<Text>,
}

/// One plugin that passed every check.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub catalog: BTreeMap<String, String>,
}

/// Why a plugin was skipped, for standard error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Skipped {
    BadId,
    NotTrusted(PathBuf),
    BadManifest(String),
    WrongVersion,
    Missing(String),
}

/// A plugin id: lowercase letters, digits and dashes, starting with a letter
/// or digit.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether a path may be trusted: it belongs to `uid` or to root and is
/// writable by no one else.
pub fn trusted(path: &Path, uid: u32) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|meta| (meta.uid() == uid || meta.uid() == 0) && meta.mode() & 0o022 == 0)
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Reads one plugin folder, checking it as folio does.
pub fn load(
    dir: &Path,
    language: &str,
    installed: &dyn Fn(&str) -> bool,
) -> Result<Plugin, Skipped> {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !valid_id(&id) {
        return Err(Skipped::BadId);
    }
    let uid = current_uid();
    let manifest_path = dir.join("manifest.json");
    for path in [dir, manifest_path.as_path()] {
        if !trusted(path, uid) {
            return Err(Skipped::NotTrusted(path.to_path_buf()));
        }
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| Skipped::BadManifest(error.to_string()))?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|error| Skipped::BadManifest(error.to_string()))?;
    if manifest.folio.or(manifest.better_files) != Some(1) {
        return Err(Skipped::WrongVersion);
    }
    if let Some(missing) = manifest.requires.iter().find(|program| !installed(program)) {
        return Err(Skipped::Missing(missing.clone()));
    }
    Ok(Plugin {
        id,
        dir: dir.to_path_buf(),
        catalog: catalog(dir, language),
        manifest,
    })
}

/// The plugin's catalog for `language` (`pt-BR` falls back to `pt`).
fn catalog(dir: &Path, language: &str) -> BTreeMap<String, String> {
    let short = language.split(['-', '_']).next().unwrap_or(language);
    [language, short]
        .iter()
        .find_map(|code| {
            std::fs::read_to_string(dir.join("i18n").join(format!("{code}.json")))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
        })
        .unwrap_or_default()
}

/// The folders plugins are read from, the person's first.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = files_platform::xdg::data_dirs_from_env().into_iter().next() {
        dirs.push(data_home.join("better-os/files/plugins"));
    }
    match std::env::var_os("BETTER_FILES_PLUGINS_PATH") {
        Some(path) => dirs.push(PathBuf::from(path)),
        None => dirs.push(PathBuf::from("/usr/share/better-os/files/plugins")),
    }
    dirs
}

/// Every plugin in `dirs`, one per id, the first folder winning; those
/// skipped and why.
pub fn discover(
    dirs: &[PathBuf],
    language: &str,
    installed: &dyn Fn(&str) -> bool,
) -> (Vec<Plugin>, Vec<(PathBuf, Skipped)>) {
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut skipped = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut folders: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        folders.sort();
        for folder in folders {
            let id = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if plugins.iter().any(|plugin| plugin.id == id) {
                continue;
            }
            match load(&folder, language, installed) {
                Ok(plugin) => plugins.push(plugin),
                Err(reason) => skipped.push((folder, reason)),
            }
        }
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    (plugins, skipped)
}

/// Why a command did not give an answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    NotTrusted(PathBuf),
    NotFound(String),
    Failed {
        code: Option<i32>,
        last_error: String,
    },
    TimedOut,
    TooLarge,
}

impl Plugin {
    /// The program a command names: one with a `/` that is not absolute is
    /// in the plugin's folder and must be trusted, as must its folders.
    fn program(&self, word: &str) -> Result<PathBuf, RunError> {
        if word.contains('/') && !word.starts_with('/') {
            let path = self.dir.join(word);
            let uid = current_uid();
            let mut check = Some(path.as_path());
            while let Some(at) = check {
                if !trusted(at, uid) {
                    return Err(RunError::NotTrusted(at.to_path_buf()));
                }
                if at == self.dir {
                    break;
                }
                check = at.parent();
            }
            return Ok(path);
        }
        Ok(PathBuf::from(word))
    }

    /// Runs a command with `arguments` appended, in the plugin's folder, in a
    /// process group of its own, and returns its standard output. `limit`
    /// `None` waits as long as it takes.
    pub fn run(
        &self,
        command: &[String],
        arguments: &[String],
        env: &[(&str, String)],
        language: &str,
        limit: Option<Duration>,
    ) -> Result<Vec<u8>, RunError> {
        let Some((program, fixed)) = command.split_first() else {
            return Err(RunError::NotFound(String::new()));
        };
        let program = self.program(program)?;
        let mut child = Command::new(&program)
            .args(fixed)
            .args(arguments)
            .current_dir(&self.dir)
            .env("BETTER_FILES_PLUGIN_DIR", &self.dir)
            .env("FOLIO_PLUGIN_DIR", &self.dir)
            .env("FOLIO_LANGUAGE", language)
            .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|_| RunError::NotFound(program.display().to_string()))?;
        let group = child.id() as i32;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let out = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = (&mut stdout)
                .take(OUTPUT_LIMIT as u64 + 1)
                .read_to_end(&mut buffer);
            buffer
        });
        let err = std::thread::spawn(move || {
            let mut buffer = String::new();
            let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut buffer);
            buffer
        });
        let started = Instant::now();
        let kill_group = || {
            // SAFETY: kill with a negative pid signals the process group the
            // child was started in, which only holds its own processes.
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        };
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if limit.is_some_and(|limit| started.elapsed() > limit) => {
                    kill_group();
                    let _ = child.wait();
                    return Err(RunError::TimedOut);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => return Err(RunError::TimedOut),
            }
        };
        // Whatever it left running goes with it.
        kill_group();
        let output = out.join().unwrap_or_default();
        let errors = err.join().unwrap_or_default();
        if output.len() > OUTPUT_LIMIT {
            return Err(RunError::TooLarge);
        }
        if !status.success() {
            return Err(RunError::Failed {
                code: status.code(),
                last_error: errors
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .chars()
                    .take(1000)
                    .collect(),
            });
        }
        Ok(output)
    }

    /// Starts a command and does not wait (`"detach": true`), for one that
    /// opens a window of its own. It runs in its own process group.
    pub fn spawn_detached(
        &self,
        command: &[String],
        arguments: &[String],
        env: &[(&str, String)],
        language: &str,
    ) -> Result<(), RunError> {
        let Some((program, fixed)) = command.split_first() else {
            return Err(RunError::NotFound(String::new()));
        };
        let program = self.program(program)?;
        Command::new(&program)
            .args(fixed)
            .args(arguments)
            .current_dir(&self.dir)
            .env("BETTER_FILES_PLUGIN_DIR", &self.dir)
            .env("FOLIO_PLUGIN_DIR", &self.dir)
            .env("FOLIO_LANGUAGE", language)
            .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map(|_| ())
            .map_err(|_| RunError::NotFound(program.display().to_string()))
    }

    /// Runs the poll. A failure is an empty answer, as in folio.
    pub fn poll(&self, language: &str) -> Option<PollOutput> {
        let poll = self.manifest.poll.as_ref()?;
        let output = self
            .run(&poll.command, &[], &[], language, Some(POLL_TIMEOUT))
            .ok()?;
        serde_json::from_slice(&output).ok()
    }

    /// How often the poll runs: between 5 seconds and a day.
    pub fn poll_every(&self) -> Duration {
        Duration::from_secs(
            self.manifest
                .poll
                .as_ref()
                .map_or(15, |poll| poll.every)
                .clamp(5, 86_400),
        )
    }

    /// The details rows for one file, or none.
    pub fn details(
        &self,
        path: &Path,
        mime: Option<&str>,
        home: Option<&Path>,
        language: &str,
    ) -> Vec<(String, String)> {
        let Some(details) = &self.manifest.details else {
            return Vec::new();
        };
        if !applies(
            &details.under,
            &details.types,
            &[(path.to_path_buf(), mime.map(str::to_string), path.is_dir())],
            home,
        ) {
            return Vec::new();
        }
        let Ok(output) = self.run(
            &details.command,
            &[path.display().to_string()],
            &[],
            language,
            Some(DETAILS_TIMEOUT),
        ) else {
            return Vec::new();
        };
        let rows: Vec<(String, String)> = serde_json::from_slice(&output).unwrap_or_default();
        rows.into_iter()
            .map(|(label, value)| (self.catalog.get(&label).cloned().unwrap_or(label), value))
            .collect()
    }

    pub fn translate(&self, text: &str) -> String {
        self.catalog
            .get(text)
            .cloned()
            .unwrap_or_else(|| text.to_string())
    }
}

/// `~/…` and absolute paths, as folio's manifests write them.
pub fn expand(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => home.map(Path::to_path_buf).unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

/// Whether a selection (path, type, is a folder) meets an action's or a
/// details command's `under` and `types`.
pub fn applies(
    under: &[String],
    types: &[String],
    selection: &[(PathBuf, Option<String>, bool)],
    home: Option<&Path>,
) -> bool {
    selection.iter().all(|(path, mime, folder)| {
        let inside = under.is_empty()
            || under
                .iter()
                .any(|folder| path.starts_with(expand(folder, home)));
        let mime = if *folder {
            Some("inode/directory")
        } else {
            mime.as_deref()
        };
        let typed = types.is_empty()
            || types
                .iter()
                .any(|wanted| match (wanted.strip_suffix("/*"), mime) {
                    (Some(media), Some(mime)) => mime.starts_with(&format!("{media}/")),
                    (None, Some(mime)) => {
                        mime == wanted || (wanted == "text/plain" && mime.starts_with("text/"))
                    }
                    (_, None) => false,
                });
        inside && typed
    })
}

/// Whether an action shows for this selection, given what the last poll
/// said is available and how many items the plugin has.
pub fn action_shows(
    action: &Action,
    available: Option<&[String]>,
    selection: &[(PathBuf, Option<String>, bool)],
    items: usize,
    home: Option<&Path>,
) -> bool {
    !selection.is_empty()
        && available.is_none_or(|ids| ids.contains(&action.id))
        && !(action.files && selection.iter().any(|(_, _, folder)| *folder))
        && !(action.single && selection.len() > 1)
        && !(action.to_items() && items == 0)
        && applies(&action.under, &action.types, selection, home)
}

//! The window's side of plugins: polling them on a thread of their own,
//! keeping their latest answers, running their actions off the window's
//! thread, and fetching details rows in the background. Drawing a sidebar
//! or opening a menu never runs a command; it reads what is kept here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugins::{ActionResult, Item, Plugin, PollOutput};

/// What an action or an item's `run` asked the window to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Notice(String),
    Copy(String),
    Go(PathBuf),
    Failed(String),
}

#[derive(Default)]
struct Shared {
    plugins: Vec<(Plugin, Option<PollOutput>)>,
    outcomes: Vec<Outcome>,
    details: HashMap<PathBuf, Vec<(String, String)>>,
    pending_details: Vec<PathBuf>,
    changed: bool,
}

/// Every plugin, its last poll, and what finished since the last frame.
#[derive(Clone)]
pub struct PluginHost {
    shared: Arc<Mutex<Shared>>,
    language: String,
    home: Option<PathBuf>,
}

impl PluginHost {
    /// Reads the plugins and starts polling them.
    pub fn start(language: &str) -> Self {
        let (plugins, skipped) = crate::plugins::discover(
            &crate::plugins::plugin_dirs(),
            language,
            &crate::terminal::on_path,
        );
        for (dir, reason) in skipped {
            eprintln!("better-files: plugin {} skipped: {reason:?}", dir.display());
        }
        let host = Self::with(plugins, language);
        host.poll_forever();
        host
    }

    /// A host over these plugins, not polling yet. For tests.
    pub fn with(plugins: Vec<Plugin>, language: &str) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                plugins: plugins.into_iter().map(|plugin| (plugin, None)).collect(),
                ..Shared::default()
            })),
            language: language.to_string(),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    fn poll_forever(&self) {
        let host = self.clone();
        let _ = std::thread::Builder::new()
            .name("files-plugins".to_string())
            .spawn(move || {
                let mut due: HashMap<String, Instant> = HashMap::new();
                loop {
                    let plugins: Vec<Plugin> = host
                        .shared
                        .lock()
                        .map(|shared| {
                            shared
                                .plugins
                                .iter()
                                .map(|(plugin, _)| plugin.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    for plugin in plugins {
                        if plugin.manifest.poll.is_none() {
                            continue;
                        }
                        if due.get(&plugin.id).is_some_and(|at| Instant::now() < *at) {
                            continue;
                        }
                        due.insert(plugin.id.clone(), Instant::now() + plugin.poll_every());
                        host.poll_now(&plugin);
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            });
    }

    /// Runs one plugin's poll and keeps the answer (a failure is empty).
    pub fn poll_now(&self, plugin: &Plugin) {
        let output = plugin.poll(&self.language);
        if let Ok(mut shared) = self.shared.lock()
            && let Some(slot) = shared
                .plugins
                .iter_mut()
                .find(|(known, _)| known.id == plugin.id)
            && slot.1 != output
        {
            slot.1 = output;
            shared.changed = true;
            // A plugin's state can change how a file stands.
            shared.details.clear();
        }
    }

    /// The plugins and their last polls, in id order.
    pub fn snapshot(&self) -> Vec<(Plugin, Option<PollOutput>)> {
        self.shared
            .lock()
            .map(|shared| shared.plugins.clone())
            .unwrap_or_default()
    }

    /// Whether anything changed since the last call, for a redraw.
    pub fn take_changed(&self) -> bool {
        self.shared
            .lock()
            .map(|mut shared| std::mem::take(&mut shared.changed))
            .unwrap_or(false)
    }

    pub fn take_outcomes(&self) -> Vec<Outcome> {
        self.shared
            .lock()
            .map(|mut shared| std::mem::take(&mut shared.outcomes))
            .unwrap_or_default()
    }

    /// Runs an action (or an item's `run` command) on a worker thread with
    /// the selected paths, then polls the plugin again so its effect shows.
    pub fn run(
        &self,
        plugin: Plugin,
        command: Vec<String>,
        paths: Vec<PathBuf>,
        target: Option<Item>,
        detach: bool,
    ) {
        let host = self.clone();
        let _ = std::thread::Builder::new()
            .name(format!("files-plugin-{}", plugin.id))
            .spawn(move || {
                let mut env = Vec::new();
                if let Some(item) = &target {
                    env.push(("FOLIO_TARGET", item.id.clone()));
                    env.push(("FOLIO_TARGET_NAME", item.name.clone()));
                }
                let arguments: Vec<String> = paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect();
                if detach {
                    if let Err(error) =
                        plugin.spawn_detached(&command, &arguments, &env, &host.language)
                        && let Ok(mut shared) = host.shared.lock()
                    {
                        shared.outcomes.push(Outcome::Failed(format!("{error:?}")));
                        shared.changed = true;
                    }
                    return;
                }
                let outcome = match plugin.run(&command, &arguments, &env, &host.language, None) {
                    Ok(output) => host.result(&plugin, &output),
                    Err(crate::plugins::RunError::Failed { last_error, .. }) => {
                        Some(Outcome::Failed(last_error))
                    }
                    Err(other) => Some(Outcome::Failed(format!("{other:?}"))),
                };
                if let (Some(outcome), Ok(mut shared)) = (outcome, host.shared.lock()) {
                    shared.outcomes.push(outcome);
                    shared.changed = true;
                }
                host.poll_now(&plugin);
            });
    }

    fn result(&self, plugin: &Plugin, output: &[u8]) -> Option<Outcome> {
        if output.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let result: ActionResult = serde_json::from_slice(output).ok()?;
        if let Some(error) = result.error {
            return Some(Outcome::Failed(error.render(&plugin.catalog)));
        }
        if let Some(copy) = result.copy {
            return Some(Outcome::Copy(copy));
        }
        if let Some(path) = result.path {
            return Some(Outcome::Go(crate::plugins::expand(
                &path,
                self.home.as_deref(),
            )));
        }
        result
            .notice
            .map(|notice| Outcome::Notice(notice.render(&plugin.catalog)))
    }

    /// The details rows plugins give for a file, if known; asks for them in
    /// the background the first time.
    pub fn details_for(&self, path: &Path, mime: Option<&str>) -> Vec<(String, String)> {
        let Ok(mut shared) = self.shared.lock() else {
            return Vec::new();
        };
        if let Some(rows) = shared.details.get(path) {
            return rows.clone();
        }
        if shared.pending_details.iter().any(|pending| pending == path)
            || !shared
                .plugins
                .iter()
                .any(|(plugin, _)| plugin.manifest.details.is_some())
        {
            return Vec::new();
        }
        shared.pending_details.push(path.to_path_buf());
        drop(shared);
        let host = self.clone();
        let path = path.to_path_buf();
        let mime = mime.map(str::to_string);
        std::thread::spawn(move || {
            let mut rows = Vec::new();
            for (plugin, _) in host.snapshot() {
                rows.extend(plugin.details(
                    &path,
                    mime.as_deref(),
                    host.home.as_deref(),
                    &host.language,
                ));
            }
            if let Ok(mut shared) = host.shared.lock() {
                shared.pending_details.retain(|pending| *pending != path);
                shared.details.insert(path, rows);
                shared.changed = true;
            }
        });
        Vec::new()
    }

    /// A plugin's status for a sidebar row's folder, and whether it warns.
    pub fn place_status(&self, folder: &Path) -> Option<(String, bool)> {
        let shared = self.shared.lock().ok()?;
        shared.plugins.iter().find_map(|(plugin, poll)| {
            poll.as_ref()?.places.iter().find_map(|place| {
                (crate::plugins::expand(&place.path, self.home.as_deref()) == folder)
                    .then_some(place.status.as_ref())
                    .flatten()
                    .map(|status| {
                        (
                            status.render(&plugin.catalog),
                            matches!(place.tone.as_deref(), Some("warning" | "error")),
                        )
                    })
            })
        })
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }
}

//! The Applications view's sizes: read once, off the window's thread, and
//! kept (ticket 128).
//!
//! A listing never waits for a size. It fills each row from what is already
//! known and lists the rest as [`ApplicationSize::Pending`]; when it has
//! finished, one thread asks the records for every application still
//! missing (one `dpkg-query` for all the packages, one `flatpak info` per
//! Flatpak) and the window lists the view again when they are in. A size is
//! kept for [`FRESH_FOR`], and all of them are forgotten when the catalog
//! changes, which is what installing or removing an application does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use app_catalog_core::{ApplicationRecord, Catalog};
use files_core::ApplicationSize;
use files_platform::ApplicationQuery;

/// How long a size is trusted. An update changes a package's size without
/// changing its desktop entry, so the catalog alone would never notice.
pub const FRESH_FOR: Duration = Duration::from_secs(600);

/// Reads the sizes of the applications asked about, in order.
pub type Sizer = Arc<dyn Fn(&[ApplicationQuery]) -> Vec<ApplicationSize> + Send + Sync>;

#[derive(Default)]
struct State {
    sizes: HashMap<PathBuf, (ApplicationSize, Instant)>,
    /// The catalog generation the sizes belong to.
    catalog: u64,
    /// A reading is running.
    reading: bool,
}

/// The window's application sizes.
#[derive(Clone)]
pub struct ApplicationSizes {
    state: Arc<Mutex<State>>,
    generation: Arc<AtomicU64>,
    sizer: Option<Sizer>,
}

impl ApplicationSizes {
    /// Sizes that are never read: every row stays pending. A test's reader
    /// and a reader with no host to ask.
    pub fn none() -> Self {
        Self {
            state: Arc::default(),
            generation: Arc::default(),
            sizer: None,
        }
    }

    pub fn with_sizer(sizer: Sizer) -> Self {
        Self {
            sizer: Some(sizer),
            ..Self::none()
        }
    }

    /// The host's records, read unprivileged.
    pub fn from_env() -> Self {
        let roots = files_platform::SizeRoots::host(std::env::var_os("HOME").map(PathBuf::from));
        Self::with_sizer(Arc::new(move |queries| {
            files_platform::application_sizes(queries, &roots, &SystemCommands)
        }))
    }

    /// Moves each time new sizes are in, so the window knows to list the
    /// view again.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The size a listing gives `record` now.
    pub fn size_of(&self, record: &ApplicationRecord, catalog: u64) -> ApplicationSize {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.catalog != catalog {
            return ApplicationSize::Pending;
        }
        match state.sizes.get(&record.source.path) {
            Some((size, read)) if read.elapsed() < FRESH_FOR => size.clone(),
            _ => ApplicationSize::Pending,
        }
    }

    /// After a listing: reads, on a thread of its own, every application of
    /// `catalog` whose size is missing or stale. Does nothing while a
    /// reading runs; the next listing asks again.
    pub fn read_missing(&self, catalog: &Arc<Catalog>, generation: u64) {
        let Some(sizer) = self.sizer.clone() else {
            return;
        };
        let missing: Vec<ApplicationQuery> = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.reading {
                return;
            }
            if state.catalog != generation {
                state.sizes.clear();
                state.catalog = generation;
            }
            let missing: Vec<ApplicationQuery> = catalog
                .records()
                .filter(|record| {
                    !state
                        .sizes
                        .get(&record.source.path)
                        .is_some_and(|(_, read)| read.elapsed() < FRESH_FOR)
                })
                .map(query_for)
                .collect();
            if missing.is_empty() {
                return;
            }
            state.reading = true;
            missing
        };
        let state = Arc::clone(&self.state);
        let counter = Arc::clone(&self.generation);
        let spawned = thread::Builder::new()
            .name("files-application-sizes".to_string())
            .spawn(move || {
                let sizes = sizer(&missing);
                let mut state = state.lock().unwrap_or_else(|poison| poison.into_inner());
                state.reading = false;
                // A catalog that changed while this ran made these sizes
                // describe entries that may be gone; the next listing asks
                // again.
                if state.catalog == generation {
                    let now = Instant::now();
                    for (query, size) in missing.into_iter().zip(sizes) {
                        state.sizes.insert(query.entry_path, (size, now));
                    }
                }
                drop(state);
                counter.fetch_add(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            self.state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .reading = false;
        }
    }
}

/// The host's own read-only commands (`dpkg-query -S`, `flatpak info`), in
/// the C locale so their output parses the same on every desktop. Standard
/// input is closed and errors are dropped; a program that cannot start is
/// `None`, and one that exits non-zero still hands back what it printed.
struct SystemCommands;

impl files_platform::HostCommands for SystemCommands {
    fn run(&self, program: &str, args: &[std::ffi::OsString]) -> Option<String> {
        let output = std::process::Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// What the sizer is told about one catalog record.
pub fn query_for(record: &ApplicationRecord) -> ApplicationQuery {
    ApplicationQuery {
        entry_path: record.source.path.clone(),
        kind: record.source.kind,
        scope: record.source.scope,
        executable: record.executable.path().map(PathBuf::from),
        program: record.exec.as_ref().map(|exec| exec.program().to_string()),
    }
}

/// The Size cell of an application: its bytes, "Calculating…" while it is
/// read, or "—" when it cannot be found.
pub fn size_text(size: &ApplicationSize, c: &'static crate::i18n::Copy) -> String {
    match size {
        ApplicationSize::Known { bytes, .. } => crate::format::bytes(*bytes),
        ApplicationSize::Pending => c.app_size_calculating.to_string(),
        ApplicationSize::Unknown(_) => "—".to_string(),
    }
}

/// What the size counts, or why there is none: the cell's tooltip and the
/// line under the size in the panels. `None` while it is being read.
pub fn size_note(size: &ApplicationSize, c: &'static crate::i18n::Copy) -> Option<String> {
    use files_core::{ApplicationSizeSource as Source, ApplicationSizeUnknown as Unknown};
    Some(match size {
        ApplicationSize::Pending => return None,
        ApplicationSize::Known { source, .. } => match source {
            Source::DebianPackage { package } => {
                c.app_size_package_only.replace("{package}", package)
            }
            Source::Flatpak {
                runtime: Some(runtime),
                ..
            } => c.app_size_flatpak.replace("{runtime}", runtime),
            Source::Flatpak { runtime: None, .. } => c.app_size_flatpak_any_runtime.to_string(),
            Source::Snap { name, revision } => c
                .app_size_snap
                .replace("{name}", name)
                .replace("{revision}", revision),
            Source::File { .. } => c.app_size_file.to_string(),
        },
        ApplicationSize::Unknown(reason) => match reason {
            Unknown::NoOwner => c.app_size_no_owner.to_string(),
            Unknown::SeveralPackages => c.app_size_several_packages.to_string(),
            Unknown::NotRecorded { package } => {
                c.app_size_not_recorded.replace("{package}", package)
            }
            Unknown::FlatpakUnanswered { app_id } => {
                c.app_size_flatpak_unanswered.replace("{app}", app_id)
            }
            Unknown::SnapMissing { name } => c.app_size_snap_missing.replace("{name}", name),
            Unknown::ProgramMissing { path } => c
                .app_size_program_missing
                .replace("{path}", &path.to_string_lossy()),
            Unknown::DpkgUnanswered => c.app_size_dpkg_unanswered.to_string(),
        },
    })
}

/// The separate Data figure, when the application's own data folder is
/// known: "Data: 12.3 MB".
pub fn data_text(size: &ApplicationSize, c: &'static crate::i18n::Copy) -> Option<String> {
    match size {
        ApplicationSize::Known {
            data: Some(bytes), ..
        } => Some(format!(
            "{}: {}",
            c.app_size_data,
            crate::format::bytes(*bytes)
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{EN_US, ZH_TW};
    use files_core::{ApplicationSizeSource, ApplicationSizeUnknown};

    fn known(source: ApplicationSizeSource, data: Option<u64>) -> ApplicationSize {
        ApplicationSize::Known {
            bytes: 1_605 * 1024,
            source,
            data,
        }
    }

    #[test]
    fn a_known_size_says_what_it_counts() {
        let package = known(
            ApplicationSizeSource::DebianPackage {
                package: "nautilus".to_string(),
            },
            None,
        );
        assert_eq!(
            size_text(&package, &EN_US),
            crate::format::bytes(1_605 * 1024)
        );
        assert_eq!(
            size_note(&package, &EN_US).unwrap(),
            "The nautilus package alone, without the libraries it uses"
        );
        assert_eq!(data_text(&package, &EN_US), None);
        let flatpak = known(
            ApplicationSizeSource::Flatpak {
                app_id: "org.example.App".to_string(),
                runtime: Some("org.gnome.Platform".to_string()),
            },
            Some(2048),
        );
        assert!(
            size_note(&flatpak, &ZH_TW)
                .unwrap()
                .contains("不含共用的執行環境 org.gnome.Platform")
        );
        assert_eq!(
            data_text(&flatpak, &EN_US).unwrap(),
            format!("Data: {}", crate::format::bytes(2048))
        );
        let snap = known(
            ApplicationSizeSource::Snap {
                name: "firefox".to_string(),
                revision: "4793".to_string(),
            },
            None,
        );
        assert_eq!(
            size_note(&snap, &EN_US).unwrap(),
            "Snap firefox, revision 4793"
        );
    }

    #[test]
    fn a_missing_size_is_a_dash_with_its_reason_and_a_pending_one_says_so() {
        let missing = ApplicationSize::Unknown(ApplicationSizeUnknown::NoOwner);
        assert_eq!(size_text(&missing, &EN_US), "—");
        assert_eq!(
            size_note(&missing, &ZH_TW).unwrap(),
            "沒有已安裝的套件擁有這個應用程式"
        );
        assert_eq!(size_text(&ApplicationSize::Pending, &EN_US), "Calculating…");
        assert_eq!(size_note(&ApplicationSize::Pending, &EN_US), None);
    }

    #[test]
    fn a_row_and_its_tile_carry_an_applications_size_once_known() {
        use files_core::{
            ApplicationFacts, Entry, EntryBody, EntryKind, EntrySize, HiddenState,
            PermissionsSummary,
        };
        let entry = |size: ApplicationSize| {
            let mut entry = Entry {
                name: "Files".to_string(),
                kind: EntryKind::Application,
                size: EntrySize::Unknown,
                modified: None,
                permissions: PermissionsSummary::UNKNOWN,
                hidden: HiddenState::Visible,
                mime: None,
                body: EntryBody::Application(ApplicationFacts {
                    desktop_id: app_catalog_core::DesktopId::new("org.example.Files.desktop")
                        .unwrap(),
                    icon: None,
                    categories: Vec::new(),
                    comment: None,
                    size: ApplicationSize::Pending,
                }),
            };
            files_core::set_application_size(&mut entry, size);
            entry
        };
        let pending =
            crate::content::rendered_row(&entry(ApplicationSize::Pending), false, false, &EN_US);
        assert_eq!(pending.size, "Calculating…");
        assert_eq!(crate::content::tile_caption(&pending), "");
        let missing = crate::content::rendered_row(
            &entry(ApplicationSize::Unknown(
                ApplicationSizeUnknown::SeveralPackages,
            )),
            false,
            false,
            &EN_US,
        );
        assert_eq!(missing.size, "—");
        assert_eq!(
            missing.size_tooltip.as_deref(),
            Some("More than one package claims this application")
        );
        assert_eq!(crate::content::tile_caption(&missing), "");
        let sized = entry(known(
            ApplicationSizeSource::File {
                path: "/home/a/Tool.AppImage".into(),
            },
            None,
        ));
        assert_eq!(sized.size, EntrySize::Bytes(1_605 * 1024));
        let row = crate::content::rendered_row(&sized, false, false, &EN_US);
        assert_eq!(
            crate::content::tile_caption(&row),
            crate::format::bytes(1_605 * 1024)
        );
    }
}

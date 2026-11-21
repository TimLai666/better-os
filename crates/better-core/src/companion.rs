//! How a Better OS program finds a companion program: a tray, a window, a
//! helper it starts (ticket 179).
//!
//! The rule is one, and every place that starts a companion goes through
//! [`Finder`]:
//!
//! * **Beside its own executable first.** A program run from a build
//!   directory (`target/debug/monitor-gui`) starts the companion built beside
//!   it (`target/debug/better-monitor-tray`), under the name cargo gives it.
//!   An installed program finds an installed companion in its own directory
//!   under the name the package installs it as.
//! * **The packaged location otherwise**, and only for a program that is
//!   itself installed. A program run from a build directory with no sibling
//!   of that name refuses: it never starts the installed one, which is
//!   another version and would talk to the window under test with another
//!   version's words.
//! * **Installed means one of the packaged directories**
//!   ([`PACKAGED_DIRECTORIES`]: `/usr/bin`, `/usr/libexec`,
//!   `/usr/lib/better-os`), the only places `packaging/build-deb.sh` puts a
//!   program. A program run from anywhere else, `/usr/local/bin` and a
//!   `cargo install` directory included, is treated as a build and starts
//!   only a sibling of the name cargo gives it.
//! * **Never a bare name resolved through `PATH`.** On the owner's machine
//!   `PATH` leads to `/usr/bin`, which is how a debug window in a sandbox
//!   started the installed `better-monitor-tray` against the sandbox's bus.
//! * **Under the test guard** ([`crate::test_guard`]) nothing installed is
//!   started at all: an installed program starts no companion, and asking
//!   for one by bus or unit name is refused too. A sibling in a build
//!   directory is not an installed program and still starts.
//!
//! A companion reached by a D-Bus name or a systemd unit name instead of a
//! path ([`StartPlan::ByName`]) is the installed one by construction: the
//! bus and the user manager read the system's activation and unit files. So
//! only an installed, unguarded program asks by name; a build asks for its
//! sibling by path.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::test_guard::TestGuard;

/// A companion program: the name cargo builds it under and the path its
/// package installs it at. The two differ for several programs
/// (`monitor-gui` is installed as `/usr/bin/better-monitor`), which is why a
/// build directory is searched under one and `/usr` under the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Companion {
    /// The binary's name in a build directory (`[[bin]] name`).
    pub built: &'static str,
    /// Where `packaging/build-deb.sh` installs it.
    pub installed: &'static str,
}

impl Companion {
    /// The installed file's own name, which is what an installed program
    /// looks for beside itself.
    pub fn installed_name(&self) -> &'static str {
        self.installed.rsplit('/').next().unwrap_or(self.installed)
    }
}

/// Better Monitor's taskbar item, started by the window.
pub const MONITOR_TRAY: Companion = Companion {
    built: "better-monitor-tray",
    installed: "/usr/bin/better-monitor-tray",
};

/// Better Monitor's window, opened by the taskbar item.
pub const MONITOR_WINDOW: Companion = Companion {
    built: "monitor-gui",
    installed: "/usr/bin/better-monitor",
};

/// Better Awake's taskbar item, started by the window.
pub const AWAKE_TRAY: Companion = Companion {
    built: "better-awake-tray",
    installed: "/usr/bin/awake-tray",
};

/// Better Awake's old window, which the taskbar item opens when Better
/// Manager cannot show the Keep awake page.
pub const AWAKE_WINDOW: Companion = Companion {
    built: "awake-gui",
    installed: "/usr/bin/awake-gui",
};

/// Better Manager's window, opened by the Awake taskbar item on its page.
pub const MANAGER_WINDOW: Companion = Companion {
    built: "manager-gui",
    installed: "/usr/bin/better-manager",
};

/// Better Files' preview program, started by the window for each preview.
pub const FILES_PREVIEW: Companion = Companion {
    built: "better-files-preview",
    installed: "/usr/bin/better-files-preview",
};

/// Every companion a Better OS program starts by path. A test holds the
/// table to `packaging/build-deb.sh`.
pub const COMPANIONS: [Companion; 6] = [
    MONITOR_TRAY,
    MONITOR_WINDOW,
    AWAKE_TRAY,
    AWAKE_WINDOW,
    MANAGER_WINDOW,
    FILES_PREVIEW,
];

/// The prefix the packaged directories and every companion's installed path
/// share. A test stands a scratch directory in for it.
pub const INSTALLED_ROOT: &str = "/usr";

/// Where a package installs programs, under [`INSTALLED_ROOT`]. A program
/// whose executable is in one of them (or below `lib/better-os`) is an
/// installed one; anywhere else it is a build.
pub const PACKAGED_DIRECTORIES: [&str; 3] = ["bin", "libexec", "lib/better-os"];

/// Why no companion was found, in words a log or a toast can show.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The test guard is on and the companion would be an installed program.
    Guarded(String),
    /// A program in a build directory with no sibling of that name. The
    /// installed one is never taken in its place.
    NotBeside { program: String, directory: PathBuf },
    /// An installed program whose companion is not installed.
    NotInstalled(PathBuf),
    /// This process's own executable could not be read, so neither "beside"
    /// nor "installed" can be told.
    NoOwnExecutable,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Guarded(reason) => f.write_str(reason),
            Refusal::NotBeside { program, directory } => write!(
                f,
                "{program} is not beside this build in {}; the installed one is not started for a build",
                directory.display()
            ),
            Refusal::NotInstalled(path) => write!(f, "{} is not installed", path.display()),
            Refusal::NoOwnExecutable => {
                f.write_str("this program's own executable could not be read")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// How to start a companion that a package also starts by name (a D-Bus
/// activation file, a systemd user unit).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartPlan {
    /// An installed, unguarded program: ask the bus or the user manager by
    /// name, which starts the installed companion under its unit.
    ByName,
    /// A build: start this sibling directly.
    Path(PathBuf),
    /// Start nothing, for this reason.
    Refused(Refusal),
}

/// The rule, with what it depends on held so a test can set it.
#[derive(Clone, Debug)]
pub struct Finder {
    own_executable: Option<PathBuf>,
    installed_root: PathBuf,
    guard: Option<TestGuard>,
}

impl Finder {
    /// This process: its own executable, `/usr`, and the guard as the
    /// environment sets it.
    pub fn from_environment() -> Self {
        Self {
            own_executable: std::env::current_exe()
                .ok()
                .map(|path| crate::self_update::strip_deleted(&path)),
            installed_root: PathBuf::from(INSTALLED_ROOT),
            guard: TestGuard::from_environment(),
        }
    }

    /// A finder for a program at `own_executable`, with `installed_root`
    /// standing in for `/usr`, for tests.
    pub fn new(
        own_executable: Option<PathBuf>,
        installed_root: impl Into<PathBuf>,
        guard: Option<TestGuard>,
    ) -> Self {
        Self {
            own_executable,
            installed_root: installed_root.into(),
            guard,
        }
    }

    /// Whether this program is an installed one, by where its executable is.
    /// `None` when that cannot be read.
    pub fn running_installed(&self) -> Option<bool> {
        self.own_executable
            .as_deref()
            .map(|own| self.is_packaged(own))
    }

    /// Whether `own` is in one of the packaged directories.
    fn is_packaged(&self, own: &Path) -> bool {
        PACKAGED_DIRECTORIES
            .iter()
            .any(|directory| own.starts_with(self.installed_root.join(directory)))
    }

    /// The companion's packaged path, under [`Self`]'s installed root.
    fn packaged(&self, companion: &Companion) -> PathBuf {
        let relative = Path::new(companion.installed)
            .strip_prefix(INSTALLED_ROOT)
            .unwrap_or(Path::new(companion.installed_name()));
        self.installed_root.join(relative)
    }

    fn guarded(&self, companion: &Companion) -> Option<Refusal> {
        self.guard
            .as_ref()
            .map(|guard| Refusal::Guarded(guard.reason(companion.installed)))
    }

    /// The companion's path by the rule, or why there is none.
    pub fn locate(&self, companion: &Companion) -> Result<PathBuf, Refusal> {
        let own = self
            .own_executable
            .as_deref()
            .ok_or(Refusal::NoOwnExecutable)?;
        let directory = own.parent().ok_or(Refusal::NoOwnExecutable)?;
        if self.is_packaged(own) {
            if let Some(refusal) = self.guarded(companion) {
                return Err(refusal);
            }
            let beside = directory.join(companion.installed_name());
            if beside.is_file() {
                return Ok(beside);
            }
            let packaged = self.packaged(companion);
            if packaged.is_file() {
                return Ok(packaged);
            }
            return Err(Refusal::NotInstalled(packaged));
        }
        let beside = directory.join(companion.built);
        if beside.is_file() {
            Ok(beside)
        } else {
            Err(Refusal::NotBeside {
                program: companion.built.to_string(),
                directory: directory.to_path_buf(),
            })
        }
    }

    /// How to start a companion the package also starts by bus or unit name.
    pub fn plan(&self, companion: &Companion) -> StartPlan {
        match self.running_installed() {
            None => StartPlan::Refused(Refusal::NoOwnExecutable),
            Some(true) => match self.guarded(companion) {
                Some(refusal) => StartPlan::Refused(refusal),
                None => StartPlan::ByName,
            },
            Some(false) => match self.locate(companion) {
                Ok(path) => StartPlan::Path(path),
                Err(refusal) => StartPlan::Refused(refusal),
            },
        }
    }
}

/// [`Finder::locate`] for this process.
pub fn locate(companion: &Companion) -> Result<PathBuf, Refusal> {
    Finder::from_environment().locate(companion)
}

/// [`Finder::plan`] for this process.
pub fn plan(companion: &Companion) -> StartPlan {
    Finder::from_environment().plan(companion)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Layout {
        _root: tempfile::TempDir,
        usr: PathBuf,
        build: PathBuf,
    }

    /// A stand-in `/usr` with the tray installed, and a build directory.
    fn layout() -> Layout {
        let root = tempfile::tempdir().unwrap();
        let usr = root.path().join("usr");
        let build = root.path().join("target/debug");
        std::fs::create_dir_all(usr.join("bin")).unwrap();
        std::fs::create_dir_all(usr.join("libexec")).unwrap();
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(usr.join("bin/better-monitor"), "").unwrap();
        std::fs::write(usr.join("bin/better-monitor-tray"), "").unwrap();
        std::fs::write(build.join("monitor-gui"), "").unwrap();
        Layout {
            _root: root,
            usr,
            build,
        }
    }

    #[test]
    fn a_build_starts_its_sibling_from_its_own_directory() {
        let layout = layout();
        std::fs::write(layout.build.join("better-monitor-tray"), "").unwrap();
        let finder = Finder::new(Some(layout.build.join("monitor-gui")), &layout.usr, None);

        assert_eq!(
            finder.locate(&MONITOR_TRAY),
            Ok(layout.build.join("better-monitor-tray"))
        );
        assert_eq!(
            finder.plan(&MONITOR_TRAY),
            StartPlan::Path(layout.build.join("better-monitor-tray"))
        );
    }

    #[test]
    fn a_build_with_no_sibling_refuses_and_never_takes_the_installed_one() {
        let layout = layout();
        let finder = Finder::new(Some(layout.build.join("monitor-gui")), &layout.usr, None);

        let refusal = finder.locate(&MONITOR_TRAY).unwrap_err();
        assert_eq!(
            refusal,
            Refusal::NotBeside {
                program: "better-monitor-tray".to_string(),
                directory: layout.build.clone(),
            }
        );
        assert!(refusal.to_string().contains("not beside this build"));
        // Never by name either: the bus would start the installed tray.
        assert_eq!(finder.plan(&MONITOR_TRAY), StartPlan::Refused(refusal));
    }

    #[test]
    fn a_build_looks_for_the_name_cargo_gives_not_the_installed_one() {
        let layout = layout();
        // `better-monitor` in a build directory is the command line, not
        // the window the tray opens.
        std::fs::write(layout.build.join("better-monitor"), "").unwrap();
        let tray = layout.build.join("better-monitor-tray");
        std::fs::write(&tray, "").unwrap();
        let finder = Finder::new(Some(tray), &layout.usr, None);

        assert_eq!(
            finder.locate(&MONITOR_WINDOW),
            Ok(layout.build.join("monitor-gui"))
        );
    }

    #[test]
    fn an_installed_program_finds_its_companion_beside_it_then_where_it_is_packaged() {
        let layout = layout();
        let window = Finder::new(
            Some(layout.usr.join("bin/better-monitor")),
            &layout.usr,
            None,
        );
        assert_eq!(
            window.locate(&MONITOR_TRAY),
            Ok(layout.usr.join("bin/better-monitor-tray"))
        );
        assert_eq!(window.plan(&MONITOR_TRAY), StartPlan::ByName);

        // A program in another packaged directory reaches /usr/bin.
        let helper = Finder::new(Some(layout.usr.join("libexec/helper")), &layout.usr, None);
        assert_eq!(
            helper.locate(&MONITOR_TRAY),
            Ok(layout.usr.join("bin/better-monitor-tray"))
        );

        // Not installed is said, not guessed at.
        assert_eq!(
            window.locate(&AWAKE_TRAY),
            Err(Refusal::NotInstalled(layout.usr.join("bin/awake-tray")))
        );
    }

    #[test]
    fn with_the_guard_set_an_installed_program_starts_nothing() {
        let layout = layout();
        let finder = Finder::new(
            Some(layout.usr.join("bin/better-monitor")),
            &layout.usr,
            Some(TestGuard::silent()),
        );

        let refusal = finder.locate(&MONITOR_TRAY).unwrap_err();
        assert!(matches!(refusal, Refusal::Guarded(_)), "{refusal:?}");
        assert!(
            refusal
                .to_string()
                .contains(crate::test_guard::GUARD_VARIABLE)
        );
        // Not by name either.
        assert!(matches!(
            finder.plan(&MONITOR_TRAY),
            StartPlan::Refused(Refusal::Guarded(_))
        ));
    }

    #[test]
    fn with_the_guard_set_a_build_starts_only_its_own_sibling() {
        let layout = layout();
        let finder = Finder::new(
            Some(layout.build.join("monitor-gui")),
            &layout.usr,
            Some(TestGuard::silent()),
        );
        // No sibling: refused, though /usr has one.
        assert!(matches!(
            finder.plan(&MONITOR_TRAY),
            StartPlan::Refused(Refusal::NotBeside { .. })
        ));
        // A sibling is not an installed program, so it is the one started.
        std::fs::write(layout.build.join("better-monitor-tray"), "").unwrap();
        assert_eq!(
            finder.plan(&MONITOR_TRAY),
            StartPlan::Path(layout.build.join("better-monitor-tray"))
        );
    }

    /// Review of ticket 179: `/usr/local/bin` is not a packaged directory.
    /// A program there is a build, so it never takes `/usr/bin`'s companion
    /// and looks beside itself under cargo's name.
    #[test]
    fn a_program_outside_the_packaged_directories_is_a_build() {
        let layout = layout();
        let local = layout.usr.join("local/bin");
        std::fs::create_dir_all(&local).unwrap();
        let finder = Finder::new(Some(local.join("better-monitor-tray")), &layout.usr, None);

        assert_eq!(finder.running_installed(), Some(false));
        assert_eq!(
            finder.locate(&MONITOR_WINDOW),
            Err(Refusal::NotBeside {
                program: "monitor-gui".to_string(),
                directory: local.clone(),
            })
        );
        // `better-monitor` beside it is cargo's command line, never taken
        // for the window.
        std::fs::write(local.join("better-monitor"), "").unwrap();
        assert!(finder.locate(&MONITOR_WINDOW).is_err());
        // The packaged directories are installed, including below
        // `lib/better-os`.
        for directory in ["bin", "libexec", "lib/better-os/before-removal"] {
            let own = layout.usr.join(directory).join("program");
            let finder = Finder::new(Some(own), &layout.usr, None);
            assert_eq!(finder.running_installed(), Some(true), "{directory}");
        }
    }

    #[test]
    fn no_own_executable_starts_nothing() {
        let finder = Finder::new(None, "/usr", None);
        assert_eq!(finder.locate(&MONITOR_TRAY), Err(Refusal::NoOwnExecutable));
        assert_eq!(
            finder.plan(&MONITOR_TRAY),
            StartPlan::Refused(Refusal::NoOwnExecutable)
        );
    }

    /// The table names each program as cargo builds it and as the package
    /// installs it, so the two cannot drift apart.
    #[test]
    fn every_companion_is_built_and_installed_under_the_names_the_table_gives() {
        let build = include_str!("../../../packaging/build-deb.sh");
        for companion in COMPANIONS {
            let pair = format!(
                "\"{}:{}\"",
                companion.built,
                companion.installed.trim_start_matches('/')
            );
            assert!(
                build.contains(&pair),
                "build-deb.sh does not install {pair}"
            );
        }
    }
}

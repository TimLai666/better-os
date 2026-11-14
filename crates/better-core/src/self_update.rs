//! A running Better OS program noticing that its own file was replaced by an
//! update, and handing its view over to the new program (ticket 105).
//!
//! After `apt upgrade` or Better Manager replaces a package, a window that was
//! already open keeps running the old program: `/proc/<pid>/exe` ends in
//! `(deleted)`, and the window talks to services that may now speak a newer
//! protocol. Every Better OS process therefore watches its own program file,
//! and once the new one is in place and dpkg has finished configuring the
//! package, saves what it is showing and starts the new program in its place.
//!
//! This module has no GPUI in it. It holds every decision, so they are tested
//! without a display:
//!
//! - [`ProgramFile`] knows which file the process was started from and which
//!   file is running, and says when the two have parted ([`OnDisk`]).
//! - [`UpdateTracker`] turns that into [`UpdateState`]: nothing happened, an
//!   update is still being unpacked or configured, or the new program is ready.
//!   It waits for dpkg to report the package installed, because a file that is
//!   in place before the package's maintainer script ran can be started against
//!   a service that has not been restarted yet.
//! - [`decide`] is the one rule about waiting: a process in the middle of work
//!   it cannot carry over keeps running the old program until that work ends.
//! - [`Handoff`] and [`HandoffDir`] carry the view to the new program through a
//!   private file in the runtime directory, keyed by the process id, which
//!   `execve(2)` keeps.
//! - [`ProgramWatch`] wakes a process when its program's directory changes, so
//!   an idle program polls nothing.
//!
//! `better_ui::restart` is the window side. A process with no window — a
//! service or a tray — runs as a systemd user unit, and the package's postinst
//! restarts it; it does not watch itself.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// dpkg's administrative directory: its status file and its updates journal.
pub const DPKG_ADMIN_DIR: &str = "/var/lib/dpkg";

/// How long a reinstall of the same version has to settle after dpkg reports
/// it configured.
pub const REINSTALL_SETTLE: Duration = Duration::from_secs(3);

/// How long a replaced program file has to stay the same before a program that
/// no package owns (a development build) starts it.
pub const UNPACKAGED_SETTLE: Duration = Duration::from_secs(2);

/// How long a replaced program file has to stay the same after dpkg reports the
/// package installed. dpkg writes its status before it exits, and this is the
/// margin for the rest of the transaction's packages.
pub const PACKAGED_SETTLE: Duration = Duration::from_secs(1);

/// How long a process waits for dpkg to say the package is configured before it
/// starts the new program anyway. A dpkg run that failed half way leaves the
/// package unconfigured for good, and the new program is on disk all the same.
pub const CONFIGURE_PATIENCE: Duration = Duration::from_secs(5 * 60);

/// How often a process looks again while an update is in progress. Between
/// updates nothing polls; [`ProgramWatch`] wakes the process.
pub const PROGRESS_POLL: Duration = Duration::from_millis(500);

/// How old a hand-off may be and still be read. The new program reads it a
/// moment after the old one wrote it; anything older belongs to a process that
/// died between the two, and its id may since have been reused.
pub const HANDOFF_MAX_AGE: Duration = Duration::from_secs(60);

/// The largest hand-off written or read. A view is a few kilobytes.
pub const MAX_HANDOFF_BYTES: u64 = 1024 * 1024;

/// The user this process acts as.
fn effective_uid() -> io::Result<u32> {
    // SAFETY: geteuid has no preconditions and cannot fail.
    Ok(unsafe { libc::geteuid() })
}

/// The hand-off format this build writes and reads.
pub const HANDOFF_FORMAT: u32 = 1;

/// A file's identity: the device and inode `stat(2)` reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

impl FileIdentity {
    fn of(metadata: &fs::Metadata) -> FileIdentity {
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

/// What is at the program's path now, compared with what is running.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnDisk {
    /// The running program is still the file at its path.
    Same,
    /// Nothing executable is at the path: an update is part way through
    /// replacing it, or the package was removed.
    Missing,
    /// A different file is at the path.
    Replaced(FileIdentity),
}

/// The file a process was started from, and the file that is running.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFile {
    path: PathBuf,
    running: FileIdentity,
}

impl ProgramFile {
    /// This process's program, read once at startup. `/proc/self/exe` names
    /// the path; `stat` through it reaches the running file even after the path
    /// was given to another one, which is what makes the comparison possible.
    pub fn current() -> io::Result<ProgramFile> {
        let path = std::env::current_exe()?;
        let path = strip_deleted(&path);
        let running = FileIdentity::of(&fs::metadata("/proc/self/exe")?);
        Ok(ProgramFile { path, running })
    }

    /// A program at `path` whose running file is the one there now. For tests
    /// and for a caller that already knows its path.
    pub fn at(path: impl Into<PathBuf>) -> io::Result<ProgramFile> {
        let path = path.into();
        let running = FileIdentity::of(&fs::metadata(&path)?);
        Ok(ProgramFile { path, running })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn running(&self) -> FileIdentity {
        self.running
    }

    /// Compares the path with the running file.
    pub fn on_disk(&self) -> OnDisk {
        match fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
                let identity = FileIdentity::of(&metadata);
                if identity == self.running {
                    OnDisk::Same
                } else {
                    OnDisk::Replaced(identity)
                }
            }
            _ => OnDisk::Missing,
        }
    }
}

/// `/proc/self/exe` reads `/usr/bin/x (deleted)` once the file was replaced.
/// The path a process restarts from is the one without that suffix.
fn strip_deleted(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_suffix(" (deleted)") {
        Some(stripped) => PathBuf::from(stripped),
        None => path.to_path_buf(),
    }
}

/// What dpkg's record says about one package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageRecord {
    /// The `Status` field's last word is `installed`: unpacked and configured.
    pub configured: bool,
    pub version: Option<String>,
}

/// Reads `dpkg-query -W -f='${Status}\n${Version}\n'` output. `None` when it
/// names no package, which is what dpkg-query prints for one it does not know.
pub fn parse_query_output(output: &str) -> Option<PackageRecord> {
    let mut lines = output.lines();
    let status = lines.next()?.trim();
    if status.is_empty() {
        return None;
    }
    let version = lines
        .next()
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string);
    let configured = status.split_whitespace().last() == Some("installed");
    // "unknown ok not-installed" is dpkg remembering a name, not a package.
    if status.ends_with("not-installed") && version.is_none() {
        return None;
    }
    Some(PackageRecord {
        configured,
        version,
    })
}

/// Where a tracker learns what dpkg has done.
pub trait PackageSource: Send {
    /// Something that changes whenever dpkg's record may have: the record is
    /// only asked for again when this moves.
    fn marker(&self) -> Vec<Option<SystemTime>>;
    /// The package as dpkg sees it now, or `None` when dpkg has no such
    /// package.
    fn record(&self, package: &str) -> Option<PackageRecord>;
}

/// The real source. `dpkg-query` is asked rather than the status file read,
/// because while dpkg runs its changes sit in `/var/lib/dpkg/updates/` until a
/// checkpoint folds them in: the status file can say the old version is
/// installed while the new one is half-configured. `dpkg-query` applies that
/// journal; and it is only run when the status file or the journal directory
/// changed.
pub struct DpkgSource {
    admin_dir: PathBuf,
}

impl DpkgSource {
    pub fn system() -> DpkgSource {
        DpkgSource {
            admin_dir: PathBuf::from(DPKG_ADMIN_DIR),
        }
    }
}

impl PackageSource for DpkgSource {
    fn marker(&self) -> Vec<Option<SystemTime>> {
        let mtime = |path: PathBuf| fs::metadata(path).and_then(|m| m.modified()).ok();
        vec![
            mtime(self.admin_dir.join("status")),
            mtime(self.admin_dir.join("updates")),
        ]
    }

    fn record(&self, package: &str) -> Option<PackageRecord> {
        let output = std::process::Command::new("dpkg-query")
            .arg(format!("--admindir={}", self.admin_dir.display()))
            .args(["-W", "-f=${Status}\n${Version}\n", "--", package])
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        parse_query_output(&String::from_utf8_lossy(&output.stdout))
    }
}

/// Where an update is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateState {
    /// The running program is the installed one.
    Current,
    /// The program file is gone or new, and the update has not finished.
    InProgress,
    /// The new program is in place and configured. `version` is what dpkg
    /// says was installed, when the program comes from a package.
    Ready { version: Option<String> },
}

/// Follows one program file through an update.
pub struct UpdateTracker {
    program: ProgramFile,
    package: Option<String>,
    source: Box<dyn PackageSource>,
    /// The version dpkg had when the program started.
    start_version: Option<String>,
    marker: Option<Vec<Option<SystemTime>>>,
    record: Option<PackageRecord>,
    candidate: Option<(FileIdentity, Instant)>,
}

impl UpdateTracker {
    /// `package` is the dpkg package that ships the program, which decides when
    /// the update has finished.
    pub fn new(program: ProgramFile, package: Option<&str>) -> UpdateTracker {
        UpdateTracker::with_source(program, package, Box::new(DpkgSource::system()))
    }

    /// The same, learning about dpkg from somewhere else.
    pub fn with_source(
        program: ProgramFile,
        package: Option<&str>,
        source: Box<dyn PackageSource>,
    ) -> UpdateTracker {
        let start_version = package
            .and_then(|package| source.record(package))
            .and_then(|record| record.version);
        UpdateTracker {
            program,
            package: package.map(str::to_string),
            source,
            start_version,
            marker: None,
            record: None,
            candidate: None,
        }
    }

    pub fn program(&self) -> &ProgramFile {
        &self.program
    }

    fn package_record(&mut self) -> Option<PackageRecord> {
        let package = self.package.as_deref()?;
        let marker = self.source.marker();
        if self.marker.as_ref() != Some(&marker) {
            self.record = self.source.record(package);
            self.marker = Some(marker);
        }
        self.record.clone()
    }

    /// Looks at the program file, and at dpkg's record when it has moved.
    pub fn poll(&mut self, now: Instant) -> UpdateState {
        let identity = match self.program.on_disk() {
            OnDisk::Same => {
                self.candidate = None;
                return UpdateState::Current;
            }
            OnDisk::Missing => {
                self.candidate = None;
                return UpdateState::InProgress;
            }
            OnDisk::Replaced(identity) => identity,
        };
        let since = match self.candidate {
            Some((seen, since)) if seen == identity => since,
            _ => {
                self.candidate = Some((identity, now));
                now
            }
        };
        let stable = now.saturating_duration_since(since);
        match self.package_record() {
            // A new version, configured: the package's postinst has run.
            Some(record)
                if record.configured
                    && record.version != self.start_version
                    && stable >= PACKAGED_SETTLE =>
            {
                UpdateState::Ready {
                    version: record.version,
                }
            }
            // The same version put back (a reinstall). dpkg-query has applied
            // its journal, so "configured" is true, but there is no version
            // change to confirm it with, so it settles longer.
            Some(record) if record.configured && stable >= REINSTALL_SETTLE => UpdateState::Ready {
                version: record.version,
            },
            Some(record) if stable >= CONFIGURE_PATIENCE => UpdateState::Ready {
                version: record.version,
            },
            Some(_) => UpdateState::InProgress,
            None if stable >= UNPACKAGED_SETTLE => UpdateState::Ready { version: None },
            None => UpdateState::InProgress,
        }
    }
}

/// What a process does about an update right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartDecision {
    /// Nothing to do yet.
    Stay,
    /// The new program is ready and the process is in the middle of something
    /// it cannot carry over; it says so and restarts when that ends.
    Wait,
    /// Save the view and start the new program now.
    Restart,
}

/// The one rule about waiting. `busy` is true while the process is doing
/// something a restart would lose: a file operation, a Git command, a package
/// transaction, or a dialog whose typed input has no saved form.
pub fn decide(state: &UpdateState, busy: bool) -> RestartDecision {
    match (state, busy) {
        (UpdateState::Ready { .. }, true) => RestartDecision::Wait,
        (UpdateState::Ready { .. }, false) => RestartDecision::Restart,
        _ => RestartDecision::Stay,
    }
}

/// How a window was placed, so the new one opens the same way. On Wayland the
/// compositor chooses where a window goes and only the size comes back.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub mode: WindowMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// What the old program hands the new one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    pub format: u32,
    /// The window's application id, which names the file.
    pub app_id: String,
    pub pid: u32,
    /// Milliseconds since the Unix epoch.
    pub written_unix_ms: u64,
    /// The version dpkg installed, for the "Updated to" notice.
    pub updated_to: Option<String>,
    pub window: Option<WindowPlacement>,
    /// The application's own view state. It is read with
    /// [`Handoff::state`], and a shape the new program no longer understands
    /// costs the view and nothing else.
    pub state: serde_json::Value,
}

impl Handoff {
    pub fn new(
        app_id: &str,
        updated_to: Option<String>,
        window: Option<WindowPlacement>,
        state: &impl Serialize,
    ) -> Handoff {
        Handoff {
            format: HANDOFF_FORMAT,
            app_id: app_id.to_string(),
            pid: std::process::id(),
            written_unix_ms: unix_ms(SystemTime::now()),
            updated_to,
            window,
            state: serde_json::to_value(state).unwrap_or(serde_json::Value::Null),
        }
    }

    /// The application's state, or `None` when it is not one this program
    /// reads.
    pub fn state<S: DeserializeOwned>(&self) -> Option<S> {
        serde_json::from_value(self.state.clone()).ok()
    }
}

fn unix_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// An application id is part of a file name, so only a plain one is accepted.
fn valid_app_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id.len() <= 128
        && !app_id.starts_with('.')
        && app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// The directory hand-offs are written to: private to the person, and on the
/// runtime filesystem, so a hand-off never outlives the login session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandoffDir {
    root: PathBuf,
}

impl HandoffDir {
    /// `$XDG_RUNTIME_DIR/better-os/restart`. A session with no runtime
    /// directory has nowhere private to put a hand-off, and a restart there
    /// starts the new program with its usual first view.
    pub fn from_environment() -> Option<HandoffDir> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        let runtime = PathBuf::from(runtime);
        if !runtime.is_absolute() {
            return None;
        }
        Some(HandoffDir::at(runtime.join("better-os").join("restart")))
    }

    pub fn at(root: impl Into<PathBuf>) -> HandoffDir {
        HandoffDir { root: root.into() }
    }

    fn path_for(&self, app_id: &str, pid: u32) -> Option<PathBuf> {
        valid_app_id(app_id).then(|| self.root.join(format!("{app_id}-{pid}.json")))
    }

    /// Makes sure the directory is this user's own and private: a real
    /// directory, not a link, owned by the effective user, mode 0700.
    fn private_root(&self) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt as _;
        if let Some(parent) = self.root.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::DirBuilder::new().mode(0o700).create(&self.root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = fs::symlink_metadata(&self.root)?;
        if !metadata.is_dir() || metadata.uid() != effective_uid()? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the hand-off directory is not this user's own",
            ));
        }
        if metadata.mode() & 0o777 != 0o700 {
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Writes the hand-off for the process it names, readable only by its
    /// owner, and replaces any earlier one in one rename. Nothing is written
    /// through a link: the file is created new, never opened if it exists.
    pub fn write(&self, handoff: &Handoff) -> io::Result<PathBuf> {
        let path = self
            .path_for(&handoff.app_id, handoff.pid)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid app id"))?;
        self.private_root()?;
        let partial = path.with_extension("json.partial");
        let text = serde_json::to_vec(handoff).map_err(io::Error::other)?;
        if text.len() as u64 > MAX_HANDOFF_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the view is too large to hand over",
            ));
        }
        let _ = fs::remove_file(&partial);
        {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&partial)?;
            file.write_all(&text)?;
            file.sync_all()?;
        }
        fs::rename(&partial, &path)?;
        Ok(path)
    }

    /// Reads and removes the hand-off left for `app_id` running as `pid`. A
    /// hand-off that is unreadable, a link, not this user's, larger than
    /// [`MAX_HANDOFF_BYTES`], for another program, or older than
    /// [`HANDOFF_MAX_AGE`] is removed and not used.
    pub fn take(&self, app_id: &str, pid: u32, now: SystemTime) -> Option<Handoff> {
        use std::io::Read as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let path = self.path_for(app_id, pid)?;
        let root = fs::symlink_metadata(&self.root).ok()?;
        let uid = effective_uid().ok()?;
        if !root.is_dir() || root.uid() != uid {
            return None;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path);
        let _ = fs::remove_file(&path);
        let file = file.ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.uid() != uid || metadata.len() > MAX_HANDOFF_BYTES {
            return None;
        }
        let mut text = Vec::new();
        file.take(MAX_HANDOFF_BYTES + 1)
            .read_to_end(&mut text)
            .ok()?;
        if text.len() as u64 > MAX_HANDOFF_BYTES {
            return None;
        }
        let handoff: Handoff = serde_json::from_slice(&text).ok()?;
        let age = unix_ms(now).saturating_sub(handoff.written_unix_ms);
        let fresh = age <= HANDOFF_MAX_AGE.as_millis() as u64;
        (handoff.format == HANDOFF_FORMAT
            && handoff.app_id == app_id
            && handoff.pid == pid
            && fresh)
            .then_some(handoff)
    }

    /// This process's own hand-off, if the program it replaced left one.
    pub fn take_own(&self, app_id: &str) -> Option<Handoff> {
        self.take(app_id, std::process::id(), SystemTime::now())
    }
}

/// Starts `program` in place of this process with the arguments it was started
/// with. The process id stays the same, which is how the new program finds its
/// hand-off, and every descriptor Rust opened closes on exec. It returns only
/// when the exec failed.
pub fn restart_in_place(program: &Path, arguments: &[OsString]) -> io::Error {
    use std::os::unix::process::CommandExt as _;
    std::process::Command::new(program).args(arguments).exec()
}

/// The arguments this process was started with, without the program name.
pub fn own_arguments() -> Vec<OsString> {
    std::env::args_os().skip(1).collect()
}

/// Wakes a process when anything happens to its program file. dpkg unpacks a
/// new file beside the old one and renames it over it, so the directory is
/// watched and events for other names are dropped.
pub struct ProgramWatch {
    _watcher: notify::RecommendedWatcher,
}

impl ProgramWatch {
    /// Calls `on_change` from the watcher's thread on every event that touches
    /// the program's path. It should only wake whoever then calls
    /// [`UpdateTracker::poll`].
    pub fn start(
        program: &Path,
        on_change: impl Fn() + Send + 'static,
    ) -> Result<ProgramWatch, notify::Error> {
        use notify::{RecursiveMode, Watcher as _};
        let directory = program
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| notify::Error::generic("the program has no directory"))?;
        let name = program.file_name().map(|name| name.to_os_string());
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    // An error from the watcher is a reason to look, not to stop.
                    on_change();
                    return;
                };
                let touches = event
                    .paths
                    .iter()
                    .any(|path| path.file_name().map(|n| n.to_os_string()) == name);
                if touches || event.need_rescan() {
                    on_change();
                }
            })?;
        watcher.watch(&directory, RecursiveMode::NonRecursive)?;
        Ok(ProgramWatch { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Puts a program at `path` the way dpkg does: a new file beside the old
    /// one, renamed over it. The returned handle keeps the file's inode alive,
    /// as a running program does, so the next file cannot be given the same one.
    fn install(path: &Path, contents: &str) -> fs::File {
        let partial = path.with_extension("dpkg-new");
        fs::write(&partial, contents).unwrap();
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755)).unwrap();
        let held = fs::File::open(&partial).unwrap();
        fs::rename(&partial, path).unwrap();
        held
    }

    /// dpkg as a test wants it: a record that changes when told to, and a
    /// count of how often it was asked.
    #[derive(Clone, Default)]
    struct FakeDpkg {
        inner: std::sync::Arc<std::sync::Mutex<(u32, Option<PackageRecord>, usize)>>,
    }

    impl FakeDpkg {
        fn set(&self, configured: bool, version: &str) {
            let mut inner = self.inner.lock().unwrap();
            inner.0 += 1;
            inner.1 = Some(PackageRecord {
                configured,
                version: Some(version.to_string()),
            });
        }

        fn asked(&self) -> usize {
            self.inner.lock().unwrap().2
        }
    }

    impl PackageSource for FakeDpkg {
        fn marker(&self) -> Vec<Option<SystemTime>> {
            let generation = self.inner.lock().unwrap().0;
            vec![Some(
                UNIX_EPOCH + Duration::from_secs(u64::from(generation)),
            )]
        }

        fn record(&self, _package: &str) -> Option<PackageRecord> {
            let mut inner = self.inner.lock().unwrap();
            inner.2 += 1;
            inner.1.clone()
        }
    }

    fn tracked(path: &Path, dpkg: &FakeDpkg) -> UpdateTracker {
        UpdateTracker::with_source(
            ProgramFile::at(path).unwrap(),
            Some("better-git"),
            Box::new(dpkg.clone()),
        )
    }

    #[test]
    fn the_deleted_suffix_is_not_part_of_the_path() {
        assert_eq!(
            strip_deleted(Path::new("/usr/bin/better-git (deleted)")),
            PathBuf::from("/usr/bin/better-git")
        );
        assert_eq!(
            strip_deleted(Path::new("/usr/bin/better-git")),
            PathBuf::from("/usr/bin/better-git")
        );
    }

    #[test]
    fn this_process_knows_its_own_program() {
        let program = ProgramFile::current().expect("the test binary has a path");
        assert_eq!(program.on_disk(), OnDisk::Same);
    }

    #[test]
    fn dpkg_query_output_says_whether_a_package_is_configured_and_its_version() {
        assert_eq!(
            parse_query_output("install ok installed\n0.2.13\n"),
            Some(PackageRecord {
                configured: true,
                version: Some("0.2.13".into())
            })
        );
        for status in [
            "install ok unpacked",
            "install ok half-configured",
            "install reinstreq half-installed",
        ] {
            let record = parse_query_output(&format!("{status}\n0.2.13\n")).unwrap();
            assert!(!record.configured, "{status}");
        }
        assert_eq!(parse_query_output("unknown ok not-installed\n\n"), None);
        assert_eq!(parse_query_output(""), None);
    }

    /// The case the status file alone gets wrong: while dpkg runs a postinst
    /// its change is in the updates journal, and the status file still says
    /// the old version is installed. dpkg-query, run against a scratch admin
    /// directory, applies the journal. Skipped where there is no dpkg.
    #[test]
    fn the_updates_journal_wins_over_a_stale_status_file() {
        if std::process::Command::new("dpkg-query")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let tmp = scratch();
        let admin = tmp.path();
        fs::create_dir_all(admin.join("updates")).unwrap();
        fs::create_dir_all(admin.join("info")).unwrap();
        let stanza = |status: &str, version: &str| {
            format!(
                "Package: better-git\nStatus: {status}\nPriority: optional\nSection: utils\n\
                 Maintainer: T <t@example.invalid>\nArchitecture: all\nVersion: {version}\n\
                 Description: test\n"
            )
        };
        fs::write(
            admin.join("status"),
            stanza("install ok installed", "0.2.12"),
        )
        .unwrap();
        let source = DpkgSource {
            admin_dir: admin.to_path_buf(),
        };
        assert_eq!(
            source.record("better-git"),
            Some(PackageRecord {
                configured: true,
                version: Some("0.2.12".into())
            })
        );
        let before = source.marker();
        fs::write(
            admin.join("updates").join("0001"),
            stanza("install ok half-configured", "0.2.13"),
        )
        .unwrap();
        assert_ne!(source.marker(), before, "the journal moving is a change");
        assert_eq!(
            source.record("better-git"),
            Some(PackageRecord {
                configured: false,
                version: Some("0.2.13".into())
            })
        );
        assert_eq!(source.record("better-files"), None);
    }

    #[test]
    fn an_unpackaged_program_restarts_once_the_new_file_has_settled() {
        let tmp = scratch();
        let dir = tmp.path();
        let path = dir.join("better-git");
        let _old = install(&path, "old");
        let mut tracker = UpdateTracker::new(ProgramFile::at(&path).unwrap(), None);
        let start = Instant::now();
        assert_eq!(tracker.poll(start), UpdateState::Current);

        fs::remove_file(&path).unwrap();
        assert_eq!(tracker.poll(start), UpdateState::InProgress);

        let _new = install(&path, "new");
        assert_eq!(tracker.poll(start), UpdateState::InProgress);
        assert_eq!(
            tracker.poll(start + UNPACKAGED_SETTLE - Duration::from_millis(1)),
            UpdateState::InProgress
        );
        assert_eq!(
            tracker.poll(start + UNPACKAGED_SETTLE),
            UpdateState::Ready { version: None }
        );
    }

    #[test]
    fn a_file_replaced_again_starts_the_wait_over() {
        let tmp = scratch();
        let dir = tmp.path();
        let path = dir.join("better-git");
        let _old = install(&path, "old");
        let mut tracker = UpdateTracker::new(ProgramFile::at(&path).unwrap(), None);
        let start = Instant::now();
        let _new = install(&path, "new");
        assert_eq!(tracker.poll(start), UpdateState::InProgress);
        let _newer = install(&path, "newer");
        let later = start + UNPACKAGED_SETTLE;
        assert_eq!(tracker.poll(later), UpdateState::InProgress);
        assert_eq!(
            tracker.poll(later + UNPACKAGED_SETTLE),
            UpdateState::Ready { version: None }
        );
    }

    #[test]
    fn a_packaged_program_waits_for_dpkg_to_configure_the_new_version() {
        let tmp = scratch();
        let path = tmp.path().join("better-git");
        let _old = install(&path, "old");
        let dpkg = FakeDpkg::default();
        dpkg.set(true, "0.2.12");
        let mut tracker = tracked(&path, &dpkg);
        let start = Instant::now();
        let _new = install(&path, "new");
        // Unpacked, then half-configured while the postinst runs.
        dpkg.set(false, "0.2.13");
        assert_eq!(tracker.poll(start), UpdateState::InProgress);
        assert_eq!(
            tracker.poll(start + Duration::from_secs(60)),
            UpdateState::InProgress
        );
        dpkg.set(true, "0.2.13");
        assert_eq!(
            tracker.poll(start + Duration::from_secs(61)),
            UpdateState::Ready {
                version: Some("0.2.13".into())
            }
        );
    }

    #[test]
    fn the_old_version_still_marked_installed_is_not_the_update() {
        // What a stale record looks like: the file is new, dpkg still says
        // the version the program started with is installed.
        let tmp = scratch();
        let path = tmp.path().join("better-git");
        let _old = install(&path, "old");
        let dpkg = FakeDpkg::default();
        dpkg.set(true, "0.2.12");
        let mut tracker = tracked(&path, &dpkg);
        let start = Instant::now();
        let _new = install(&path, "new");
        assert_eq!(tracker.poll(start), UpdateState::InProgress);
        assert_eq!(
            tracker.poll(start + PACKAGED_SETTLE),
            UpdateState::InProgress,
            "the same version, not yet settled as a reinstall"
        );
        assert_eq!(
            tracker.poll(start + REINSTALL_SETTLE),
            UpdateState::Ready {
                version: Some("0.2.12".into())
            },
            "a reinstall of the same version, once it has settled"
        );
    }

    #[test]
    fn dpkg_is_only_asked_again_when_its_record_moved() {
        let tmp = scratch();
        let path = tmp.path().join("better-git");
        let _old = install(&path, "old");
        let dpkg = FakeDpkg::default();
        dpkg.set(false, "0.2.13");
        let mut tracker = tracked(&path, &dpkg);
        let _new = install(&path, "new");
        let start = Instant::now();
        let asked = dpkg.asked();
        for step in 0..20 {
            tracker.poll(start + Duration::from_millis(500 * step));
        }
        assert_eq!(dpkg.asked(), asked + 1, "once, then not until it moves");
        dpkg.set(true, "0.2.13");
        tracker.poll(start + Duration::from_secs(20));
        assert_eq!(dpkg.asked(), asked + 2);
    }

    #[test]
    fn a_package_left_unconfigured_still_restarts_after_a_long_wait() {
        let tmp = scratch();
        let path = tmp.path().join("better-git");
        let _old = install(&path, "old");
        let dpkg = FakeDpkg::default();
        dpkg.set(true, "0.2.12");
        let mut tracker = tracked(&path, &dpkg);
        let start = Instant::now();
        let _new = install(&path, "new");
        dpkg.set(false, "0.2.13");
        assert_eq!(tracker.poll(start), UpdateState::InProgress);
        assert_eq!(
            tracker.poll(start + CONFIGURE_PATIENCE),
            UpdateState::Ready {
                version: Some("0.2.13".into())
            }
        );
    }

    #[test]
    fn a_program_dpkg_does_not_know_is_treated_as_unpackaged() {
        let tmp = scratch();
        let path = tmp.path().join("better-git");
        let _old = install(&path, "old");
        let dpkg = FakeDpkg::default();
        let mut tracker = tracked(&path, &dpkg);
        let start = Instant::now();
        let _new = install(&path, "new");
        tracker.poll(start);
        assert_eq!(
            tracker.poll(start + UNPACKAGED_SETTLE),
            UpdateState::Ready { version: None }
        );
    }

    #[test]
    fn a_file_that_is_not_executable_is_not_a_new_program() {
        let tmp = scratch();
        let dir = tmp.path();
        let path = dir.join("better-git");
        let _old = install(&path, "old");
        let program = ProgramFile::at(&path).unwrap();
        fs::write(dir.join("plain"), "new").unwrap();
        fs::rename(dir.join("plain"), &path).unwrap();
        assert_eq!(program.on_disk(), OnDisk::Missing);
    }

    #[test]
    fn work_that_cannot_be_carried_over_holds_the_restart() {
        let ready = UpdateState::Ready { version: None };
        assert_eq!(decide(&ready, false), RestartDecision::Restart);
        assert_eq!(decide(&ready, true), RestartDecision::Wait);
        for state in [UpdateState::Current, UpdateState::InProgress] {
            assert_eq!(decide(&state, false), RestartDecision::Stay);
            assert_eq!(decide(&state, true), RestartDecision::Stay);
        }
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct View {
        folder: String,
        #[serde(default)]
        scroll: f32,
    }

    #[test]
    fn a_handoff_reaches_the_same_process_once() {
        let tmp = scratch();
        let dir = HandoffDir::at(tmp.path().join("restart"));
        let view = View {
            folder: "/home/someone/Documents".into(),
            scroll: 120.0,
        };
        let placement = WindowPlacement {
            x: 10.0,
            y: 20.0,
            width: 1280.0,
            height: 800.0,
            mode: WindowMode::Maximized,
        };
        let handoff = Handoff::new(
            "io.betteros.Files",
            Some("0.2.13".into()),
            Some(placement),
            &view,
        );
        let path = dir.write(&handoff).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // Another program, or another process, finds nothing.
        assert_eq!(
            dir.take("io.betteros.Git", handoff.pid, SystemTime::now()),
            None
        );
        assert_eq!(
            dir.take("io.betteros.Files", handoff.pid + 1, SystemTime::now()),
            None
        );

        let taken = dir
            .take("io.betteros.Files", handoff.pid, SystemTime::now())
            .expect("the hand-off is there");
        assert_eq!(taken.updated_to.as_deref(), Some("0.2.13"));
        assert_eq!(taken.window, Some(placement));
        assert_eq!(taken.state::<View>(), Some(view));
        // Read once: a later start of the same id does not see it again.
        assert_eq!(
            dir.take("io.betteros.Files", handoff.pid, SystemTime::now()),
            None
        );
    }

    #[test]
    fn an_old_or_broken_handoff_is_removed_and_not_used() {
        let tmp = scratch();
        let root = tmp.path().join("restart");
        let dir = HandoffDir::at(&root);
        let handoff = Handoff::new("io.betteros.Git", None, None, &View::default());
        let path = dir.write(&handoff).unwrap();
        let later = SystemTime::now() + HANDOFF_MAX_AGE + Duration::from_secs(1);
        assert_eq!(dir.take("io.betteros.Git", handoff.pid, later), None);
        assert!(!path.exists());

        fs::write(&path, "not json").unwrap();
        assert_eq!(
            dir.take("io.betteros.Git", handoff.pid, SystemTime::now()),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_state_the_new_program_does_not_read_costs_only_the_view() {
        let tmp = scratch();
        let dir = HandoffDir::at(tmp.path().join("restart"));
        let handoff = Handoff::new("io.betteros.Git", Some("1".into()), None, &vec![1, 2, 3]);
        dir.write(&handoff).unwrap();
        let taken = dir
            .take("io.betteros.Git", handoff.pid, SystemTime::now())
            .unwrap();
        assert_eq!(taken.state::<View>(), None);
        assert_eq!(taken.updated_to.as_deref(), Some("1"));
    }

    #[test]
    fn a_handoff_directory_that_is_a_link_or_a_handoff_that_is_a_link_is_refused() {
        let tmp = scratch();
        let real = tmp.path().join("elsewhere");
        fs::create_dir_all(&real).unwrap();
        let linked = tmp.path().join("restart");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let dir = HandoffDir::at(&linked);
        let handoff = Handoff::new("io.betteros.Git", None, None, &View::default());
        assert!(dir.write(&handoff).is_err(), "never written through a link");

        let tmp = scratch();
        let dir = HandoffDir::at(tmp.path().join("restart"));
        let handoff = Handoff::new("io.betteros.Git", None, None, &View::default());
        let path = dir.write(&handoff).unwrap();
        let target = tmp.path().join("target.json");
        fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(
            dir.take("io.betteros.Git", handoff.pid, SystemTime::now()),
            None,
            "a hand-off that is a link is not followed"
        );
        assert!(!path.exists());
    }

    #[test]
    fn an_oversized_handoff_is_not_read() {
        let tmp = scratch();
        let dir = HandoffDir::at(tmp.path().join("restart"));
        let handoff = Handoff::new("io.betteros.Git", None, None, &View::default());
        let path = dir.write(&handoff).unwrap();
        let mut huge = serde_json::to_vec(&handoff).unwrap();
        huge.resize(MAX_HANDOFF_BYTES as usize + 10, b' ');
        fs::write(&path, huge).unwrap();
        assert_eq!(
            dir.take("io.betteros.Git", handoff.pid, SystemTime::now()),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn the_handoff_directory_is_made_private() {
        let tmp = scratch();
        let root = tmp.path().join("restart");
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let dir = HandoffDir::at(&root);
        dir.write(&Handoff::new(
            "io.betteros.Git",
            None,
            None,
            &View::default(),
        ))
        .unwrap();
        let mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn an_application_id_that_is_not_a_plain_name_is_refused() {
        let tmp = scratch();
        let dir = HandoffDir::at(tmp.path().join("restart"));
        for id in ["", "../escape", "a/b", ".hidden"] {
            let handoff = Handoff::new(id, None, None, &View::default());
            assert!(dir.write(&handoff).is_err(), "{id}");
            assert_eq!(dir.take(id, handoff.pid, SystemTime::now()), None);
        }
    }

    #[test]
    fn the_runtime_directory_decides_where_handoffs_go() {
        // Read-only: the environment is inspected, not changed.
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(runtime) if Path::new(&runtime).is_absolute() => assert_eq!(
                HandoffDir::from_environment(),
                Some(HandoffDir::at(
                    PathBuf::from(runtime).join("better-os").join("restart")
                ))
            ),
            _ => assert_eq!(HandoffDir::from_environment(), None),
        }
    }

    #[test]
    fn the_watch_wakes_on_a_replacement_and_not_on_a_neighbour() {
        let tmp = scratch();
        let dir = tmp.path();
        let path = dir.join("better-git");
        let _old = install(&path, "old");
        let (sender, receiver) = std::sync::mpsc::channel();
        let _watch = ProgramWatch::start(&path, move || {
            let _ = sender.send(());
        })
        .unwrap();
        fs::write(dir.join("neighbour"), "x").unwrap();
        assert!(
            receiver.recv_timeout(Duration::from_millis(300)).is_err(),
            "a neighbour's file is not the program"
        );
        let _new = install(&path, "new");
        assert!(receiver.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}

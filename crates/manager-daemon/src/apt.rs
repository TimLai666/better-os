//! Driving local APT.
//!
//! The real driver is deliberately thin: it spawns `apt-get` and reports what
//! happened. All the decisions — what to install, in which order, when to roll
//! back — live in [`crate::executor`], which is tested against
//! [`FakeAptDriver`] instead of a package manager.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use manager_ipc::{LogEntry, MAX_LOG_OUTPUT_BYTES};

use crate::DaemonError;
use crate::dpkg_config::PathFilter;

/// What one `apt-get` or `dpkg` invocation did.
#[derive(Clone, Debug)]
pub struct AptRun {
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub started_at_unix: u64,
    pub finished_at_unix: u64,
}

impl AptRun {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    pub fn into_log_entry(self) -> LogEntry {
        LogEntry {
            argv: self.argv,
            exit_code: self.exit_code,
            stdout_tail: self.stdout_tail,
            stderr_tail: self.stderr_tail,
            started_at_unix: self.started_at_unix,
            finished_at_unix: self.finished_at_unix,
        }
    }

    /// Whether the failure was another package manager holding the dpkg lock,
    /// which is worth telling a user apart from a broken package.
    pub fn is_lock_contention(&self) -> bool {
        !self.succeeded()
            && (self.stderr_tail.contains("Could not get lock")
                || self.stderr_tail.contains("dpkg frontend lock")
                || self
                    .stderr_tail
                    .contains("Unable to acquire the dpkg frontend lock"))
    }
}

/// The control fields the daemon cross-checks a `.deb` against, so a package
/// cannot claim to be something it is not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebFields {
    pub package: String,
    pub version: String,
    pub architecture: String,
}

/// What dpkg's own file database says a package put on disk.
///
/// `paths` is everything `dpkg-query -L` listed, directories included, in the
/// order dpkg reported. `conffiles` is the subset dpkg treats as
/// administrator-owned configuration, which a machine's owner is allowed to
/// delete without the package being broken. `excluded` is the subset this
/// machine's dpkg was configured never to unpack, which is listed all the same.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstalledFiles {
    pub paths: Vec<PathBuf>,
    pub conffiles: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
}

impl InstalledFiles {
    /// Whether this path has to be on disk for the package to be sound.
    ///
    /// A conffile need not: dpkg lets a machine's owner delete one. A
    /// path-excluded file need not: dpkg was told not to install it.
    pub fn is_required(&self, path: &Path) -> bool {
        !self.conffiles.iter().any(|known| known == path)
            && !self.excluded.iter().any(|known| known == path)
    }
}

pub trait AptDriver: Send + Sync {
    fn install_local_deb(
        &self,
        deb_path: &Path,
        allow_downgrade: bool,
    ) -> Result<AptRun, DaemonError>;
    fn remove(&self, package: &str) -> Result<AptRun, DaemonError>;
    fn installed_version(&self, package: &str) -> Result<Option<String>, DaemonError>;
    fn deb_control_fields(&self, deb_path: &Path) -> Result<DebFields, DaemonError>;
    /// The files dpkg recorded for an installed package.
    ///
    /// This is dpkg's account of the package the daemon just verified and
    /// installed, which is why the health check may trust it: nothing a
    /// manifest wrote reaches it.
    fn installed_files(&self, package: &str) -> Result<InstalledFiles, DaemonError>;
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_LOG_OUTPUT_BYTES {
        return text.into_owned();
    }
    // Keep the end: a failure explains itself in its last lines.
    let start = text.len() - MAX_LOG_OUTPUT_BYTES;
    let boundary = (start..text.len())
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(text.len());
    format!("[truncated]\n{}", &text[boundary..])
}

/// The real driver.
pub struct AptGetDriver;

impl AptGetDriver {
    fn spawn(program: &str, arguments: &[&str]) -> Result<std::process::Output, DaemonError> {
        // A cleared environment keeps whatever the caller's session had set out
        // of a root process, and pins the frontend to something non-interactive
        // so a maintainer script can never block waiting for a prompt.
        Command::new(program)
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .env("APT_LISTCHANGES_FRONTEND", "none")
            .env("LC_ALL", "C")
            .output()
            .map_err(|error| DaemonError::Storage(error.to_string()))
    }

    fn run(&self, program: &str, arguments: &[&str]) -> Result<AptRun, DaemonError> {
        let started_at_unix = now_unix();
        let output = Self::spawn(program, arguments)?;

        let mut argv = vec![program.to_string()];
        argv.extend(arguments.iter().map(|argument| argument.to_string()));
        Ok(AptRun {
            argv,
            exit_code: output.status.code().unwrap_or(-1),
            stdout_tail: tail(&output.stdout),
            stderr_tail: tail(&output.stderr),
            started_at_unix,
            finished_at_unix: now_unix(),
        })
    }

    /// The path filters in force for the installs this driver performs.
    ///
    /// dpkg's own configuration, plus anything apt adds to the dpkg command
    /// line — this driver calls `apt-get`, so apt's `DPkg::Options` reach dpkg
    /// whether or not the driver named them. An `apt-config` that cannot be run
    /// contributes nothing rather than failing the query: dpkg's own files are
    /// where a minimized image writes its filters.
    fn path_filter(&self) -> PathFilter {
        let mut filter = PathFilter::from_config_dir(Path::new("/etc/dpkg"));
        if let Ok((true, dumped)) = self.query("apt-config", &["dump", "DPkg::Options"]) {
            for line in dumped.lines() {
                if let Some(option) = parse_apt_config_value(line) {
                    filter.push_option(option);
                }
            }
        }
        filter
    }

    /// Runs a query for its whole output rather than for a log entry.
    ///
    /// A package's file list is longer than the log tail a transaction keeps,
    /// and a health check that saw a truncated list would be checking fewer
    /// files than the package installed without saying so.
    fn query(&self, program: &str, arguments: &[&str]) -> Result<(bool, String), DaemonError> {
        let output = Self::spawn(program, arguments)?;
        if !output.status.success() {
            return Ok((false, tail(&output.stderr)));
        }
        Ok((true, String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

impl AptDriver for AptGetDriver {
    fn install_local_deb(
        &self,
        deb_path: &Path,
        allow_downgrade: bool,
    ) -> Result<AptRun, DaemonError> {
        let path = deb_path.to_string_lossy().into_owned();
        // `apt-get install ./file.deb` resolves dependencies in one
        // transaction. `dpkg -i` followed by `apt-get -f install` would leave a
        // half-configured system in between.
        let mut arguments = vec![
            "install",
            "--yes",
            "--no-install-recommends",
            "-o",
            "DPkg::Lock::Timeout=120",
            "-o",
            "Dpkg::Options::=--force-confdef",
            "-o",
            "Dpkg::Options::=--force-confold",
        ];
        if allow_downgrade {
            arguments.push("--allow-downgrades");
        }
        arguments.push("--");
        arguments.push(&path);
        self.run("apt-get", &arguments)
    }

    fn remove(&self, package: &str) -> Result<AptRun, DaemonError> {
        // Remove, not purge: deleting a component's configuration is a separate
        // decision from uninstalling it.
        self.run(
            "apt-get",
            &[
                "remove",
                "--yes",
                "-o",
                "DPkg::Lock::Timeout=120",
                "--",
                package,
            ],
        )
    }

    fn installed_version(&self, package: &str) -> Result<Option<String>, DaemonError> {
        let run = self.run(
            "dpkg-query",
            &["-W", "-f=${db:Status-Status} ${Version}", package],
        )?;
        if !run.succeeded() {
            // dpkg-query exits non-zero for a package it has never heard of,
            // which is not an error here: it means "not installed".
            return Ok(None);
        }
        let mut parts = run.stdout_tail.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("installed"), Some(version)) => Ok(Some(version.to_string())),
            _ => Ok(None),
        }
    }

    fn deb_control_fields(&self, deb_path: &Path) -> Result<DebFields, DaemonError> {
        let path = deb_path.to_string_lossy().into_owned();
        let run = self.run(
            "dpkg-deb",
            &["-f", &path, "Package", "Version", "Architecture"],
        )?;
        if !run.succeeded() {
            return Err(DaemonError::PlanRejected("unreadable package".to_string()));
        }
        parse_control_fields(&run.stdout_tail)
    }

    fn installed_files(&self, package: &str) -> Result<InstalledFiles, DaemonError> {
        // `dpkg-query -L` rather than `dpkg -L`: same database, same output, and
        // it is the query tool the rest of this driver already uses.
        let (listed_ok, listed) = self.query("dpkg-query", &["-L", "--", package])?;
        if !listed_ok {
            return Err(DaemonError::Storage(format!(
                "dpkg-query -L {package} failed: {}",
                listed.trim()
            )));
        }
        let (conffiles_ok, conffiles) =
            self.query("dpkg-query", &["-W", "-f=${Conffiles}", "--", package])?;
        if !conffiles_ok {
            return Err(DaemonError::Storage(format!(
                "dpkg-query -W {package} failed: {}",
                conffiles.trim()
            )));
        }
        let paths = parse_file_list(&listed);
        let filter = self.path_filter();
        let excluded = if filter.is_empty() {
            Vec::new()
        } else {
            paths
                .iter()
                .filter(|path| filter.excludes(path))
                .cloned()
                .collect()
        };
        Ok(InstalledFiles {
            paths,
            conffiles: parse_conffiles(&conffiles),
            excluded,
        })
    }
}

/// The value out of one `apt-config dump` line, which is written
/// `DPkg::Options:: "--path-exclude=/usr/share/doc/*";`.
fn parse_apt_config_value(line: &str) -> Option<&str> {
    let (_, value) = line.trim().split_once(' ')?;
    let value = value.trim().trim_end_matches(';').trim_matches('"').trim();
    (!value.is_empty()).then_some(value)
}

/// Keeps the absolute paths out of a `dpkg-query -L` listing.
///
/// dpkg prints one path per line and, for a package involved in a diversion,
/// explanatory lines that are not paths. Only lines that start at the root are
/// files this package is answerable for.
fn parse_file_list(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .map(str::trim_end)
        .filter(|line| line.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// Reads the paths out of dpkg's `${Conffiles}` field, which is one
/// ` /path hash [obsolete]` line per configuration file.
fn parse_conffiles(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|path| path.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

fn parse_control_fields(output: &str) -> Result<DebFields, DaemonError> {
    let mut fields = HashMap::new();
    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let take = |key: &str| {
        fields
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| DaemonError::PlanRejected(format!("package has no {key} field")))
    };
    Ok(DebFields {
        package: take("package")?,
        version: take("version")?,
        architecture: take("architecture")?,
    })
}

/// A scripted APT for tests. It records what it was asked to do and returns
/// whatever the test decided should happen.
pub struct FakeAptDriver {
    installed: Mutex<HashMap<String, String>>,
    control: Mutex<HashMap<String, DebFields>>,
    /// What dpkg would list for a package. A package with no entry here ships
    /// one binary named after itself, which is the ordinary single-binary
    /// shape; [`FakeAptDriver::with_files`] declares any other.
    files: Mutex<HashMap<String, InstalledFiles>>,
    /// Packages whose file list cannot be read, for the case where the health
    /// check has to report `Undetermined` rather than a verdict.
    unreadable_files: Mutex<Vec<String>>,
    /// Installing or removing are scripted separately: a package that fails to
    /// install can usually still be removed, and rollback depends on that.
    pub failing_install: Mutex<Vec<String>>,
    pub failing_remove: Mutex<Vec<String>>,
    pub lock_contention: Mutex<bool>,
    pub calls: Mutex<Vec<String>>,
}

impl Default for FakeAptDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAptDriver {
    pub fn new() -> Self {
        Self {
            installed: Mutex::new(HashMap::new()),
            control: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
            unreadable_files: Mutex::new(Vec::new()),
            failing_install: Mutex::new(Vec::new()),
            failing_remove: Mutex::new(Vec::new()),
            lock_contention: Mutex::new(false),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn with_installed(self, package: &str, version: &str) -> Self {
        self.installed
            .lock()
            .unwrap()
            .insert(package.to_string(), version.to_string());
        self
    }

    /// Declares the file list dpkg would report for a package, which is how a
    /// test describes a package shape other than one eponymous binary — a
    /// multi-binary package, or one with configuration files.
    pub fn with_files(self, package: &str, paths: &[&str], conffiles: &[&str]) -> Self {
        self.files.lock().unwrap().insert(
            package.to_string(),
            InstalledFiles {
                paths: paths.iter().map(PathBuf::from).collect(),
                conffiles: conffiles.iter().map(PathBuf::from).collect(),
                excluded: Vec::new(),
            },
        );
        self
    }

    /// Declares paths dpkg listed but this machine was configured never to
    /// unpack, which is what a minimized image does to `/usr/share/doc`.
    pub fn with_excluded_files(self, package: &str, excluded: &[&str]) -> Self {
        if let Some(files) = self.files.lock().unwrap().get_mut(package) {
            files.excluded = excluded.iter().map(PathBuf::from).collect();
        }
        self
    }

    /// Makes the file list unreadable for this package, the way a broken dpkg
    /// database would be.
    pub fn with_unreadable_files(self, package: &str) -> Self {
        self.unreadable_files
            .lock()
            .unwrap()
            .push(package.to_string());
        self
    }

    /// Declares what a staged file claims to be.
    pub fn with_deb(self, filename: &str, fields: DebFields) -> Self {
        self.control
            .lock()
            .unwrap()
            .insert(filename.to_string(), fields);
        self
    }

    /// Makes installing this package fail. Removing it still works, which is
    /// what lets a rollback succeed.
    pub fn fail_install(&self, package: &str) {
        self.failing_install
            .lock()
            .unwrap()
            .push(package.to_string());
    }

    /// Makes removing this package fail, so a rollback cannot complete.
    pub fn fail_remove(&self, package: &str) {
        self.failing_remove
            .lock()
            .unwrap()
            .push(package.to_string());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn filename(path: &Path) -> String {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn run(&self, argv: Vec<String>, failed: bool) -> AptRun {
        AptRun {
            argv,
            exit_code: if failed { 100 } else { 0 },
            stdout_tail: String::new(),
            stderr_tail: if failed && *self.lock_contention.lock().unwrap() {
                "E: Could not get lock /var/lib/dpkg/lock-frontend".to_string()
            } else if failed {
                "E: Sub-process /usr/bin/dpkg returned an error code".to_string()
            } else {
                String::new()
            },
            started_at_unix: 1,
            finished_at_unix: 2,
        }
    }
}

impl AptDriver for FakeAptDriver {
    fn install_local_deb(
        &self,
        deb_path: &Path,
        allow_downgrade: bool,
    ) -> Result<AptRun, DaemonError> {
        let filename = Self::filename(deb_path);
        let fields = self.control.lock().unwrap().get(&filename).cloned();
        let package = fields
            .as_ref()
            .map(|fields| fields.package.clone())
            .unwrap_or_else(|| filename.clone());
        self.calls
            .lock()
            .unwrap()
            .push(format!("install:{package}:downgrade={allow_downgrade}"));

        let failed = self.failing_install.lock().unwrap().contains(&package);
        if !failed && let Some(fields) = fields {
            self.installed
                .lock()
                .unwrap()
                .insert(fields.package.clone(), fields.version.clone());
        }
        Ok(self.run(vec!["apt-get".to_string(), "install".to_string()], failed))
    }

    fn remove(&self, package: &str) -> Result<AptRun, DaemonError> {
        self.calls.lock().unwrap().push(format!("remove:{package}"));
        let failed = self
            .failing_remove
            .lock()
            .unwrap()
            .contains(&package.to_string());
        if !failed {
            self.installed.lock().unwrap().remove(package);
        }
        Ok(self.run(vec!["apt-get".to_string(), "remove".to_string()], failed))
    }

    fn installed_version(&self, package: &str) -> Result<Option<String>, DaemonError> {
        Ok(self.installed.lock().unwrap().get(package).cloned())
    }

    fn deb_control_fields(&self, deb_path: &Path) -> Result<DebFields, DaemonError> {
        let filename = Self::filename(deb_path);
        self.control
            .lock()
            .unwrap()
            .get(&filename)
            .cloned()
            .ok_or_else(|| DaemonError::PlanRejected("unreadable package".to_string()))
    }

    fn installed_files(&self, package: &str) -> Result<InstalledFiles, DaemonError> {
        if self
            .unreadable_files
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == package)
        {
            return Err(DaemonError::Storage(format!(
                "dpkg-query -L {package} failed"
            )));
        }
        if let Some(files) = self.files.lock().unwrap().get(package) {
            return Ok(files.clone());
        }
        Ok(InstalledFiles {
            paths: vec![PathBuf::from("/usr/bin").join(package)],
            conffiles: Vec::new(),
            excluded: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_fields_are_parsed_from_dpkg_deb_output() {
        let fields =
            parse_control_fields("Package: better-monitor\nVersion: 0.1.0\nArchitecture: amd64\n")
                .unwrap();
        assert_eq!(
            fields,
            DebFields {
                package: "better-monitor".to_string(),
                version: "0.1.0".to_string(),
                architecture: "amd64".to_string(),
            }
        );
    }

    #[test]
    fn a_package_missing_a_control_field_is_refused() {
        assert!(parse_control_fields("Package: better-monitor\nVersion:\n").is_err());
    }

    #[test]
    fn a_file_listing_keeps_paths_and_drops_dpkgs_prose() {
        // The second line is what dpkg prints for a diverted file. It is not a
        // path this package has to have on disk.
        let listed = parse_file_list(
            "/.\n\
             package diverts others to: /usr/bin/better-awake-service.distrib\n\
             /usr/bin\n\
             /usr/bin/better-awake-service\n\
             /usr/bin/awake-tray\n",
        );
        assert_eq!(
            listed,
            vec![
                PathBuf::from("/."),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/bin/better-awake-service"),
                PathBuf::from("/usr/bin/awake-tray"),
            ]
        );
    }

    #[test]
    fn conffile_paths_are_read_without_their_checksums() {
        let conffiles = parse_conffiles(
            " /etc/better-os/awake.conf 3d5e0c2f9a1b4c6d7e8f90a1b2c3d4e5\n\
             /etc/better-os/old.conf 0123456789abcdef0123456789abcdef obsolete\n",
        );
        assert_eq!(
            conffiles,
            vec![
                PathBuf::from("/etc/better-os/awake.conf"),
                PathBuf::from("/etc/better-os/old.conf"),
            ]
        );
        assert!(parse_conffiles("").is_empty());
    }

    #[test]
    fn an_apt_config_line_yields_the_option_apt_would_pass_to_dpkg() {
        // The shape `apt-config dump` prints, confirmed against apt on the host.
        assert_eq!(
            parse_apt_config_value("DPkg::Options:: \"--path-exclude=/usr/share/doc/*\";"),
            Some("--path-exclude=/usr/share/doc/*")
        );
        assert_eq!(
            parse_apt_config_value("DPkg::Options:: \"--force-confold\";"),
            Some("--force-confold")
        );
        assert_eq!(parse_apt_config_value(""), None);
        assert_eq!(parse_apt_config_value("DPkg::Options \"\";"), None);
    }

    #[test]
    fn lock_contention_is_told_apart_from_a_broken_package() {
        let contended = AptRun {
            argv: Vec::new(),
            exit_code: 100,
            stdout_tail: String::new(),
            stderr_tail: "E: Could not get lock /var/lib/dpkg/lock-frontend".to_string(),
            started_at_unix: 0,
            finished_at_unix: 0,
        };
        assert!(contended.is_lock_contention());

        let broken = AptRun {
            stderr_tail: "E: Sub-process /usr/bin/dpkg returned an error code".to_string(),
            ..contended.clone()
        };
        assert!(!broken.is_lock_contention());
    }

    #[test]
    fn long_command_output_keeps_its_tail_rather_than_being_dropped() {
        let noisy = "x".repeat(MAX_LOG_OUTPUT_BYTES * 2);
        let kept = tail(noisy.as_bytes());
        assert!(kept.starts_with("[truncated]"));
        assert!(kept.len() < noisy.len());
        assert!(kept.ends_with('x'));
    }
}

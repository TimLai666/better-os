//! Which applications are running, from `/proc/<pid>/comm` and
//! `/proc/<pid>/cgroup`.
//!
//! # What is deliberately not read
//!
//! `/proc/<pid>/cmdline` is never opened. A command line routinely carries
//! access tokens, database URLs, and the names of the documents someone is
//! working on, and ticket 26 requires that none of that reach the history file.
//! The safest way to keep it out of a file is to never read it into memory, so
//! the matchable surface is the kernel's own short name in `comm` and, where the
//! session recorded one, the desktop identifier of the application.
//!
//! Desktop identifiers come from the systemd application scope a desktop
//! launcher puts an application into: `app-gnome-org.gnome.Builder-1234.scope`
//! names `org.gnome.Builder`. A process that was not started from a `.desktop`
//! file simply has no identifier, which is not a failure and is not reported as
//! one.

use awake_core::{Observations, ProviderKind};

use crate::provider::{Cadence, PROCESS_POLL_SECONDS, TriggerProvider};
use crate::roots::{ReadError, Roots, list_dir, read_attribute, read_text};

/// More processes than this and the machine has bigger problems than a
/// keep-awake rule. The bound exists so one runaway fork bomb cannot turn a
/// five-second poll into an allocation storm.
pub const MAX_SCANNED_PROCESSES: usize = 8_192;

/// The two lists one scan produces.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProcessScan {
    /// Distinct `comm` values, sorted.
    pub executable_names: Vec<String>,
    /// Distinct desktop identifiers, sorted.
    pub desktop_ids: Vec<String>,
    /// Whether any cgroup file could be read. When nothing could, desktop
    /// identifiers are unavailable rather than empty.
    pub cgroups_readable: bool,
}

/// Extracts a desktop identifier from a systemd cgroup path.
///
/// Recognizes the `app-<launcher>-<id>-<pid>.scope` and `app-<id>-<pid>.scope`
/// forms that GNOME, KDE, and Flatpak produce. Anything else yields nothing,
/// which is the correct answer for a system service or a shell job.
pub fn desktop_id_from_cgroup(cgroup: &str) -> Option<String> {
    for line in cgroup.lines() {
        // Both cgroup v1 (`12:name=systemd:/path`) and v2 (`0::/path`) put the
        // path last, so the last colon-separated field is the one to read.
        let path = line.rsplit(':').next()?;
        for segment in path.split('/') {
            let Some(unit) = segment.strip_suffix(".scope") else {
                continue;
            };
            let Some(unit) = unit.strip_prefix("app-") else {
                continue;
            };
            // systemd escapes characters it cannot put in a unit name. A hyphen
            // in a desktop identifier arrives as `\x2d`, and leaving it escaped
            // would make `org.gnome.Text\x2dEditor` fail to match a rule the
            // user wrote as `org.gnome.Text-Editor`.
            let unit = unit.replace("\\x2d", "-");
            // Drop the trailing `-<pid>`, which is instance identity and not
            // part of the application's name.
            let unit = match unit.rsplit_once('-') {
                Some((head, tail))
                    if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) =>
                {
                    head
                }
                _ => unit.as_str(),
            };
            // Drop the launcher prefix, if one is there.
            let identifier = ["gnome-", "flatpak-", "snap-", "kde-"]
                .iter()
                .find_map(|prefix| unit.strip_prefix(prefix))
                .unwrap_or(unit);
            if identifier.is_empty() {
                continue;
            }
            return Some(identifier.to_string());
        }
    }
    None
}

/// Scans `/proc` for running applications.
#[derive(Clone, Debug)]
pub struct ProcessProvider {
    roots: Roots,
}

impl ProcessProvider {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }

    /// One scan of `/proc`.
    pub fn scan(&self) -> Result<ProcessScan, ReadError> {
        let entries = list_dir(self.roots.proc_dir())?;
        let mut scan = ProcessScan::default();

        for entry in entries.into_iter().take(MAX_SCANNED_PROCESSES) {
            let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            // Only the numeric entries are processes. `/proc/self` and
            // `/proc/stat` are not.
            if !name.chars().all(|character| character.is_ascii_digit()) {
                continue;
            }
            // A process that exits between the listing and the read is normal
            // and says nothing about whether this provider works.
            if let Ok(comm) = read_attribute(&entry.join("comm"))
                && !comm.is_empty()
            {
                scan.executable_names.push(comm);
            }
            if let Ok(cgroup) = read_text(&entry.join("cgroup")) {
                scan.cgroups_readable = true;
                if let Some(identifier) = desktop_id_from_cgroup(&cgroup) {
                    scan.desktop_ids.push(identifier);
                }
            }
        }

        scan.executable_names.sort();
        scan.executable_names.dedup();
        scan.desktop_ids.sort();
        scan.desktop_ids.dedup();
        Ok(scan)
    }
}

impl TriggerProvider for ProcessProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::ProcessRunning
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: PROCESS_POLL_SECONDS,
        }
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        match self.scan() {
            Err(error) => into.mark_unavailable(ProviderKind::ProcessRunning, error.explanation()),
            Ok(scan) => {
                into.running_processes = Some(scan.executable_names);
                // A readable cgroup file with no application scope in it means
                // nothing was launched from a desktop file, which is an empty
                // list. An unreadable one means we cannot answer at all.
                into.running_desktop_ids = scan.cgroups_readable.then_some(scan.desktop_ids);
                into.mark_available(ProviderKind::ProcessRunning);
            }
        }
    }
}

/// Builds a fake `/proc/<pid>` entry.
#[cfg(any(test, feature = "test-support"))]
pub fn write_process(proc_dir: &std::path::Path, pid: u32, comm: &str, cgroup: Option<&str>) {
    let directory = proc_dir.join(pid.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("comm"), format!("{comm}\n")).unwrap();
    if let Some(cgroup) = cgroup {
        std::fs::write(directory.join("cgroup"), format!("{cgroup}\n")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(processes: &[(u32, &str, Option<&str>)]) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc");
        std::fs::create_dir_all(&proc).unwrap();
        for (pid, comm, cgroup) in processes {
            write_process(&proc, *pid, comm, *cgroup);
        }
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn sample(roots: &Roots) -> Observations {
        let mut observations = Observations::at(1_000);
        ProcessProvider::new(roots.clone()).sample(1_000, &mut observations);
        observations
    }

    #[test]
    fn a_scan_reports_the_kernels_own_short_names_deduplicated_and_sorted() {
        let (_directory, roots) = fixture(&[
            (1, "systemd", None),
            (200, "java", None),
            (201, "java", None),
            (300, "gnome-shell", None),
        ]);
        assert_eq!(
            sample(&roots).running_processes,
            Some(vec![
                "gnome-shell".to_string(),
                "java".to_string(),
                "systemd".to_string()
            ])
        );
    }

    #[test]
    fn a_command_line_is_never_read_even_when_it_is_sitting_right_there() {
        let (_directory, roots) = fixture(&[(200, "myapp", None)]);
        std::fs::write(
            roots.proc_path("200/cmdline"),
            b"myapp\0--password\0hunter2\0",
        )
        .unwrap();

        let observations = sample(&roots);
        let names = observations.running_processes.unwrap();
        assert_eq!(names, vec!["myapp".to_string()]);
        assert!(
            !names.iter().any(|name| name.contains("hunter2")),
            "a password on a command line must never enter the process this file runs in"
        );
    }

    #[test]
    fn the_non_process_entries_under_proc_are_not_mistaken_for_processes() {
        let (_directory, roots) = fixture(&[(1, "systemd", None)]);
        std::fs::write(roots.proc_path("stat"), b"cpu 0 0 0 0\n").unwrap();
        std::fs::create_dir_all(roots.proc_path("self")).unwrap();
        std::fs::write(roots.proc_path("self/comm"), b"impostor\n").unwrap();

        assert_eq!(
            sample(&roots).running_processes,
            Some(vec!["systemd".to_string()])
        );
    }

    #[test]
    fn a_desktop_launched_application_is_matchable_by_its_identifier() {
        let (_directory, roots) = fixture(&[(
            200,
            "gnome-builder",
            Some(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Builder-3312.scope",
            ),
        )]);
        assert_eq!(
            sample(&roots).running_desktop_ids,
            Some(vec!["org.gnome.Builder".to_string()])
        );
    }

    #[test]
    fn a_systemd_escaped_hyphen_is_unescaped_so_a_rule_can_be_written_normally() {
        assert_eq!(
            desktop_id_from_cgroup(
                "0::/user.slice/app.slice/app-gnome-org.gnome.Text\\x2dEditor-991.scope"
            ),
            Some("org.gnome.Text-Editor".to_string())
        );
    }

    #[test]
    fn a_flatpak_and_a_snap_scope_both_yield_the_application_identifier() {
        assert_eq!(
            desktop_id_from_cgroup("0::/app.slice/app-flatpak-com.spotify.Client-4001.scope"),
            Some("com.spotify.Client".to_string())
        );
        assert_eq!(
            desktop_id_from_cgroup("0::/app.slice/app-snap-code-77.scope"),
            Some("code".to_string())
        );
    }

    #[test]
    fn a_system_service_has_no_desktop_identifier_and_that_is_not_a_failure() {
        assert_eq!(
            desktop_id_from_cgroup("0::/system.slice/NetworkManager.service"),
            None
        );
        assert_eq!(
            desktop_id_from_cgroup("0::/user.slice/session-2.scope"),
            None
        );
        assert_eq!(desktop_id_from_cgroup(""), None);
    }

    #[test]
    fn a_cgroup_v1_style_line_is_read_the_same_way() {
        assert_eq!(
            desktop_id_from_cgroup(
                "12:name=systemd:/user.slice/app.slice/app-gnome-org.gnome.Nautilus-2200.scope"
            ),
            Some("org.gnome.Nautilus".to_string())
        );
    }

    #[test]
    fn an_unreadable_cgroup_makes_desktop_ids_unknown_not_an_empty_list() {
        // No process carries a cgroup file, which is what a `/proc` without
        // cgroup support looks like.
        let (_directory, roots) = fixture(&[(1, "systemd", None), (200, "java", None)]);
        let observations = sample(&roots);
        assert!(observations.running_processes.is_some());
        assert_eq!(
            observations.running_desktop_ids, None,
            "not knowing which applications are running is not the same as none being"
        );
    }

    #[test]
    fn a_readable_cgroup_with_no_application_scope_is_an_empty_list_not_unknown() {
        let (_directory, roots) = fixture(&[(1, "systemd", Some("0::/system.slice/init.scope"))]);
        assert_eq!(sample(&roots).running_desktop_ids, Some(Vec::new()));
    }

    #[test]
    fn a_missing_proc_reports_unavailable_rather_than_an_idle_machine() {
        let directory = tempfile::tempdir().unwrap();
        let mut observations = Observations::at(1_000);
        ProcessProvider::new(Roots::at(directory.path())).sample(1_000, &mut observations);
        assert_eq!(observations.running_processes, None);
        assert!(
            !observations
                .availability_of(ProviderKind::ProcessRunning)
                .is_available()
        );
    }

    #[test]
    fn a_process_that_exits_mid_scan_does_not_fail_the_whole_sample() {
        let (_directory, roots) = fixture(&[(1, "systemd", None)]);
        // A directory with no `comm`, which is what a process that exited
        // between the listing and the read looks like.
        std::fs::create_dir_all(roots.proc_path("999")).unwrap();
        assert_eq!(
            sample(&roots).running_processes,
            Some(vec!["systemd".to_string()])
        );
    }
}

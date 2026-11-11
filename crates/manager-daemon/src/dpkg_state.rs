//! Whether dpkg's last run finished, read from dpkg's own record.
//!
//! ADR 0014: a machine that lost power mid-install leaves dpkg with packages
//! it did not finish and a journal it did not fold back, and every later
//! `apt-get` refuses to run. The daemon decides that from the two places dpkg
//! writes it down, `status` and `updates/` under its admin directory, and never
//! from the wording of an error message.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use manager_ipc::{
    DpkgPackageStatus, InterruptedPackage, MAX_REPORTED_PACKAGES, PROTOCOL_VERSION,
    PackageSystemState,
};

use crate::DaemonError;

/// dpkg's admin directory on every Debian-derived system.
pub const DPKG_ADMIN_DIR: &str = "/var/lib/dpkg";

/// Reads the state under an admin directory.
///
/// A missing or unreadable `status` file is an error rather than "clean": a
/// daemon that cannot read dpkg's record has no business saying nothing is
/// wrong with it. A missing `updates/` directory holds no journal entries.
pub fn read(admin_dir: &Path) -> Result<PackageSystemState, DaemonError> {
    let status = fs::read_to_string(admin_dir.join("status"))
        .map_err(|error| DaemonError::HostUnreadable(format!("dpkg status file: {error}")))?;
    let (packages, omitted) = unfinished_packages(&status);
    Ok(PackageSystemState {
        protocol_version: PROTOCOL_VERSION,
        journal_pending: journal_pending(&admin_dir.join("updates"))?,
        packages,
        omitted,
    })
}

/// The packages a `status` file records as left mid-operation, in file order,
/// capped at [`MAX_REPORTED_PACKAGES`] with the remainder counted.
pub fn unfinished_packages(status: &str) -> (Vec<InterruptedPackage>, u32) {
    let mut packages = Vec::new();
    let mut omitted = 0_u32;
    for stanza in status.split("\n\n") {
        let mut name = None;
        let mut state = None;
        for line in stanza.lines() {
            // Continuation lines of a multi-line field start with whitespace
            // and never carry either field this reads.
            if let Some(value) = line.strip_prefix("Package:") {
                name = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("Status:") {
                // `want flag status`: the third word is where dpkg got to.
                state = value
                    .split_whitespace()
                    .nth(2)
                    .and_then(DpkgPackageStatus::from_dpkg);
            }
        }
        let (Some(name), Some(state)) = (name, state) else {
            continue;
        };
        if packages.len() < MAX_REPORTED_PACKAGES {
            packages.push(InterruptedPackage {
                package: name.to_string(),
                status: state,
            });
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    (packages, omitted)
}

/// Whether dpkg's journal holds an entry it has not folded back.
///
/// dpkg names journal entries with digits only; anything else in the directory,
/// such as its `tmp.i` scratch file, is not an entry. This is the same test apt
/// makes before it refuses to run.
fn journal_pending(updates: &Path) -> Result<bool, DaemonError> {
    let entries = match fs::read_dir(updates) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(DaemonError::HostUnreadable(format!(
                "dpkg updates directory: {error}"
            )));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            DaemonError::HostUnreadable(format!("dpkg updates directory: {error}"))
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.is_empty() && name.chars().all(|character| character.is_ascii_digit()) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A status file shaped like the real one: stanzas separated by a blank
    /// line, multi-line fields continued with a leading space, and packages at
    /// rest mixed in with the unfinished ones.
    const STATUS: &str = "Package: libc6\n\
Status: install ok installed\n\
Priority: optional\n\
Description: GNU C Library\n \
Contains the standard libraries.\n\
\n\
Package: better-monitor\n\
Status: install ok half-configured\n\
Version: 0.2.8\n\
\n\
Package: better-awake\n\
Status: install reinstreq half-installed\n\
Version: 0.2.8\n\
\n\
Package: old-thing\n\
Status: deinstall ok config-files\n\
\n\
Package: tzdata\n\
Status: install ok unpacked\n\
\n\
Package: man-db\n\
Status: install ok triggers-pending\n\
Triggers-Pending: /usr/share/man\n\
\n\
Package: initramfs-tools\n\
Status: install ok triggers-awaited\n";

    fn scratch(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "better-os-dpkg-state-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn every_unfinished_status_is_read_and_packages_at_rest_are_not() {
        let (packages, omitted) = unfinished_packages(STATUS);
        assert_eq!(omitted, 0);
        assert_eq!(
            packages,
            vec![
                InterruptedPackage {
                    package: "better-monitor".to_string(),
                    status: DpkgPackageStatus::HalfConfigured,
                },
                InterruptedPackage {
                    package: "better-awake".to_string(),
                    status: DpkgPackageStatus::HalfInstalled,
                },
                InterruptedPackage {
                    package: "tzdata".to_string(),
                    status: DpkgPackageStatus::Unpacked,
                },
                InterruptedPackage {
                    package: "man-db".to_string(),
                    status: DpkgPackageStatus::TriggersPending,
                },
                InterruptedPackage {
                    package: "initramfs-tools".to_string(),
                    status: DpkgPackageStatus::TriggersAwaited,
                },
            ]
        );
    }

    #[test]
    fn a_status_file_with_everything_installed_reports_nothing() {
        let (packages, omitted) = unfinished_packages(
            "Package: libc6\nStatus: install ok installed\n\n\
             Package: bash\nStatus: install ok installed\n",
        );
        assert!(packages.is_empty());
        assert_eq!(omitted, 0);
    }

    #[test]
    fn a_long_list_is_capped_and_the_rest_are_counted() {
        let status = (0..MAX_REPORTED_PACKAGES + 5)
            .map(|index| format!("Package: package{index}\nStatus: install ok unpacked\n"))
            .collect::<Vec<_>>()
            .join("\n");
        let (packages, omitted) = unfinished_packages(&status);
        assert_eq!(packages.len(), MAX_REPORTED_PACKAGES);
        assert_eq!(omitted, 5);
    }

    #[test]
    fn a_journal_entry_in_updates_means_dpkg_was_interrupted() {
        let root = scratch("journal");
        fs::write(
            root.join("status"),
            "Package: libc6\nStatus: install ok installed\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("updates")).unwrap();
        // dpkg's scratch file is not a journal entry.
        fs::write(root.join("updates/tmp.i"), "").unwrap();
        assert!(!read(&root).unwrap().needs_repair());

        fs::write(root.join("updates/0003"), "Package: tzdata\n").unwrap();
        let state = read(&root).unwrap();
        assert!(state.journal_pending);
        assert!(state.packages.is_empty());
        assert!(state.needs_repair());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_updates_directory_holds_no_journal() {
        let root = scratch("no-updates");
        fs::write(root.join("status"), STATUS).unwrap();
        let state = read(&root).unwrap();
        assert!(!state.journal_pending);
        assert_eq!(state.packages.len(), 5);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_status_file_is_an_error_not_a_clean_bill() {
        let root = scratch("no-status");
        assert!(matches!(read(&root), Err(DaemonError::HostUnreadable(_))));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn what_the_reader_produces_is_a_valid_wire_document() {
        let root = scratch("wire");
        fs::write(root.join("status"), STATUS).unwrap();
        read(&root).unwrap().validate().unwrap();
        let _ = fs::remove_dir_all(&root);
    }
}

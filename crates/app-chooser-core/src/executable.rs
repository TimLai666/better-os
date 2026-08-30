//! Choose Executable: a separate mode that refuses more often than it answers.
//!
//! Some developer tools genuinely need a path to a binary. That is a different
//! question from "what should open this file", and it has an honest answer far
//! less often than a chooser full of applications suggests. A Flatpak, a Snap,
//! an AppImage, a wrapper script, or a D-Bus-activated application has no one
//! executable that behaves like the application, and an entry whose `Exec` line
//! carries its own arguments does not behave the same way when the bare program
//! is run without them.
//!
//! So this module returns a path only when running that path is actually
//! equivalent to launching the application, and otherwise returns the reason it
//! will not, for the surface to show. It never invents one.

use std::path::{Path, PathBuf};

use app_catalog_core::exec::ArgumentPiece;
use app_catalog_core::{ApplicationRecord, ExecutableStatus, NoCanonicalExecutable};

/// Why no executable path is being offered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableWarning {
    /// The application is packaged in a way that has no single executable.
    NoSingleExecutable { reason: NoCanonicalExecutable },
    /// The application is started over D-Bus, so there is no command to run.
    DBusActivated,
    /// The entry names a program that is not installed on this host.
    ProgramNotFound { program: String },
    /// The entry has no `Exec` line at all.
    NoExecLine,
    /// Running the bare program would drop arguments the entry depends on.
    ComplexArguments {
        program: String,
        /// The arguments the entry supplies that a bare path would lose.
        dropped: Vec<String>,
    },
    /// A browsed path does not exist.
    NotFound { path: PathBuf },
    /// A browsed path exists but is not an executable file.
    NotExecutable { path: PathBuf },
}

impl ExecutableWarning {
    /// Whether the reason is about how the application is packaged rather than
    /// about this host. A packaging reason will never resolve, so the surface
    /// should offer browsing instead of suggesting the user install something.
    pub fn is_packaging(&self) -> bool {
        matches!(
            self,
            Self::NoSingleExecutable { .. } | Self::DBusActivated | Self::ComplexArguments { .. }
        )
    }
}

/// The answer to "give me a path for this application".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableResolution {
    /// Running this path is equivalent to launching the application.
    Resolved(PathBuf),
    /// No path is being offered, and this is why.
    Refused(ExecutableWarning),
}

impl ExecutableResolution {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Resolved(path) => Some(path.as_path()),
            Self::Refused(_) => None,
        }
    }

    pub fn warning(&self) -> Option<&ExecutableWarning> {
        match self {
            Self::Refused(warning) => Some(warning),
            Self::Resolved(_) => None,
        }
    }
}

/// Resolves an application to an executable path, or explains why it cannot.
///
/// The checks run in the order the reasons matter: packaging first, because a
/// Flatpak's wrapper resolving on `PATH` would otherwise look like a good
/// answer; then D-Bus activation; then whether the program exists; then whether
/// the entry's own arguments make the bare program a different thing.
pub fn resolve_executable(record: &ApplicationRecord) -> ExecutableResolution {
    if let ExecutableStatus::NotApplicable { reason } = record.executable {
        return ExecutableResolution::Refused(match reason {
            NoCanonicalExecutable::DBusActivated => ExecutableWarning::DBusActivated,
            reason => ExecutableWarning::NoSingleExecutable { reason },
        });
    }
    if record.capabilities.dbus_activatable {
        return ExecutableResolution::Refused(ExecutableWarning::DBusActivated);
    }
    let Some(exec) = &record.exec else {
        return ExecutableResolution::Refused(ExecutableWarning::NoExecLine);
    };
    let dropped = supplied_arguments(record);
    if !dropped.is_empty() {
        return ExecutableResolution::Refused(ExecutableWarning::ComplexArguments {
            program: exec.program().to_string(),
            dropped,
        });
    }
    match &record.executable {
        ExecutableStatus::Resolved(path) => ExecutableResolution::Resolved(path.clone()),
        ExecutableStatus::Unresolved { program } => {
            ExecutableResolution::Refused(ExecutableWarning::ProgramNotFound {
                program: program.clone(),
            })
        }
        ExecutableStatus::NotApplicable { .. } => unreachable!("handled above"),
    }
}

/// The arguments the entry supplies beyond the program itself, ignoring field
/// codes, which stand in for the launch targets rather than for behavior.
fn supplied_arguments(record: &ApplicationRecord) -> Vec<String> {
    let Some(exec) = &record.exec else {
        return Vec::new();
    };
    exec.arguments()
        .iter()
        .skip(1)
        .filter_map(|argument| {
            let rendered: String = argument
                .iter()
                .filter_map(|piece| match piece {
                    ArgumentPiece::Literal(text) => Some(text.as_str()),
                    ArgumentPiece::Field(_) => None,
                })
                .collect();
            // An argument made only of field codes stands in for the launch
            // targets, so it is not behavior the bare program would lose.
            (!rendered.is_empty()).then_some(rendered)
        })
        .collect()
}

/// The directories the fallback file picker browses, user-local first. Only
/// directories that exist are returned, so the picker never shows a dead root.
pub fn browse_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/bin"));
        roots.push(home.join("bin"));
    }
    roots.push(PathBuf::from("/usr/local/bin"));
    roots.push(PathBuf::from("/usr/bin"));
    roots.retain(|root| root.is_dir());
    roots
}

/// The executable files directly inside `directory`, sorted by name. Not
/// recursive: a picker over `/usr/bin` is a list, not a crawl.
pub fn list_executables(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_executable_file(path))
        .collect();
    paths.sort();
    paths
}

/// Accepts a path the user browsed to, after checking it is really runnable.
pub fn accept_executable_path(path: &Path) -> Result<PathBuf, ExecutableWarning> {
    if !path.exists() {
        return Err(ExecutableWarning::NotFound {
            path: path.to_path_buf(),
        });
    }
    if !is_executable_file(path) {
        return Err(ExecutableWarning::NotExecutable {
            path: path.to_path_buf(),
        });
    }
    Ok(path.to_path_buf())
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // `metadata` follows symlinks, which is what a picker over `/usr/bin`
    // needs: most of what is there is a link to a real binary.
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{DesktopFile, DesktopId, EntryScope, ExecutableProbe};

    struct AlwaysResolves;

    impl ExecutableProbe for AlwaysResolves {
        fn resolve(&self, program: &str) -> Option<PathBuf> {
            Some(PathBuf::from("/usr/bin").join(program))
        }
    }

    struct NeverResolves;

    impl ExecutableProbe for NeverResolves {
        fn resolve(&self, _program: &str) -> Option<PathBuf> {
            None
        }
    }

    fn record(desktop_id: &str, body: &str, probe: &dyn ExecutableProbe) -> ApplicationRecord {
        let file = DesktopFile::parse(body).expect("valid entry");
        ApplicationRecord::from_desktop_file(
            DesktopId::new(desktop_id).expect("valid id"),
            PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
            EntryScope::System,
            &file,
            probe,
        )
        .expect("valid record")
    }

    #[test]
    fn a_plain_entry_whose_program_exists_resolves() {
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\n",
            &AlwaysResolves,
        );
        assert_eq!(
            resolve_executable(&record),
            ExecutableResolution::Resolved(PathBuf::from("/usr/bin/editor"))
        );
    }

    #[test]
    fn a_flatpak_entry_is_refused_rather_than_given_a_wrapper_path() {
        let record = record(
            "org.example.App.desktop",
            "[Desktop Entry]\nType=Application\nName=App\nExec=/usr/bin/flatpak run --branch=stable org.example.App\n",
            &AlwaysResolves,
        );
        assert_eq!(
            resolve_executable(&record),
            ExecutableResolution::Refused(ExecutableWarning::NoSingleExecutable {
                reason: NoCanonicalExecutable::Flatpak
            })
        );
        assert!(resolve_executable(&record).path().is_none());
    }

    #[test]
    fn a_snap_entry_is_refused() {
        let record = record(
            "snap-app.desktop",
            "[Desktop Entry]\nType=Application\nName=App\nExec=/snap/bin/app %U\n",
            &AlwaysResolves,
        );
        assert!(matches!(
            resolve_executable(&record),
            ExecutableResolution::Refused(ExecutableWarning::NoSingleExecutable {
                reason: NoCanonicalExecutable::Snap
            })
        ));
    }

    #[test]
    fn a_wrapper_entry_is_refused() {
        let record = record(
            "wrapped.desktop",
            "[Desktop Entry]\nType=Application\nName=App\nExec=sh -c \"app --flag\"\n",
            &AlwaysResolves,
        );
        assert!(matches!(
            resolve_executable(&record),
            ExecutableResolution::Refused(ExecutableWarning::NoSingleExecutable {
                reason: NoCanonicalExecutable::Wrapper
            })
        ));
    }

    #[test]
    fn a_dbus_activated_entry_is_refused_even_though_its_exec_line_resolves() {
        let record = record(
            "org.gnome.Nautilus.desktop",
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus %U\nDBusActivatable=true\n",
            &AlwaysResolves,
        );
        assert_eq!(
            resolve_executable(&record),
            ExecutableResolution::Refused(ExecutableWarning::DBusActivated)
        );
    }

    #[test]
    fn an_entry_with_its_own_arguments_is_refused_instead_of_losing_them() {
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor --new-window %F\n",
            &AlwaysResolves,
        );
        let ExecutableResolution::Refused(ExecutableWarning::ComplexArguments { program, dropped }) =
            resolve_executable(&record)
        else {
            panic!("expected a refusal");
        };
        assert_eq!(program, "editor");
        assert_eq!(dropped, vec!["--new-window".to_string()]);
    }

    #[test]
    fn a_missing_program_is_reported_as_a_host_problem_not_a_packaging_one() {
        let record = record(
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\n",
            &NeverResolves,
        );
        let ExecutableResolution::Refused(warning) = resolve_executable(&record) else {
            panic!("expected a refusal");
        };
        assert_eq!(
            warning,
            ExecutableWarning::ProgramNotFound {
                program: "editor".to_string()
            }
        );
        assert!(!warning.is_packaging());
    }

    #[test]
    fn packaging_refusals_are_marked_as_such() {
        for warning in [
            ExecutableWarning::DBusActivated,
            ExecutableWarning::NoSingleExecutable {
                reason: NoCanonicalExecutable::Snap,
            },
            ExecutableWarning::ComplexArguments {
                program: "x".into(),
                dropped: vec!["--y".into()],
            },
        ] {
            assert!(warning.is_packaging(), "{warning:?}");
        }
    }

    #[test]
    fn browsing_accepts_an_executable_file_and_refuses_the_rest() {
        let dir = tempfile::tempdir().expect("temp dir");
        let runnable = dir.path().join("runme");
        std::fs::write(&runnable, "#!/bin/sh\n").expect("write");
        let plain = dir.path().join("data.txt");
        std::fs::write(&plain, "not a program").expect("write");
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&runnable).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&runnable, permissions).expect("chmod");
        }

        assert_eq!(accept_executable_path(&runnable), Ok(runnable.clone()));
        assert_eq!(
            accept_executable_path(&plain),
            Err(ExecutableWarning::NotExecutable { path: plain })
        );
        let missing = dir.path().join("nope");
        assert_eq!(
            accept_executable_path(&missing),
            Err(ExecutableWarning::NotFound {
                path: missing.clone()
            })
        );
        assert_eq!(list_executables(dir.path()), vec![runnable]);
        assert!(list_executables(&missing).is_empty());
    }
}

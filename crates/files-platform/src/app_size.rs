//! How much space an application takes, from what the system already records
//! (ticket 128). Everything here is a read: dpkg's status file and
//! `dpkg-query -S`, `flatpak info --show-size`, a snap's revision file, and
//! `stat` of a program. Nothing is installed, changed, or asked of a
//! privileged service.
//!
//! Where an application came from decides which record answers:
//!
//! - a Flatpak (its desktop entry resolves into a `flatpak/app/<id>/`
//!   deployment): the installed size `flatpak` reports. The runtime is shared
//!   with other applications and is not added; its name is kept so the panel
//!   can say so.
//! - a Snap (its entry is in snapd's desktop directory): the size of the
//!   current revision's squashfs file.
//! - an AppImage, or a program in the person's home: the program file's size.
//! - anything else: dpkg's `Installed-Size` of the package that owns the
//!   desktop entry, the package alone. dpkg records which packages a package
//!   depends on, not which of them nothing else needs, and apt's "installed
//!   automatically" marks are not that either, so no dependency is added.
//!
//! A size that cannot be found is [`ApplicationSize::Unknown`] with the
//! reason, never a number made up to fill the cell.
//!
//! The person's data is counted only where it lives for certain: a Flatpak's
//! `~/.var/app/<id>` and a snap's `~/snap/<name>`. A native application's
//! `~/.config` folder has no recorded name, and guessing one would count
//! another program's files, so none is shown.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use app_catalog_core::{EntryScope, SourceKind};
use files_core::{ApplicationSize, ApplicationSizeSource, ApplicationSizeUnknown};

/// What the sizer is told about one application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationQuery {
    /// The desktop entry the catalog read.
    pub entry_path: PathBuf,
    pub kind: SourceKind,
    pub scope: EntryScope,
    /// The program the entry runs, when the catalog resolved one.
    pub executable: Option<PathBuf>,
    /// The entry's `Exec` program as written, for an AppImage the catalog
    /// does not resolve.
    pub program: Option<String>,
}

/// Runs a read-only host command and hands back what it printed.
///
/// A trait so tests answer with fixture output and never reach the host's
/// dpkg or flatpak. The runner that starts real programs lives in the window
/// (`files_gui::app_sizes`): this crate is on the file-operation path and
/// spawns nothing (`files-operations/tests/no_shell_strings.rs`).
pub trait HostCommands: Send + Sync {
    /// Standard output, or `None` when the program could not be started.
    /// A command that exits non-zero still has its output returned:
    /// `dpkg-query -S` exits 1 when any one path is unowned and still names
    /// the owners of the rest.
    fn run(&self, program: &str, args: &[OsString]) -> Option<String>;
}

/// Where the records are. [`SizeRoots::host`] is the machine's; a test
/// points every one of them into a temporary directory.
#[derive(Clone, Debug)]
pub struct SizeRoots {
    pub dpkg_status: PathBuf,
    /// Where snaps are mounted: `/snap/<name>/current` names the revision.
    pub snap_mounts: PathBuf,
    /// Where the revision files are: `<name>_<revision>.snap`.
    pub snap_files: PathBuf,
    /// Where snapd puts its desktop entries.
    pub snap_desktop: PathBuf,
    pub home: Option<PathBuf>,
}

impl SizeRoots {
    pub fn host(home: Option<PathBuf>) -> Self {
        Self {
            dpkg_status: PathBuf::from("/var/lib/dpkg/status"),
            snap_mounts: PathBuf::from("/snap"),
            snap_files: PathBuf::from("/var/lib/snapd/snaps"),
            snap_desktop: PathBuf::from("/var/lib/snapd/desktop/applications"),
            home,
        }
    }
}

/// At most this many files are looked at for one application's data folder.
/// A folder larger than that shows no data figure rather than part of one.
pub const DATA_ITEMS: u64 = 200_000;

/// Which record answers for one application.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Origin {
    Flatpak {
        app_id: String,
        user: Option<bool>,
        deploy: Option<PathBuf>,
    },
    Snap {
        name: String,
    },
    File {
        path: PathBuf,
    },
    Package,
}

/// Sizes every application in `queries`, in the same order.
///
/// dpkg is asked once for all of them, so a hundred native applications
/// cost one `dpkg-query` and one reading of the status file.
pub fn application_sizes(
    queries: &[ApplicationQuery],
    roots: &SizeRoots,
    commands: &dyn HostCommands,
) -> Vec<ApplicationSize> {
    let origins: Vec<Origin> = queries.iter().map(|query| origin(query, roots)).collect();
    let package_paths: Vec<PathBuf> = queries
        .iter()
        .zip(&origins)
        .filter(|(_, origin)| **origin == Origin::Package)
        .map(|(query, _)| query.entry_path.clone())
        .collect();
    let dpkg = if package_paths.is_empty() {
        None
    } else {
        dpkg_records(&package_paths, roots, commands)
    };
    queries
        .iter()
        .zip(origins)
        .map(|(query, origin)| match origin {
            Origin::Flatpak {
                app_id,
                user,
                deploy,
            } => flatpak_size(&app_id, user, deploy.as_deref(), roots, commands),
            Origin::Snap { name } => snap_size(&name, roots),
            Origin::File { path } => file_size(&path),
            Origin::Package => match &dpkg {
                None => ApplicationSize::Unknown(ApplicationSizeUnknown::DpkgUnanswered),
                Some(records) => records.size_of(&query.entry_path),
            },
        })
        .collect()
}

fn origin(query: &ApplicationQuery, roots: &SizeRoots) -> Origin {
    let canonical = fs::canonicalize(&query.entry_path).ok();
    if let Some(found) = canonical.as_deref().and_then(flatpak_deployment) {
        let user = roots
            .home
            .as_deref()
            .map(|home| found.deploy.starts_with(home));
        return Origin::Flatpak {
            app_id: found.app_id,
            user,
            deploy: Some(found.deploy),
        };
    }
    // An entry that runs `flatpak` but whose deployment could not be
    // followed. The catalog's own Flatpak mark is not enough on its own: a
    // distribution package's entry can carry `X-Flatpak-RenamedFrom` (GNOME
    // Text Editor and Evolution do) and is still dpkg's.
    let runs_flatpak = query
        .program
        .as_deref()
        .is_some_and(|program| program.rsplit('/').next() == Some("flatpak"));
    if runs_flatpak && let Some(app_id) = desktop_stem(&query.entry_path) {
        return Origin::Flatpak {
            app_id,
            user: None,
            deploy: None,
        };
    }
    if (query.kind == SourceKind::Snap || query.entry_path.starts_with(&roots.snap_desktop))
        && let Some(name) = snap_instance(&query.entry_path)
    {
        return Origin::Snap { name };
    }
    let program = query.executable.clone().or_else(|| {
        query
            .program
            .as_deref()
            .filter(|program| program.starts_with('/'))
            .map(PathBuf::from)
    });
    if let Some(path) = program {
        let appimage = query.kind == SourceKind::AppImage
            || path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("appimage"));
        let in_home = roots
            .home
            .as_deref()
            .is_some_and(|home| path.starts_with(home));
        if appimage || in_home {
            return Origin::File { path };
        }
    }
    Origin::Package
}

struct Deployment {
    app_id: String,
    /// The deployed commit's directory: `…/app/<id>/<arch>/<branch>/<commit>`.
    deploy: PathBuf,
}

/// The Flatpak an exported desktop entry belongs to, read from where its
/// symbolic link leads: `…/flatpak/app/<id>/<arch>/<branch>/<commit>/export/…`.
fn flatpak_deployment(canonical: &Path) -> Option<Deployment> {
    let parts: Vec<_> = canonical.components().collect();
    let at = parts
        .windows(2)
        .position(|pair| pair[0].as_os_str() == "flatpak" && pair[1].as_os_str() == "app")?;
    let app_id = parts.get(at + 2)?.as_os_str().to_str()?.to_string();
    // id, arch, branch, commit, then "export".
    if parts.get(at + 6)?.as_os_str() != "export" {
        return None;
    }
    let deploy: PathBuf = parts[..at + 6].iter().collect();
    Some(Deployment { app_id, deploy })
}

fn desktop_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.strip_suffix(".desktop").unwrap_or(name).to_string())
}

/// The snap instance a snapd desktop entry is named after:
/// `<instance>_<app>.desktop`, where a parallel instance `name_key` is
/// written `name+key`.
fn snap_instance(path: &Path) -> Option<String> {
    let stem = desktop_stem(path)?;
    let (instance, _) = stem.split_once('_')?;
    if instance.is_empty() {
        return None;
    }
    Some(instance.replace('+', "_"))
}

fn flatpak_size(
    app_id: &str,
    user: Option<bool>,
    deploy: Option<&Path>,
    roots: &SizeRoots,
    commands: &dyn HostCommands,
) -> ApplicationSize {
    let mut args: Vec<OsString> = vec!["info".into(), "--show-size".into()];
    match user {
        Some(true) => args.push("--user".into()),
        Some(false) => args.push("--system".into()),
        None => {}
    }
    args.push(app_id.into());
    let Some(bytes) = commands
        .run("flatpak", &args)
        .as_deref()
        .and_then(parse_flatpak_size)
    else {
        return ApplicationSize::Unknown(ApplicationSizeUnknown::FlatpakUnanswered {
            app_id: app_id.to_string(),
        });
    };
    let runtime = deploy
        .and_then(|deploy| fs::read_to_string(deploy.join("metadata")).ok())
        .and_then(|metadata| parse_flatpak_runtime(&metadata));
    let data = roots
        .home
        .as_deref()
        .and_then(|home| data_size(&home.join(".var/app").join(app_id)));
    ApplicationSize::Known {
        bytes,
        source: ApplicationSizeSource::Flatpak {
            app_id: app_id.to_string(),
            runtime,
        },
        data,
    }
}

/// `flatpak info --show-size` prints the installed size in bytes on a line
/// of its own.
pub fn parse_flatpak_size(output: &str) -> Option<u64> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .parse()
        .ok()
}

/// The runtime a deployed Flatpak's `metadata` names: `runtime=` in its
/// `[Application]` group, without the architecture and branch.
pub fn parse_flatpak_runtime(metadata: &str) -> Option<String> {
    let mut in_application = false;
    for line in metadata.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_application = line == "[Application]";
            continue;
        }
        if in_application && let Some(value) = line.strip_prefix("runtime=") {
            let name = value.split('/').next().unwrap_or(value).trim();
            return (!name.is_empty()).then(|| name.to_string());
        }
    }
    None
}

fn snap_size(name: &str, roots: &SizeRoots) -> ApplicationSize {
    let missing = || {
        ApplicationSize::Unknown(ApplicationSizeUnknown::SnapMissing {
            name: name.to_string(),
        })
    };
    let Ok(revision) = fs::read_link(roots.snap_mounts.join(name).join("current")) else {
        return missing();
    };
    let Some(revision) = revision.to_str().map(str::to_string) else {
        return missing();
    };
    let file = roots.snap_files.join(format!("{name}_{revision}.snap"));
    let Ok(metadata) = fs::metadata(&file) else {
        return missing();
    };
    let data = roots
        .home
        .as_deref()
        .and_then(|home| data_size(&home.join("snap").join(name)));
    ApplicationSize::Known {
        bytes: metadata.len(),
        source: ApplicationSizeSource::Snap {
            name: name.to_string(),
            revision,
        },
        data,
    }
}

fn file_size(path: &Path) -> ApplicationSize {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => ApplicationSize::Known {
            bytes: metadata.len(),
            source: ApplicationSizeSource::File {
                path: path.to_path_buf(),
            },
            data: None,
        },
        _ => ApplicationSize::Unknown(ApplicationSizeUnknown::ProgramMissing {
            path: path.to_path_buf(),
        }),
    }
}

/// The bytes in a data folder, or `None` when there is none or it holds more
/// than [`DATA_ITEMS`] entries. Symbolic links are not followed.
fn data_size(folder: &Path) -> Option<u64> {
    if !fs::symlink_metadata(folder).ok()?.is_dir() {
        return None;
    }
    let mut total = 0u64;
    let mut seen = 0u64;
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for item in read.flatten() {
            seen += 1;
            if seen > DATA_ITEMS {
                return None;
            }
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(item.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Some(total)
}

/// What dpkg says about the desktop entries asked about.
struct DpkgRecords {
    owners: HashMap<PathBuf, Vec<String>>,
    sizes: HashMap<String, Option<u64>>,
}

impl DpkgRecords {
    fn size_of(&self, entry: &Path) -> ApplicationSize {
        let Some(owners) = self.owners.get(entry) else {
            return ApplicationSize::Unknown(ApplicationSizeUnknown::NoOwner);
        };
        let [package] = owners.as_slice() else {
            return ApplicationSize::Unknown(ApplicationSizeUnknown::SeveralPackages);
        };
        match self.sizes.get(package) {
            Some(Some(kib)) => ApplicationSize::Known {
                bytes: kib.saturating_mul(1024),
                source: ApplicationSizeSource::DebianPackage {
                    package: package.clone(),
                },
                data: None,
            },
            Some(None) => ApplicationSize::Unknown(ApplicationSizeUnknown::NotRecorded {
                package: package.clone(),
            }),
            // dpkg-query named an owner the status file does not have as
            // installed: it is half removed, or the file changed between
            // the two readings. Neither is a size.
            None => ApplicationSize::Unknown(ApplicationSizeUnknown::NoOwner),
        }
    }
}

fn dpkg_records(
    paths: &[PathBuf],
    roots: &SizeRoots,
    commands: &dyn HostCommands,
) -> Option<DpkgRecords> {
    let mut args: Vec<OsString> = vec!["-S".into()];
    args.extend(paths.iter().map(|path| path.as_os_str().to_os_string()));
    let output = commands.run("dpkg-query", &args)?;
    let status = fs::read_to_string(&roots.dpkg_status).ok()?;
    Some(DpkgRecords {
        owners: parse_dpkg_search(&output),
        sizes: parse_dpkg_status(&status),
    })
}

/// `dpkg-query -S` output: `package[, package…]: path` per owned path. A
/// package may carry its architecture (`libfoo:amd64`), which is dropped,
/// because the status file names the package without it. Diversion lines
/// and error lines are skipped.
pub fn parse_dpkg_search(output: &str) -> HashMap<PathBuf, Vec<String>> {
    let mut owners: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for line in output.lines() {
        if line.starts_with("diversion by ") {
            continue;
        }
        // The path is absolute, so the separator is the first ": /".
        let Some(at) = line.find(": /") else {
            continue;
        };
        let (packages, path) = (&line[..at], &line[at + 2..]);
        let names = packages
            .split(',')
            .map(|name| name.trim())
            .map(|name| name.split(':').next().unwrap_or(name).to_string())
            .filter(|name| !name.is_empty());
        let entry = owners.entry(PathBuf::from(path)).or_default();
        for name in names {
            if !entry.contains(&name) {
                entry.push(name);
            }
        }
    }
    owners
}

/// Each installed package's `Installed-Size` (in KiB, as dpkg records it),
/// from the status file. A package not in the `installed` state is left
/// out; one that is installed and records no size maps to `None`.
pub fn parse_dpkg_status(status: &str) -> HashMap<String, Option<u64>> {
    let mut sizes = HashMap::new();
    for stanza in status.split("\n\n") {
        let mut package = None;
        let mut installed = false;
        let mut size = None;
        for line in stanza.lines() {
            if let Some(value) = line.strip_prefix("Package:") {
                package = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("Status:") {
                installed = value.split_whitespace().last() == Some("installed");
            } else if let Some(value) = line.strip_prefix("Installed-Size:") {
                size = value.trim().parse::<u64>().ok();
            }
        }
        if let (Some(package), true) = (package, installed) {
            // Two architectures of one package each have a stanza; the
            // larger is the one a desktop entry is most likely to belong to
            // and the other is at most as large, so neither understates.
            let slot: &mut Option<u64> = sizes.entry(package).or_insert(None);
            *slot = match (*slot, size) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers from fixtures and records what it was asked.
    #[derive(Default)]
    struct Fixture {
        dpkg: Option<String>,
        flatpak: HashMap<String, String>,
        asked: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl HostCommands for Fixture {
        fn run(&self, program: &str, args: &[OsString]) -> Option<String> {
            let args: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            self.asked
                .lock()
                .unwrap()
                .push((program.to_string(), args.clone()));
            match program {
                "dpkg-query" => self.dpkg.clone(),
                "flatpak" => self.flatpak.get(args.last()?).cloned(),
                _ => None,
            }
        }
    }

    const STATUS: &str = "Package: nautilus\nStatus: install ok installed\nPriority: optional\nInstalled-Size: 1605\nVersion: 1:46.2\n\nPackage: gone\nStatus: deinstall ok config-files\nInstalled-Size: 99\n\nPackage: nosize\nStatus: install ok installed\nVersion: 1\n\nPackage: libdual\nStatus: install ok installed\nArchitecture: amd64\nInstalled-Size: 300\n\nPackage: libdual\nStatus: install ok installed\nArchitecture: i386\nInstalled-Size: 280\n";

    fn roots(dir: &Path) -> SizeRoots {
        fs::write(dir.join("status"), STATUS).unwrap();
        SizeRoots {
            dpkg_status: dir.join("status"),
            snap_mounts: dir.join("snap"),
            snap_files: dir.join("snaps"),
            snap_desktop: dir.join("snapd-desktop"),
            home: Some(dir.join("home")),
        }
    }

    fn query(path: PathBuf, kind: SourceKind) -> ApplicationQuery {
        ApplicationQuery {
            entry_path: path,
            kind,
            scope: EntryScope::System,
            executable: None,
            program: None,
        }
    }

    #[test]
    fn the_status_file_gives_installed_packages_their_size() {
        let sizes = parse_dpkg_status(STATUS);
        assert_eq!(sizes["nautilus"], Some(1605));
        assert_eq!(sizes["nosize"], None);
        assert_eq!(sizes["libdual"], Some(300));
        assert!(!sizes.contains_key("gone"));
    }

    #[test]
    fn dpkg_search_output_names_each_paths_owners() {
        let owners = parse_dpkg_search(
            "nautilus: /usr/share/applications/org.gnome.Nautilus.desktop\n\
             diversion by foo from: /usr/share/applications/x.desktop\n\
             a:amd64, b: /usr/share/applications/shared.desktop\n\
             dpkg-query: no path found matching pattern /nowhere.desktop\n",
        );
        assert_eq!(
            owners[Path::new("/usr/share/applications/org.gnome.Nautilus.desktop")],
            vec!["nautilus".to_string()]
        );
        assert_eq!(
            owners[Path::new("/usr/share/applications/shared.desktop")],
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(owners.len(), 2);
    }

    #[test]
    fn a_debian_package_is_the_package_alone_and_dpkg_is_asked_once() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let commands = Fixture {
            dpkg: Some(
                "nautilus: /usr/share/applications/org.gnome.Nautilus.desktop\n\
                 nosize: /usr/share/applications/nosize.desktop\n\
                 a, b: /usr/share/applications/shared.desktop\n"
                    .to_string(),
            ),
            ..Fixture::default()
        };
        let queries = ["org.gnome.Nautilus", "nosize", "shared", "local"].map(|name| {
            query(
                PathBuf::from(format!("/usr/share/applications/{name}.desktop")),
                SourceKind::Native,
            )
        });
        let sizes = application_sizes(&queries, &roots, &commands);
        assert_eq!(
            sizes[0],
            ApplicationSize::Known {
                bytes: 1605 * 1024,
                source: ApplicationSizeSource::DebianPackage {
                    package: "nautilus".to_string()
                },
                data: None,
            }
        );
        assert_eq!(
            sizes[1],
            ApplicationSize::Unknown(ApplicationSizeUnknown::NotRecorded {
                package: "nosize".to_string()
            })
        );
        assert_eq!(
            sizes[2],
            ApplicationSize::Unknown(ApplicationSizeUnknown::SeveralPackages)
        );
        assert_eq!(
            sizes[3],
            ApplicationSize::Unknown(ApplicationSizeUnknown::NoOwner)
        );
        let asked = commands.asked.lock().unwrap();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(asked[0].1.len(), 5);
    }

    #[test]
    fn dpkg_that_cannot_be_asked_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let sizes = application_sizes(
            &[query(
                PathBuf::from("/usr/share/applications/a.desktop"),
                SourceKind::Native,
            )],
            &roots,
            &Fixture::default(),
        );
        assert_eq!(
            sizes[0],
            ApplicationSize::Unknown(ApplicationSizeUnknown::DpkgUnanswered)
        );
    }

    #[test]
    fn a_flatpak_is_its_reported_size_with_the_runtime_named_and_its_data_apart() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let deploy = dir
            .path()
            .join("var/lib/flatpak/app/org.example.App/x86_64/stable/abc123");
        fs::create_dir_all(deploy.join("export/share/applications")).unwrap();
        fs::write(
            deploy.join("metadata"),
            "[Application]\nname=org.example.App\nruntime=org.gnome.Platform/x86_64/46\nsdk=org.gnome.Sdk/x86_64/46\n",
        )
        .unwrap();
        let exported = deploy.join("export/share/applications/org.example.App.desktop");
        fs::write(&exported, "an exported entry").unwrap();
        let exports = dir.path().join("exports");
        fs::create_dir_all(&exports).unwrap();
        let entry = exports.join("org.example.App.desktop");
        std::os::unix::fs::symlink(&exported, &entry).unwrap();
        let data = dir.path().join("home/.var/app/org.example.App/config");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("settings"), vec![0u8; 4000]).unwrap();
        let commands = Fixture {
            flatpak: HashMap::from([("org.example.App".to_string(), "10516992\n".to_string())]),
            ..Fixture::default()
        };
        let sizes = application_sizes(&[query(entry, SourceKind::Native)], &roots, &commands);
        assert_eq!(
            sizes[0],
            ApplicationSize::Known {
                bytes: 10_516_992,
                source: ApplicationSizeSource::Flatpak {
                    app_id: "org.example.App".to_string(),
                    runtime: Some("org.gnome.Platform".to_string()),
                },
                data: Some(4000),
            }
        );
        let asked = commands.asked.lock().unwrap();
        assert_eq!(
            asked[0].1,
            ["info", "--show-size", "--system", "org.example.App"]
        );
    }

    #[test]
    fn a_flatpak_that_does_not_answer_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let mut gone = query(
            PathBuf::from("/nowhere/org.example.Gone.desktop"),
            SourceKind::Flatpak,
        );
        gone.program = Some("/usr/bin/flatpak".to_string());
        let sizes = application_sizes(&[gone], &roots, &Fixture::default());
        assert_eq!(
            sizes[0],
            ApplicationSize::Unknown(ApplicationSizeUnknown::FlatpakUnanswered {
                app_id: "org.example.Gone".to_string()
            })
        );
    }

    #[test]
    fn a_package_entry_with_a_flatpak_key_is_still_the_package() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let commands = Fixture {
            dpkg: Some(
                "nautilus: /usr/share/applications/org.gnome.Nautilus.desktop\n".to_string(),
            ),
            ..Fixture::default()
        };
        // The catalog marks an entry with `X-Flatpak-RenamedFrom` as a
        // Flatpak; it runs a native program.
        let mut entry = query(
            PathBuf::from("/usr/share/applications/org.gnome.Nautilus.desktop"),
            SourceKind::Flatpak,
        );
        entry.program = Some("nautilus".to_string());
        let sizes = application_sizes(&[entry], &roots, &commands);
        assert_eq!(sizes[0].bytes(), Some(1605 * 1024));
    }

    #[test]
    fn flatpak_output_and_metadata_parse() {
        assert_eq!(parse_flatpak_size("\n10516992\n"), Some(10_516_992));
        assert_eq!(parse_flatpak_size("error: not installed"), None);
        assert_eq!(parse_flatpak_size(""), None);
        assert_eq!(
            parse_flatpak_runtime(
                "[Runtime]\nruntime=x/y\n[Application]\nruntime=org.kde.Platform/x86_64/6.7\n"
            ),
            Some("org.kde.Platform".to_string())
        );
        assert_eq!(parse_flatpak_runtime("[Application]\nname=a\n"), None);
    }

    #[test]
    fn a_snap_is_its_current_revisions_file() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        fs::create_dir_all(roots.snap_mounts.join("firefox")).unwrap();
        std::os::unix::fs::symlink("4793", roots.snap_mounts.join("firefox/current")).unwrap();
        fs::create_dir_all(&roots.snap_files).unwrap();
        fs::write(roots.snap_files.join("firefox_4793.snap"), vec![0u8; 8192]).unwrap();
        fs::write(roots.snap_files.join("firefox_4700.snap"), vec![0u8; 1]).unwrap();
        fs::create_dir_all(roots.home.as_ref().unwrap().join("snap/firefox/common")).unwrap();
        fs::write(
            roots.home.as_ref().unwrap().join("snap/firefox/common/a"),
            b"12345",
        )
        .unwrap();
        let entry = roots.snap_desktop.join("firefox_firefox.desktop");
        let gone = roots.snap_desktop.join("gone_gone.desktop");
        let sizes = application_sizes(
            &[
                query(entry, SourceKind::Native),
                query(gone, SourceKind::Snap),
            ],
            &roots,
            &Fixture::default(),
        );
        assert_eq!(
            sizes[0],
            ApplicationSize::Known {
                bytes: 8192,
                source: ApplicationSizeSource::Snap {
                    name: "firefox".to_string(),
                    revision: "4793".to_string()
                },
                data: Some(5),
            }
        );
        assert_eq!(
            sizes[1],
            ApplicationSize::Unknown(ApplicationSizeUnknown::SnapMissing {
                name: "gone".to_string()
            })
        );
    }

    #[test]
    fn a_parallel_snap_instance_is_named_from_its_entry() {
        assert_eq!(
            snap_instance(Path::new("/x/foo+beta_foo.desktop")),
            Some("foo_beta".to_string())
        );
        assert_eq!(snap_instance(Path::new("/x/plain.desktop")), None);
    }

    #[test]
    fn an_appimage_and_a_home_program_are_their_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let home = roots.home.clone().unwrap();
        fs::create_dir_all(home.join("Apps")).unwrap();
        let image = home.join("Apps/Tool.AppImage");
        fs::write(&image, vec![0u8; 3000]).unwrap();
        let script = home.join(".local/bin/thing");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, vec![0u8; 42]).unwrap();
        let mut appimage = query(home.join("a.desktop"), SourceKind::AppImage);
        appimage.program = Some(image.to_string_lossy().into_owned());
        let mut own = query(home.join("b.desktop"), SourceKind::Native);
        own.executable = Some(script.clone());
        own.scope = EntryScope::User;
        let mut missing = query(home.join("c.desktop"), SourceKind::AppImage);
        missing.program = Some("/opt/Gone.AppImage".to_string());
        let sizes = application_sizes(&[appimage, own, missing], &roots, &Fixture::default());
        assert_eq!(sizes[0].bytes(), Some(3000));
        assert_eq!(
            sizes[1],
            ApplicationSize::Known {
                bytes: 42,
                source: ApplicationSizeSource::File { path: script },
                data: None,
            }
        );
        assert_eq!(
            sizes[2],
            ApplicationSize::Unknown(ApplicationSizeUnknown::ProgramMissing {
                path: PathBuf::from("/opt/Gone.AppImage")
            })
        );
    }
}

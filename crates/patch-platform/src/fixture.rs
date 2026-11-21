//! A machine made of files: the demo window's, the sandbox screenshots',
//! and the tests'.
//!
//! The Flatpak half is real: the same reading and writing as the person's
//! machine, under a scratch root. The fontconfig answers, the display scale,
//! and the session type come from files beside it, because a scratch root
//! cannot have a damaged fontconfig cache or a second monitor. Nothing here
//! starts a process.
//!
//! ```text
//! <root>/fonts.tsv          pattern TAB file TAB style, one per query
//! <root>/fonts-rebuilt.tsv  what fonts.tsv becomes when the cache is rebuilt
//! <root>/scale              the logical monitors' scales, space separated
//! <root>/session            the session type
//! <root>/system-flatpak/    a system installation
//! <root>/.local/share/flatpak/ the person's installation and overrides
//! <root>/.local/state/better-os/ the record of applied fixes
//! <root>/machine/           the USB-C entries' machine: sys/, etc/, var/,
//!                           and kernel.log (this boot's classified lines)
//! ```

use std::path::{Path, PathBuf};

use patch_core::command::FC_MATCH;
use patch_core::{Command, FontQuery, ReadError};

use crate::command::{CommandRunner, Output, RunError};
use crate::display::DisplayScales;
use crate::host::LinuxHost;
use crate::paths::Paths;
use crate::store::RecordStore;

const FONTS: &str = "fonts.tsv";

/// The fixture machine's own root for the USB-C entries: `sys/`, `etc/`,
/// `var/`, and the kernel log.
pub const MACHINE: &str = "machine";
const FONTS_REBUILT: &str = "fonts-rebuilt.tsv";

/// Answers commands from the fixture's files.
pub struct FixtureCommands {
    root: PathBuf,
    paths: Paths,
}

impl FixtureCommands {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            paths: Paths::under(root),
        }
    }
}

fn ok(stdout: String) -> Result<Output, RunError> {
    Ok(Output {
        success: true,
        stdout,
        stderr: String::new(),
    })
}

impl CommandRunner for FixtureCommands {
    fn run(&self, command: &Command) -> Result<Output, RunError> {
        match command {
            Command::MatchFont(query) => {
                let Ok(text) = std::fs::read_to_string(self.root.join(FONTS)) else {
                    return Err(RunError::NotInstalled(FC_MATCH));
                };
                text.lines()
                    .filter_map(|line| {
                        let mut parts = line.splitn(3, '\t');
                        Some((parts.next()?, parts.next()?, parts.next().unwrap_or("")))
                    })
                    .find(|(pattern, _, _)| *pattern == query.pattern())
                    .map(|(_, file, style)| ok(format!("{file}\t{style}\n")))
                    .unwrap_or_else(|| {
                        Err(RunError::Failed(format!(
                            "no answer for {}",
                            query.pattern()
                        )))
                    })
            }
            Command::RebuildFontCache => {
                let rebuilt = self.root.join(FONTS_REBUILT);
                if rebuilt.is_file() {
                    std::fs::copy(&rebuilt, self.root.join(FONTS))
                        .map_err(|error| RunError::Failed(error.to_string()))?;
                }
                ok(String::new())
            }
            // Flatpak prints the file as it parsed it; the fixture prints
            // the file.
            Command::ShowFlatpakOverride(app) => {
                ok(std::fs::read_to_string(self.paths.user_override(app)).unwrap_or_default())
            }
            // journalctl prints the lines the query keeps; the fixture's
            // log is already that.
            Command::KernelUsbLog { .. } => ok(std::fs::read_to_string(
                self.root.join(MACHINE).join(crate::usb::FIXTURE_LOG),
            )
            .unwrap_or_default()),
        }
    }
}

/// The scales in `<root>/scale`.
pub struct FixtureDisplay {
    root: PathBuf,
}

impl DisplayScales for FixtureDisplay {
    fn scales(&self) -> Result<Vec<f64>, ReadError> {
        let text = std::fs::read_to_string(self.root.join("scale"))
            .map_err(|error| ReadError::Failed(format!("scale: {error}")))?;
        text.split_whitespace()
            .map(|word| {
                word.parse::<f64>()
                    .map_err(|error| ReadError::Failed(format!("scale {word:?}: {error}")))
            })
            .collect()
    }
}

/// The demo window's fixture machine: `<runtime>/better-os/fixes-demo`,
/// under the session's private runtime directory and nowhere else.
///
/// A fixture machine's files are written and copied by a Fix, so its root
/// must be the person's alone: the runtime directory has to be a real
/// directory (not a link) they own with no access for anyone else, and the
/// two folders below it are made 0700 or, when they exist, must be real
/// directories they own that nobody else can write. There is no shared
/// fallback such as `/tmp`, where another account could have laid links in
/// advance; without a private runtime directory the demo offers no fix and
/// says why.
pub fn private_demo_root(runtime: Option<&Path>) -> Result<PathBuf, String> {
    let path =
        better_core::private_dir::private_runtime_dir(runtime, &["better-os", "fixes-demo"])?;
    if !path.join("session").exists() {
        seed(&path, Seed::AllFaults).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(path)
}

/// A host over a fixture root.
pub fn host(root: &Path) -> LinuxHost {
    let session = std::fs::read_to_string(root.join("session"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    LinuxHost::new(
        Paths::under(root),
        Box::new(FixtureCommands::new(root)),
        Box::new(FixtureDisplay {
            root: root.to_path_buf(),
        }),
        session,
    )
    .with_usb(
        root.join(MACHINE),
        Box::new(crate::usb::FixtureUsbDaemon {
            root: root.join(MACHINE),
        }),
    )
}

/// The record store of a fixture root.
pub fn store(root: &Path) -> RecordStore {
    RecordStore::at(Paths::under(root).records)
}

/// What a seeded fixture machine has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Seed {
    /// Both faults: a damaged font cache, and an Electron Flatpak on
    /// Wayland at scale 2. Its USB-C machine is another laptop's.
    BothFaults,
    /// Both of those, on the owner's M5406WA with ticket 101's USB-C
    /// faults ([`UsbSeed::Failing`]): the demo window's machine.
    AllFaults,
    /// Neither.
    Healthy,
}

/// The Electron app a seeded machine has, the one ticket 76 reproduced.
pub const SEEDED_APP: &str = "io.github.shiftey.Desktop";

const DAMAGED_FILE: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

fn healthy_fonts() -> String {
    let answers = [
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "Regular",
        ),
        (
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "Regular",
        ),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "Book",
        ),
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
            "Bold",
        ),
        (
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "Regular",
        ),
        (
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "Regular",
        ),
    ];
    FontQuery::ALL
        .iter()
        .zip(answers)
        .map(|(query, (file, style))| format!("{}\t{file}\t{style}\n", query.pattern()))
        .collect()
}

fn damaged_fonts() -> String {
    FontQuery::ALL
        .iter()
        .map(|query| format!("{}\t{DAMAGED_FILE}\t\n", query.pattern()))
        .collect()
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// Makes a fixture machine under `root`, replacing what a previous seed
/// wrote there.
pub fn seed(root: &Path, seed: Seed) -> std::io::Result<()> {
    write(&root.join("session"), "wayland\n")?;
    write(&root.join("scale"), "2\n")?;
    write(&root.join(FONTS_REBUILT), &healthy_fonts())?;
    let app = Paths::under(root)
        .system_flatpak
        .join("app")
        .join(SEEDED_APP)
        .join("current/active");
    match seed {
        Seed::BothFaults | Seed::AllFaults => {
            write(&root.join(FONTS), &damaged_fonts())?;
            write(
                &app.join("metadata"),
                &format!(
                    "[Application]\nname={SEEDED_APP}\nruntime=org.freedesktop.Platform/x86_64/25.08\n\
                     base=app/org.electronjs.Electron2.BaseApp/x86_64/25.08\n\n\
                     [Context]\nsockets=x11;wayland;fallback-x11;\n\n\
                     [Environment]\nELECTRON_OZONE_PLATFORM_HINT=auto\n"
                ),
            )?;
            write(
                &app.join(format!("export/share/applications/{SEEDED_APP}.desktop")),
                "[Desktop Entry]\nType=Application\nName=GitHub Desktop\n",
            )?;
            // Electron's own version file, beside its binary, as GitHub
            // Desktop ships it (Electron 32, from before the hint was removed).
            write(&app.join("files/github-desktop/version"), "32.3.0")?;
            write(&app.join("files/github-desktop/resources.pak"), "")?;
        }
        Seed::Healthy => {
            write(&root.join(FONTS), &healthy_fonts())?;
            let _ = std::fs::remove_dir_all(Paths::under(root).system_flatpak.join("app"));
        }
    }
    seed_usb(
        root,
        match seed {
            Seed::AllFaults => UsbSeed::Failing,
            Seed::BothFaults | Seed::Healthy => UsbSeed::NotAffected,
        },
    )
}

/// What a fixture machine's USB-C looks like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbSeed {
    /// The owner's M5406WA with ticket 101's faults: the interface timed
    /// out after a sleep (one port left), the controller at 0000:63:00.4
    /// died on resume, and two sleeps since failed on its hub work.
    Failing,
    /// The same machine with the controller dead and its hub work free
    /// (no failed sleep yet), so a rebind is offered.
    ControllerDeadOnly,
    /// The same machine with nothing wrong this boot.
    Healthy,
    /// Another laptop.
    NotAffected,
}

/// This boot's kernel lines for a seed, from ticket 101's excerpts (message
/// text only).
fn usb_log(seed: UsbSeed) -> &'static str {
    match seed {
        UsbSeed::Failing => "\
ucsi_acpi USBC000:00: ucsi_handle_connector_change: GET_CONNECTOR_STATUS failed (-110)
PM: suspend entry (s2idle)
xhci_hcd 0000:63:00.4: AMD-Vi: Event logged [IO_PAGE_FAULT domain=0x0014 address=0x0 flags=0x0000]
xhci_hcd 0000:63:00.4: WARN: xHC restore state timeout
xhci_hcd 0000:63:00.4: PCI post-resume error -110!
xhci_hcd 0000:63:00.4: HC died; cleaning up
PM: suspend exit
ucsi_acpi USBC000:00: failed to re-enable notifications (-110)
PM: suspend entry (s2idle)
Freezing remaining freezable tasks failed after 20.004 seconds (0 tasks refusing to freeze, wq_busy=1):
workqueue usb_hub_wq: flags=0x104
PM: suspend exit
PM: suspend entry (s2idle)
Freezing remaining freezable tasks failed after 20.000 seconds (0 tasks refusing to freeze, wq_busy=1):
workqueue usb_hub_wq: flags=0x104
PM: suspend exit
",
        UsbSeed::ControllerDeadOnly => "\
PM: suspend entry (s2idle)
xhci_hcd 0000:63:00.4: HC died; cleaning up
PM: suspend exit
",
        UsbSeed::Healthy | UsbSeed::NotAffected => "\
PM: suspend entry (s2idle)
PM: suspend exit
",
    }
}

/// Lays out the fixture's USB-C machine under `<root>/machine`.
pub fn seed_usb(root: &Path, seed: UsbSeed) -> std::io::Result<()> {
    let machine = root.join(MACHINE);
    let _ = std::fs::remove_dir_all(&machine);
    let sys = machine.join("sys");
    let board = if seed == UsbSeed::NotAffected {
        "UX3405MA"
    } else {
        "M5406WA"
    };
    write(&sys.join("class/dmi/id/board_name"), &format!("{board}\n"))?;
    write(
        &sys.join("class/dmi/id/sys_vendor"),
        "ASUSTeK COMPUTER INC.\n",
    )?;
    let ports: &[&str] = if seed == UsbSeed::Failing {
        &["port0"]
    } else {
        &["port0", "port1"]
    };
    for port in ports {
        std::fs::create_dir_all(sys.join("class/typec").join(port))?;
    }
    let charging = if seed == UsbSeed::Failing { "0" } else { "1" };
    for (name, kind, online) in [
        ("ACAD", "Mains", "1"),
        ("ucsi-source-psy-USBC000:001", "USB", charging),
        ("ucsi-source-psy-USBC000:002", "USB", "0"),
    ] {
        let dir = sys.join("class/power_supply").join(name);
        write(&dir.join("type"), kind)?;
        write(&dir.join("online"), online)?;
    }
    let function = sys.join("devices/pci0000:00/0000:00:08.1/0000:63:00.4");
    write(&function.join("vendor"), "0x1022\n")?;
    write(&function.join("device"), "0x151e\n")?;
    write(&function.join("class"), "0x0c0330\n")?;
    std::fs::create_dir_all(sys.join("bus/pci/drivers/xhci_hcd"))?;
    std::os::unix::fs::symlink(
        "../../../../bus/pci/drivers/xhci_hcd",
        function.join("driver"),
    )?;
    std::fs::create_dir_all(sys.join("bus/pci/devices"))?;
    std::os::unix::fs::symlink(
        "../../../devices/pci0000:00/0000:00:08.1/0000:63:00.4",
        sys.join("bus/pci/devices/0000:63:00.4"),
    )?;
    std::fs::create_dir_all(sys.join("bus/usb/devices"))?;
    write(&machine.join(crate::usb::FIXTURE_LOG), usb_log(seed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn runtime() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn the_demo_machine_is_made_private_under_the_runtime_directory() {
        let dir = runtime();
        let root = private_demo_root(Some(dir.path())).unwrap();
        assert_eq!(root, dir.path().join("better-os/fixes-demo"));
        let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(root.join("fonts.tsv").is_file());
        // Again: the same machine, not reseeded.
        std::fs::write(root.join("fonts.tsv"), "kept").unwrap();
        private_demo_root(Some(dir.path())).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("fonts.tsv")).unwrap(),
            "kept"
        );
    }

    #[test]
    fn no_private_runtime_directory_means_no_demo_machine() {
        assert!(private_demo_root(None).is_err());
        assert!(private_demo_root(Some(Path::new("relative"))).is_err());
        let open = tempfile::tempdir().unwrap();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(private_demo_root(Some(open.path())).is_err());
    }

    #[test]
    fn a_link_laid_in_advance_is_refused() {
        let dir = runtime();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("better-os")).unwrap();
        assert!(private_demo_root(Some(dir.path())).is_err());
        assert!(
            std::fs::read_dir(elsewhere.path())
                .unwrap()
                .next()
                .is_none()
        );
    }
}

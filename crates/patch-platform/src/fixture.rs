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
//! <root>/data/flatpak/      the person's installation and overrides
//! <root>/state/better-os/   the record of applied fixes
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
}

/// The record store of a fixture root.
pub fn store(root: &Path) -> RecordStore {
    RecordStore::at(Paths::under(root).records)
}

/// What a seeded fixture machine has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Seed {
    /// Both faults: a damaged font cache, and an Electron Flatpak on
    /// Wayland at scale 2.
    BothFaults,
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
        Seed::BothFaults => {
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
        }
        Seed::Healthy => {
            write(&root.join(FONTS), &healthy_fonts())?;
            let _ = std::fs::remove_dir_all(Paths::under(root).system_flatpak.join("app"));
        }
    }
    Ok(())
}

//! The file a panel reads the picture with the power mode's mark from.
//!
//! GNOME's AppIndicator extension fits an `IconPixmap` into a square, so a
//! picture wider than tall would shrink to a strip (seen in a headless GNOME
//! Shell 46, `better-os-evidence/295/wide-pixmap-vs-file.png`). An
//! `IconName` that is a path to an image in a place the person can write
//! (`$XDG_RUNTIME_DIR` among them) and at least half again as wide as tall
//! is drawn at the file's own size instead (`appIndicator.js`,
//! `_createIconByIconData` and `_loadCustomImage`), and an SVG there is
//! rasterised at the display's scale (`better-os-evidence/295/
//! scale2-png-vs-svg.png`).
//!
//! The picture is written under one of two fixed names in turn. `IconName`
//! changes with every new picture, which is what makes the extension load
//! it again, while GNOME Shell's texture cache, which keeps a file monitor
//! for every path it ever loaded until the shell ends, holds two at most. A
//! picture identical to the one on disk is not written again. Both files
//! are removed when the item ends, and any left by a run that did not end
//! cleanly are removed when the next one starts.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The two names, in turn.
const NAMES: [&str; 2] = ["picture-a.svg", "picture-b.svg"];

/// Everything the item may leave in its directory: the pictures, and a
/// half-written one (`.picture-a.svg.tmp`).
fn is_ours(name: &str) -> bool {
    name.starts_with("picture-") || name.starts_with(".picture-")
}

/// The last picture written: which name, and what it said.
#[derive(Default)]
struct Last {
    name: Option<usize>,
    svg: String,
    /// The item is ending: nothing more is written.
    closed: bool,
}

/// The directory the item writes its pictures to.
pub struct Pictures {
    dir: PathBuf,
    last: Mutex<Last>,
}

impl Pictures {
    /// `$XDG_RUNTIME_DIR/better-os/monitor-tray`, or `None` without a
    /// runtime directory (then the item keeps its square pixmaps).
    pub fn in_runtime_dir() -> Option<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
        if !runtime.is_absolute() {
            return None;
        }
        Self::at(runtime.join("better-os").join("monitor-tray")).ok()
    }

    /// Pictures under `dir`, which is made private to the person; pictures
    /// a run before this one left there are removed.
    pub fn at(dir: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let pictures = Self {
            dir,
            last: Mutex::new(Last::default()),
        };
        pictures.remove_all();
        Ok(pictures)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `svg` under the other name and returns its path, or returns
    /// the current path when the picture is the one already there.
    pub fn write(&self, svg: &str) -> io::Result<PathBuf> {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.closed {
            return Err(io::Error::other("the taskbar item is ending"));
        }
        if let Some(name) = last.name
            && last.svg == svg
        {
            return Ok(self.dir.join(NAMES[name]));
        }
        let name = last.name.map_or(0, |n| 1 - n);
        let path = self.dir.join(NAMES[name]);
        let partial = self.dir.join(format!(".{}.tmp", NAMES[name]));
        std::fs::write(&partial, svg)?;
        std::fs::rename(&partial, &path)?;
        last.name = Some(name);
        last.svg = svg.to_string();
        Ok(path)
    }

    /// Removes the pictures; the next one written starts again.
    pub fn clear(&self) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let closed = last.closed;
        *last = Last {
            closed,
            ..Last::default()
        };
        self.remove_all();
    }

    /// Removes the pictures and writes none again: the item is ending, and
    /// a redraw still on its way must not leave a file behind.
    pub fn close(&self) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        *last = Last {
            closed: true,
            ..Last::default()
        };
        self.remove_all();
    }

    fn remove_all(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if is_ours(&entry.file_name().to_string_lossy()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

impl Drop for Pictures {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="36" height="16"/>"#;
    const TWO: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="16"/>"#;

    #[test]
    fn pictures_take_two_names_in_turn_and_nothing_outlives_the_item() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pictures");
        std::fs::create_dir_all(&root).unwrap();
        // What an earlier run left: old names and a half-written file.
        for left in ["picture-7.png", "picture-a.svg", ".picture-b.svg.tmp"] {
            std::fs::write(root.join(left), b"left over").unwrap();
        }
        let pictures = Pictures::at(root.clone()).unwrap();
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            0,
            "a stale file stays"
        );

        let first = pictures.write(ONE).unwrap();
        let second = pictures.write(TWO).unwrap();
        let third = pictures.write(ONE).unwrap();
        assert_ne!(first, second);
        assert_eq!(first, third, "two names, in turn");
        assert_eq!(std::fs::read_to_string(&third).unwrap(), ONE);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);

        // The same picture again is not written again, and keeps its name.
        let modified = std::fs::metadata(&third).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(pictures.write(ONE).unwrap(), third);
        assert_eq!(
            std::fs::metadata(&third).unwrap().modified().unwrap(),
            modified
        );

        // Ending: the files go, and a redraw on its way writes nothing.
        pictures.close();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        assert!(pictures.write(TWO).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        drop(pictures);
    }
}

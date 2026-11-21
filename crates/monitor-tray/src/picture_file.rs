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
//! scale2-png-vs-svg.png`). So the picture is written there as an SVG, under
//! a new name each time, because a panel may keep a file it already loaded;
//! the one before is removed once the new one is in place, and every one is
//! removed when the item ends.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const PREFIX: &str = "picture-";

/// The directory the item writes its pictures to, and the last one written.
pub struct Pictures {
    dir: PathBuf,
    next: AtomicU64,
    last: Mutex<Option<PathBuf>>,
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
        for entry in std::fs::read_dir(&dir)?.flatten() {
            if entry.file_name().to_string_lossy().starts_with(PREFIX) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(Self {
            dir,
            next: AtomicU64::new(0),
            last: Mutex::new(None),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `picture` under a new name and removes the one before.
    /// How many pictures this item has written.
    pub fn written(&self) -> u64 {
        self.next.load(Ordering::Relaxed)
    }

    /// Writes `svg` under a new name and removes the one before.
    pub fn write(&self, svg: &str) -> io::Result<PathBuf> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!("{PREFIX}{n}.svg"));
        let partial = self.dir.join(format!(".{PREFIX}{n}.svg.tmp"));
        std::fs::write(&partial, svg)?;
        std::fs::rename(&partial, &path)?;
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(before) = last.replace(path.clone()) {
            let _ = std::fs::remove_file(before);
        }
        Ok(path)
    }

    /// Removes the last picture written.
    pub fn clear(&self) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(before) = last.take() {
            let _ = std::fs::remove_file(before);
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

    #[test]
    fn each_picture_replaces_the_one_before_and_nothing_outlives_the_item() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pictures");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("picture-7.svg"), b"left over").unwrap();
        let pictures = Pictures::at(root.clone()).unwrap();
        assert!(
            !root.join("picture-7.svg").exists(),
            "a stale picture stays"
        );
        let image = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"36\" height=\"16\"/>";
        let first = pictures.write(image).unwrap();
        let second = pictures.write(image).unwrap();
        assert_eq!(pictures.written(), 2);
        assert_ne!(first, second);
        assert!(!first.exists());
        assert!(second.exists());
        assert!(std::fs::read(&second).unwrap().starts_with(b"<svg"));
        drop(pictures);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }
}

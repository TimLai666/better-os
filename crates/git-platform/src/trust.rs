//! Which repositories the person said they trust.
//!
//! git runs a `filter.<name>.clean` command named by a repository's
//! attributes whenever it reads a working-tree file, and has no switch to
//! stop it. So before Better Git reads a repository for the first time it
//! asks "Do you trust this repository?", and the answer is remembered here
//! per path. The file is `$XDG_STATE_HOME/better-os/git-trusted`: one
//! canonical path per entry, NUL-separated, written beside its name and
//! renamed over it.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The trusted repositories, read from and written to one file.
#[derive(Clone, Debug)]
pub struct TrustStore {
    file: Option<PathBuf>,
    trusted: Vec<PathBuf>,
}

impl TrustStore {
    /// Reads the store. A missing or unreadable file is an empty store; a
    /// store with no file (`None`) remembers only for this run.
    pub fn load(file: Option<PathBuf>) -> TrustStore {
        let trusted = file
            .as_deref()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| {
                bytes
                    .split(|&b| b == 0)
                    .filter(|entry| !entry.is_empty())
                    .map(|entry| PathBuf::from(OsStr::from_bytes(entry)))
                    .collect()
            })
            .unwrap_or_default();
        TrustStore { file, trusted }
    }

    /// The default file, under `$XDG_STATE_HOME` or `~/.local/state`.
    pub fn default_file() -> Option<PathBuf> {
        let state = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(state.join("better-os").join("git-trusted"))
    }

    fn key(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    pub fn is_trusted(&self, path: &Path) -> bool {
        let key = Self::key(path);
        self.trusted.contains(&key)
    }

    /// Remembers the path. The file is re-read first so another window's
    /// answer is not lost.
    pub fn trust(&mut self, path: &Path) -> io::Result<()> {
        let key = Self::key(path);
        if let Some(file) = &self.file {
            let fresh = TrustStore::load(Some(file.clone()));
            for p in fresh.trusted {
                if !self.trusted.contains(&p) {
                    self.trusted.push(p);
                }
            }
        }
        if !self.trusted.contains(&key) {
            self.trusted.push(key);
        }
        self.save()
    }

    /// Forgets the path.
    pub fn forget(&mut self, path: &Path) -> io::Result<()> {
        let key = Self::key(path);
        self.trusted.retain(|p| *p != key);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        for path in &self.trusted {
            bytes.extend_from_slice(path.as_os_str().as_bytes());
            bytes.push(0);
        }
        let temporary = file.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, file)
    }
}

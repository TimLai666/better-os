//! The two files the setting lives in, each with exactly one writer.
//!
//! `choices.json` is written by Better Manager and only read by the service;
//! `seen.json` is written by the service and only read by Better Manager. Two
//! files rather than one, so a choice made in the window and a request
//! recorded by the service at the same moment can never overwrite each other.
//! Both live in `$XDG_CONFIG_HOME/better-os/window-attention/`, and both are
//! written to a temporary file beside them and renamed into place, so a reader
//! sees the old file or the new one and never half of one.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{CHOICES_VERSION, Choices, SEEN_VERSION, SeenApps};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{path} could not be read: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a window attention file this version understands: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path} was written by a newer Better OS (version {found})")]
    Newer { path: PathBuf, found: u32 },
    #[error("{path} could not be written: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Where the two files are.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Store {
    directory: PathBuf,
}

impl Store {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// `$XDG_CONFIG_HOME/better-os/window-attention`, or the same under
    /// `$HOME/.config`.
    pub fn user_directory() -> PathBuf {
        let config = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
        };
        config.join("better-os").join("window-attention")
    }

    pub fn for_user() -> Self {
        Self::new(Self::user_directory())
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn choices_path(&self) -> PathBuf {
        self.directory.join("choices.json")
    }

    pub fn seen_path(&self) -> PathBuf {
        self.directory.join("seen.json")
    }

    /// The choices, or the defaults when nobody has chosen anything yet.
    pub fn load_choices(&self) -> Result<Choices, StoreError> {
        let path = self.choices_path();
        let choices: Choices = match read(&path)? {
            Some(choices) => choices,
            None => return Ok(Choices::default()),
        };
        if choices.version > CHOICES_VERSION {
            return Err(StoreError::Newer {
                path,
                found: choices.version,
            });
        }
        Ok(choices)
    }

    pub fn save_choices(&self, choices: &Choices) -> Result<(), StoreError> {
        let mut choices = choices.clone();
        choices.version = CHOICES_VERSION;
        write(&self.choices_path(), &choices)
    }

    /// The applications that have asked, or none.
    pub fn load_seen(&self) -> Result<SeenApps, StoreError> {
        let path = self.seen_path();
        let seen: SeenApps = match read(&path)? {
            Some(seen) => seen,
            None => {
                return Ok(SeenApps {
                    version: SEEN_VERSION,
                    ..SeenApps::default()
                });
            }
        };
        if seen.version > SEEN_VERSION {
            return Err(StoreError::Newer {
                path,
                found: seen.version,
            });
        }
        Ok(seen)
    }

    pub fn save_seen(&self, seen: &SeenApps) -> Result<(), StoreError> {
        let mut seen = seen.clone();
        seen.version = SEEN_VERSION;
        write(&self.seen_path(), &seen)
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StoreError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StoreError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let failed = |source| StoreError::Write {
        path: path.to_path_buf(),
        source,
    };
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(directory).map_err(failed)?;
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(std::io::Error::other)
        .map_err(failed)?;
    bytes.push(b'\n');
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = directory.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(failed)
}

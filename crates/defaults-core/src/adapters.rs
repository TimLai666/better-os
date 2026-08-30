//! The adapter set a run works against, built in one place.
//!
//! Which adapters exist for a real desktop is a decision ([ADR 0009]), not a
//! caller's choice, so every surface asks for the same two modes rather than
//! assembling its own list. That also keeps a user-facing surface from naming
//! `defaults-platform` at all: the GUI asks for a session and hands the engine
//! what it gets back.
//!
//! [ADR 0009]: ../../../docs/decisions/0009-defaults-declarations-and-adapters.md

use std::io;
use std::path::PathBuf;

use better_core::defaults::AdapterId;
use defaults_platform::{AdapterSet, DconfAdapter, MockDesktop, XdgDefaultAppAdapter};
use thiserror::Error;

/// Whether a run changes the desktop or a simulation of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterMode {
    /// The adapters that read and change the running desktop.
    Production,
    /// A simulated desktop that touches nothing outside itself. With a path it
    /// is kept between runs, which is the only way a simulated apply is still
    /// visible the next time something reads.
    Simulated { desktop_path: Option<PathBuf> },
}

#[derive(Debug, Error)]
pub enum AdapterSessionError {
    #[error("could not open the user's default-application records: {0}")]
    DefaultApplications(String),
    #[error("could not read the simulated desktop at {path}")]
    SimulatedDesktop {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The adapters for one run, and whatever has to be written back afterwards.
pub struct AdapterSession {
    adapters: AdapterSet,
    simulated: Option<MockDesktop>,
    desktop_path: Option<PathBuf>,
}

impl AdapterSession {
    pub fn open(mode: &AdapterMode) -> Result<Self, AdapterSessionError> {
        match mode {
            AdapterMode::Production => Ok(Self {
                adapters: production_adapters()?,
                simulated: None,
                desktop_path: None,
            }),
            AdapterMode::Simulated { desktop_path } => {
                let desktop = match desktop_path {
                    Some(path) => MockDesktop::load(path).map_err(|source| {
                        AdapterSessionError::SimulatedDesktop {
                            path: path.clone(),
                            source,
                        }
                    })?,
                    None => MockDesktop::new(),
                };
                Ok(Self {
                    adapters: desktop.adapter_set(),
                    simulated: Some(desktop),
                    desktop_path: desktop_path.clone(),
                })
            }
        }
    }

    pub fn adapters(&self) -> &AdapterSet {
        &self.adapters
    }

    pub fn adapters_mut(&mut self) -> &mut AdapterSet {
        &mut self.adapters
    }

    /// Whether this session changes a simulation rather than the desktop.
    pub fn is_simulated(&self) -> bool {
        self.simulated.is_some()
    }

    /// Whether a simulated desktop is discarded when this session ends.
    pub fn is_ephemeral(&self) -> bool {
        self.simulated.is_some() && self.desktop_path.is_none()
    }

    /// Writes a simulated desktop back to where it came from. A production
    /// session has nothing to write; its changes already went to the desktop.
    pub fn persist(&self) -> Result<(), AdapterSessionError> {
        let (Some(desktop), Some(path)) = (&self.simulated, &self.desktop_path) else {
            return Ok(());
        };
        desktop
            .save(path)
            .map_err(|source| AdapterSessionError::SimulatedDesktop {
                path: path.clone(),
                source,
            })
    }
}

/// The adapters that exist for a real desktop today.
///
/// Every other integration kind has no adapter at all, which is how the planner
/// reports manual action required instead of guessing a command. The two GNOME
/// adapters read and verify; they report manual action for a change, for the
/// reasons ADR 0009 records.
fn production_adapters() -> Result<AdapterSet, AdapterSessionError> {
    let writable = XdgDefaultAppAdapter::for_user()
        .map_err(|error| AdapterSessionError::DefaultApplications(error.to_string()))?;
    let effective = XdgDefaultAppAdapter::effective_for_user()
        .map_err(|error| AdapterSessionError::DefaultApplications(error.to_string()))?;
    Ok(AdapterSet::new()
        .with(Box::new(writable))
        .with(Box::new(effective))
        .with(Box::new(DconfAdapter::for_user(AdapterId::GnomeKeybinding)))
        .with(Box::new(DconfAdapter::for_user(
            AdapterId::GnomeDesktopSetting,
        ))))
}

/// Where a simulated desktop is kept when a caller wants one that survives.
pub fn default_simulated_desktop_path() -> PathBuf {
    let data = match std::env::var("XDG_DATA_HOME") {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"),
    };
    data.join("better-os/defaults/simulated-desktop.json")
}

/// The session this is running in.
///
/// An undetectable session stays undetectable rather than being assumed to be
/// GNOME: a declaration that names a session it is not running in should not
/// apply, and guessing here would make it apply anyway.
pub fn desktop_session() -> String {
    for key in ["XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"] {
        if let Ok(value) = std::env::var(key) {
            if let Some(first) = value.split(':').next() {
                if !first.trim().is_empty() {
                    return first.trim().to_lowercase();
                }
            }
        }
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simulated_session_carries_an_adapter_for_every_declared_kind() {
        let session = AdapterSession::open(&AdapterMode::Simulated { desktop_path: None })
            .expect("a simulated desktop with no path always opens");
        for id in defaults_platform::mock::ALL_ADAPTER_IDS {
            assert!(session.adapters().contains(id), "no adapter for {id:?}");
        }
        assert!(session.is_simulated());
        assert!(session.is_ephemeral());
    }

    #[test]
    fn a_simulated_session_with_a_path_is_kept_between_runs() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("desktop.json");
        let session = AdapterSession::open(&AdapterMode::Simulated {
            desktop_path: Some(path.clone()),
        })
        .expect("an absent file is an empty desktop");
        assert!(!session.is_ephemeral());
        session.persist().expect("the desktop must be writable");
        assert!(path.exists());
    }
}

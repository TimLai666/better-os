//! External displays, from `/sys/class/drm`.
//!
//! Each connector has a `status` file reading `connected`, `disconnected`, or
//! `unknown`. "External" means every connector except the internal panel, which
//! is recognized by connector type — `eDP` and `LVDS` are the two the kernel
//! uses for a laptop's own screen, and `DSI` for an embedded one.
//!
//! A connector reporting `unknown` is a real state: some analogue outputs cannot
//! tell whether anything is attached. It is counted as not connected rather than
//! as connected, because keeping a machine awake for a monitor that may not
//! exist is the wrong way round to be wrong.

use std::path::PathBuf;

use awake_core::{Observations, ProviderKind};

use crate::provider::{Cadence, DISPLAY_POLL_SECONDS, TriggerProvider};
use crate::roots::{ReadError, Roots, list_dir, read_attribute};

/// Connector name prefixes that are a machine's own built-in panel.
const INTERNAL_PANEL_TYPES: [&str; 3] = ["eDP", "LVDS", "DSI"];

/// One connector and what it reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Connector {
    /// The connector name with its card prefix removed, such as `HDMI-A-1`.
    pub name: String,
    pub connected: bool,
    pub internal: bool,
}

#[derive(Clone, Debug)]
pub struct DisplayProvider {
    roots: Roots,
}

impl DisplayProvider {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }

    fn drm_dir(&self) -> PathBuf {
        self.roots.sys_path("class/drm")
    }

    /// Every connector the kernel exposes.
    pub fn connectors(&self) -> Result<Vec<Connector>, ReadError> {
        let entries = list_dir(&self.drm_dir())?;
        let mut connectors = Vec::new();

        for entry in entries {
            let Some(entry_name) = entry.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            // `/sys/class/drm` also holds `card1`, `renderD128`, and `version`.
            // Only the entries with a `status` file are connectors, which is a
            // more durable test than pattern-matching the name.
            let Ok(status) = read_attribute(&entry.join("status")) else {
                continue;
            };
            // `card1-HDMI-A-1` becomes `HDMI-A-1`; a name with no card prefix is
            // used as it stands.
            let name = entry_name
                .split_once('-')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_else(|| entry_name.to_string());
            let internal = INTERNAL_PANEL_TYPES
                .iter()
                .any(|panel| name.starts_with(panel));
            // A writeback connector is a virtual capture target, not a screen a
            // person is looking at, so it never counts as an external display.
            if name.starts_with("Writeback") {
                continue;
            }
            connectors.push(Connector {
                name,
                connected: status == "connected",
                internal,
            });
        }

        Ok(connectors)
    }

    /// Whether any connector that is not the built-in panel has a screen on it.
    pub fn external_connected(&self) -> Result<bool, ReadError> {
        Ok(self
            .connectors()?
            .iter()
            .any(|connector| !connector.internal && connector.connected))
    }
}

impl TriggerProvider for DisplayProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::ExternalDisplay
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: DISPLAY_POLL_SECONDS,
        }
    }

    fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        match self.connectors() {
            Err(error) => into.mark_unavailable(ProviderKind::ExternalDisplay, error.explanation()),
            Ok(connectors) if connectors.is_empty() => {
                // A `/sys/class/drm` with no connectors at all means no
                // kernel-mode-setting driver is loaded, which is a real state on
                // a virtual machine. Reporting "no external display" there would
                // be a guess dressed as a reading.
                into.mark_unavailable(
                    ProviderKind::ExternalDisplay,
                    "awake.provider.no_drm_connectors",
                );
            }
            Ok(connectors) => {
                into.external_display_connected = Some(
                    connectors
                        .iter()
                        .any(|connector| !connector.internal && connector.connected),
                );
                into.mark_available(ProviderKind::ExternalDisplay);
            }
        }
    }
}

/// Builds a fake `/sys/class/drm` connector.
#[cfg(any(test, feature = "test-support"))]
pub fn write_connector(sys_dir: &std::path::Path, name: &str, status: &str) {
    let directory = sys_dir.join("class/drm").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("status"), format!("{status}\n")).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(connectors: &[(&str, &str)]) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let sys = directory.path().join("sys");
        std::fs::create_dir_all(sys.join("class/drm")).unwrap();
        for (name, status) in connectors {
            write_connector(&sys, name, status);
        }
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn external(roots: &Roots) -> Option<bool> {
        let mut observations = Observations::at(1_000);
        DisplayProvider::new(roots.clone()).sample(1_000, &mut observations);
        observations.external_display_connected
    }

    #[test]
    fn a_laptop_with_only_its_own_panel_reports_no_external_display() {
        let (_directory, roots) = fixture(&[
            ("card1-eDP-1", "connected"),
            ("card1-HDMI-A-1", "disconnected"),
            ("card1-DP-1", "disconnected"),
        ]);
        assert_eq!(external(&roots), Some(false));
    }

    #[test]
    fn a_monitor_on_hdmi_is_an_external_display() {
        let (_directory, roots) = fixture(&[
            ("card1-eDP-1", "connected"),
            ("card1-HDMI-A-1", "connected"),
        ]);
        assert_eq!(external(&roots), Some(true));
    }

    #[test]
    fn a_connector_that_cannot_tell_is_not_counted_as_a_monitor() {
        let (_directory, roots) =
            fixture(&[("card1-eDP-1", "connected"), ("card1-VGA-1", "unknown")]);
        assert_eq!(
            external(&roots),
            Some(false),
            "keeping the machine awake for a monitor that may not be there is the wrong way to be wrong"
        );
    }

    #[test]
    fn a_writeback_connector_is_not_a_screen_anyone_is_looking_at() {
        let (_directory, roots) = fixture(&[
            ("card1-eDP-1", "connected"),
            ("card1-Writeback-1", "connected"),
        ]);
        assert_eq!(external(&roots), Some(false));
    }

    #[test]
    fn an_internal_panel_is_recognized_by_every_name_the_kernel_uses_for_one() {
        let (_directory, roots) = fixture(&[
            ("card0-LVDS-1", "connected"),
            ("card0-DSI-1", "connected"),
            ("card0-eDP-1", "connected"),
        ]);
        assert_eq!(external(&roots), Some(false));
    }

    #[test]
    fn the_card_and_render_nodes_are_not_mistaken_for_connectors() {
        let directory = tempfile::tempdir().unwrap();
        let sys = directory.path().join("sys");
        std::fs::create_dir_all(sys.join("class/drm/card1")).unwrap();
        std::fs::create_dir_all(sys.join("class/drm/renderD128")).unwrap();
        std::fs::write(sys.join("class/drm/version"), b"drm 1.1.0\n").unwrap();
        write_connector(&sys, "card1-eDP-1", "connected");

        let provider = DisplayProvider::new(Roots::at(directory.path()));
        let connectors = provider.connectors().unwrap();
        assert_eq!(connectors.len(), 1);
        assert_eq!(connectors[0].name, "eDP-1");
        assert!(connectors[0].internal);
    }

    #[test]
    fn a_machine_with_no_kernel_mode_setting_says_unavailable_rather_than_no() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("sys/class/drm")).unwrap();
        let mut observations = Observations::at(1_000);
        DisplayProvider::new(Roots::at(directory.path())).sample(1_000, &mut observations);

        assert_eq!(observations.external_display_connected, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::ExternalDisplay)
                .explanation(),
            Some("awake.provider.no_drm_connectors")
        );
    }

    #[test]
    fn a_missing_drm_directory_names_the_path_that_was_not_there() {
        let directory = tempfile::tempdir().unwrap();
        let mut observations = Observations::at(1_000);
        DisplayProvider::new(Roots::at(directory.path())).sample(1_000, &mut observations);
        let explanation = observations
            .availability_of(ProviderKind::ExternalDisplay)
            .explanation()
            .unwrap()
            .to_string();
        assert!(explanation.contains("class/drm"), "{explanation}");
    }
}

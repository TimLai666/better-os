//! The display scale, as the compositor reports it.

use std::collections::HashMap;

use patch_core::ReadError;
use zbus::zvariant::OwnedValue;

/// What reports the scale of every logical monitor.
pub trait DisplayScales: Send {
    fn scales(&self) -> Result<Vec<f64>, ReadError>;
}

/// Mutter's `org.gnome.Mutter.DisplayConfig` on the session bus: the
/// scale each logical monitor is drawn at, which is what the pointer has to
/// match. A call that Mutter does not answer within [`MUTTER_TIMEOUT`]
/// fails, so a check always ends.
#[derive(Debug, Default)]
pub struct MutterDisplay;

/// How long Mutter has to answer.
pub const MUTTER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A desktop with nothing to ask: every read says "not on this desktop" at
/// once rather than waiting on a bus name nobody owns.
#[derive(Debug, Default)]
pub struct NoDisplayService;

impl DisplayScales for NoDisplayService {
    fn scales(&self) -> Result<Vec<f64>, ReadError> {
        Err(ReadError::NotOnThisDesktop)
    }
}

/// What reports the scale on the desktop this session runs: Mutter on
/// GNOME, nothing yet elsewhere (the Better OS session on cosmic-comp is to
/// come).
pub fn for_desktop(desktop: &better_core::CurrentDesktop) -> Box<dyn DisplayScales> {
    if desktop.is_gnome() {
        Box::new(MutterDisplay)
    } else {
        Box::new(NoDisplayService)
    }
}

type MonitorSpec = (String, String, String, String);
type Mode = (
    String,
    i32,
    i32,
    f64,
    f64,
    Vec<f64>,
    HashMap<String, OwnedValue>,
);
type Monitor = (MonitorSpec, Vec<Mode>, HashMap<String, OwnedValue>);
type LogicalMonitor = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<MonitorSpec>,
    HashMap<String, OwnedValue>,
);
type CurrentState = (
    u32,
    Vec<Monitor>,
    Vec<LogicalMonitor>,
    HashMap<String, OwnedValue>,
);

impl DisplayScales for MutterDisplay {
    fn scales(&self) -> Result<Vec<f64>, ReadError> {
        let failed =
            |error: zbus::Error| ReadError::Failed(format!("Mutter DisplayConfig: {error}"));
        let connection = zbus::blocking::connection::Builder::session()
            .map_err(failed)?
            .method_timeout(MUTTER_TIMEOUT)
            .build()
            .map_err(failed)?;
        let reply = connection
            .call_method(
                Some("org.gnome.Mutter.DisplayConfig"),
                "/org/gnome/Mutter/DisplayConfig",
                Some("org.gnome.Mutter.DisplayConfig"),
                "GetCurrentState",
                &(),
            )
            .map_err(failed)?;
        let (_, _, logical, _): CurrentState = reply.body().deserialize().map_err(failed)?;
        Ok(scales_of(&logical))
    }
}

fn scales_of(logical: &[LogicalMonitor]) -> Vec<f64> {
    logical.iter().map(|monitor| monitor.2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_gnome_the_scale_is_not_on_this_desktop_at_once() {
        let cosmic = better_core::CurrentDesktop::from_values(Some("COSMIC"), None);
        assert_eq!(
            for_desktop(&cosmic).scales(),
            Err(ReadError::NotOnThisDesktop)
        );
        let unknown = better_core::CurrentDesktop::from_values(None, None);
        assert_eq!(
            for_desktop(&unknown).scales(),
            Err(ReadError::NotOnThisDesktop)
        );
    }

    #[test]
    fn each_logical_monitor_gives_its_scale() {
        let spec = || {
            (
                "eDP-1".to_string(),
                "BOE".to_string(),
                "x".to_string(),
                "1".to_string(),
            )
        };
        let logical = vec![
            (0, 0, 2.0, 0, true, vec![spec()], HashMap::new()),
            (1440, 0, 1.0, 0, false, vec![spec()], HashMap::new()),
        ];
        assert_eq!(scales_of(&logical), [2.0, 1.0]);
    }
}

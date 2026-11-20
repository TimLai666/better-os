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
/// match. Another compositor has no such interface, and the read fails
/// rather than guessing a scale.
#[derive(Debug, Default)]
pub struct MutterDisplay;

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
        let connection = zbus::blocking::Connection::session().map_err(failed)?;
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

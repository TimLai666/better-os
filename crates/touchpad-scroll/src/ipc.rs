//! The names the settings window and the service share on the system bus.
//!
//! There is one service per touchpad, so each owns its own name under
//! [`BUS_NAME_PREFIX`]: `org.betteros.TouchpadScroll1.event7` for
//! `/dev/input/event7`. The D-Bus policy lets only the service's account own
//! names under the prefix, and lets anyone call them; the service decides who
//! may change a factor (see [`crate::session_rule`]).

pub const BUS_NAME_PREFIX: &str = "org.betteros.TouchpadScroll1.";
pub const INTERFACE: &str = "org.betteros.TouchpadScroll1";
pub const OBJECT_PATH: &str = "/org/betteros/TouchpadScroll1";

/// The account the service runs as once it has opened its two nodes.
pub const SERVICE_USER: &str = "_better-touchpad-scroll";

/// Where the service keeps the factors across restarts and reboots.
pub const STATE_FILE: &str = "/var/lib/better-touchpad-scroll/factors";

/// Written by better-touchpad's postinst when it could not start the
/// pass-throughs for the touchpads already present; read by the Scrolling page
/// to say "restart or reconnect the touchpad". Under /run, so a restart, which
/// is one of the two fixes, also clears it.
pub const RECONNECT_MARKER: &str = "/run/better-os/touchpad-scroll-reconnect";

/// The exit status of an instance whose touchpad went away (`ENODEV`). The unit
/// names it in `RestartPreventExitStatus=`, so a pad that drops off and comes
/// back is restarted by udev and `BindsTo=`, never by `Restart=`, and a flaky
/// pad never runs the unit into its start limit. `EX_UNAVAILABLE`.
pub const EXIT_DEVICE_GONE: u8 = 69;

pub fn bus_name(event: &str) -> String {
    format!("{BUS_NAME_PREFIX}{event}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_name_is_under_the_prefix_the_policy_allows() {
        assert_eq!(
            super::bus_name("event7"),
            "org.betteros.TouchpadScroll1.event7"
        );
        assert!(super::bus_name("event7").starts_with(super::BUS_NAME_PREFIX));
    }
}

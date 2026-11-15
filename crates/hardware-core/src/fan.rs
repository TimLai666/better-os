//! Fan mode: the two modes and what `pwm1_enable` says about them.

/// The modes the card offers and `SetFanMode` accepts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FanMode {
    Auto,
    Full,
}

impl FanMode {
    pub const ALL: [FanMode; 2] = [FanMode::Auto, FanMode::Full];

    /// The argument `org.betteros.Hardware1.SetFanMode` takes.
    pub fn wire(self) -> &'static str {
        match self {
            FanMode::Auto => "auto",
            FanMode::Full => "full",
        }
    }

    /// Exactly `auto` or `full`; nothing else, not even another case.
    pub fn from_wire(mode: &str) -> Option<FanMode> {
        FanMode::ALL.into_iter().find(|m| m.wire() == mode)
    }

    /// What is written to `pwm1_enable`: the hwmon ABI's 0 (no control,
    /// full speed) and 2 (automatic). Whether `asus-nb-wmi` honours both is
    /// the second machine fact ticket 86 records.
    pub fn pwm_enable(self) -> u8 {
        match self {
            FanMode::Auto => 2,
            FanMode::Full => 0,
        }
    }
}

/// What a read of `pwm1_enable` means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FanState {
    Mode(FanMode),
    /// Another value (1 is manual, which this machine offers no `pwm1` for);
    /// the card selects neither mode.
    Other(u8),
}

impl FanState {
    pub fn from_pwm_enable(raw: &str) -> Option<FanState> {
        let value: u8 = raw.trim().parse().ok()?;
        Some(match value {
            2 => FanState::Mode(FanMode::Auto),
            0 => FanState::Mode(FanMode::Full),
            other => FanState::Other(other),
        })
    }

    pub fn mode(self) -> Option<FanMode> {
        match self {
            FanState::Mode(mode) => Some(mode),
            FanState::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_auto_and_full_cross_the_bus() {
        assert_eq!(FanMode::from_wire("auto"), Some(FanMode::Auto));
        assert_eq!(FanMode::from_wire("full"), Some(FanMode::Full));
        for refused in ["", "Auto", "FULL", "manual", "auto ", "2", "auto\0"] {
            assert_eq!(FanMode::from_wire(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn pwm_enable_values_map_both_ways() {
        assert_eq!(
            FanState::from_pwm_enable("2\n"),
            Some(FanState::Mode(FanMode::Auto))
        );
        assert_eq!(
            FanState::from_pwm_enable("0"),
            Some(FanState::Mode(FanMode::Full))
        );
        assert_eq!(FanState::from_pwm_enable("1"), Some(FanState::Other(1)));
        assert_eq!(FanState::from_pwm_enable("x"), None);
        for mode in FanMode::ALL {
            assert_eq!(
                FanState::from_pwm_enable(&mode.pwm_enable().to_string()).and_then(FanState::mode),
                Some(mode)
            );
        }
    }
}

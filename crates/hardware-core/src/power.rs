//! Power mode: what power-profiles-daemon says and what the card offers.

/// One of the profiles power-profiles-daemon knows, in the order the card
/// lists them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];

    /// The name on the bus.
    pub fn wire(self) -> &'static str {
        match self {
            Profile::PowerSaver => "power-saver",
            Profile::Balanced => "balanced",
            Profile::Performance => "performance",
        }
    }

    pub fn from_wire(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.wire() == name)
    }
}

/// Why the daemon says performance is reduced (`PerformanceDegraded`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Degradation {
    LapDetected,
    HighTemperature,
    /// A reason this version does not know, kept as the daemon wrote it.
    Other(String),
}

impl Degradation {
    /// `None` for the empty string, which means nothing is degraded.
    pub fn parse(raw: &str) -> Option<Degradation> {
        match raw.trim() {
            "" => None,
            "lap-detected" => Some(Degradation::LapDetected),
            "high-operating-temperature" => Some(Degradation::HighTemperature),
            other => Some(Degradation::Other(other.to_string())),
        }
    }
}

/// An application holding a profile (`ActiveProfileHolds`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hold {
    pub application: String,
    /// The profile as the daemon names it; a name this version does not know
    /// is shown as written.
    pub profile: String,
}

/// Everything read from power-profiles-daemon.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PowerProfilesFacts {
    pub active: String,
    /// `Profiles`, each entry's `Profile` key, in the daemon's order.
    pub profiles: Vec<String>,
    pub degraded: String,
    pub holds: Vec<Hold>,
}

/// What the Power mode card shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PowerModeView {
    /// Only the profiles the daemon lists, in the card's order.
    pub options: Vec<Profile>,
    /// `None` when the daemon's active profile is not one of `options`.
    pub selected: Option<Profile>,
    /// The second, caution-coloured lines: holds first, then degradation.
    pub cautions: Vec<Caution>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Caution {
    Held(Hold),
    Degraded(Degradation),
}

pub fn power_mode_view(facts: &PowerProfilesFacts) -> PowerModeView {
    let options: Vec<Profile> = Profile::ALL
        .into_iter()
        .filter(|p| facts.profiles.iter().any(|listed| listed == p.wire()))
        .collect();
    let selected = Profile::from_wire(&facts.active).filter(|p| options.contains(p));
    let mut cautions: Vec<Caution> = facts.holds.iter().cloned().map(Caution::Held).collect();
    // The daemon reports degradation for the performance profile; it is shown
    // whenever the daemon reports it, as GNOME Settings does.
    if let Some(degradation) = Degradation::parse(&facts.degraded) {
        cautions.push(Caution::Degraded(degradation));
    }
    PowerModeView {
        options,
        selected,
        cautions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(profiles: &[&str], active: &str) -> PowerProfilesFacts {
        PowerProfilesFacts {
            active: active.into(),
            profiles: profiles.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn only_listed_profiles_are_offered_in_the_cards_order() {
        let view = power_mode_view(&facts(
            &["performance", "balanced", "power-saver"],
            "balanced",
        ));
        assert_eq!(view.options, Profile::ALL.to_vec());
        assert_eq!(view.selected, Some(Profile::Balanced));
        let view = power_mode_view(&facts(&["balanced", "power-saver"], "balanced"));
        assert_eq!(view.options, vec![Profile::PowerSaver, Profile::Balanced]);
        assert!(view.cautions.is_empty());
    }

    #[test]
    fn an_active_profile_not_offered_selects_nothing() {
        let view = power_mode_view(&facts(&["balanced", "power-saver"], "performance"));
        assert_eq!(view.selected, None);
        let view = power_mode_view(&facts(&["balanced"], "turbo"));
        assert_eq!(view.selected, None);
    }

    #[test]
    fn degradation_rows_read_each_reason() {
        assert_eq!(Degradation::parse(""), None);
        assert_eq!(
            Degradation::parse("lap-detected"),
            Some(Degradation::LapDetected)
        );
        assert_eq!(
            Degradation::parse("high-operating-temperature"),
            Some(Degradation::HighTemperature)
        );
        assert_eq!(
            Degradation::parse("something-new"),
            Some(Degradation::Other("something-new".into()))
        );
    }

    #[test]
    fn holds_come_before_degradation() {
        let mut f = facts(&["power-saver", "balanced", "performance"], "performance");
        f.degraded = "lap-detected".into();
        f.holds = vec![Hold {
            application: "GNOME Boxes".into(),
            profile: "performance".into(),
        }];
        let view = power_mode_view(&f);
        assert_eq!(
            view.cautions,
            vec![
                Caution::Held(f.holds[0].clone()),
                Caution::Degraded(Degradation::LapDetected)
            ]
        );
    }

    #[test]
    fn wire_names_round_trip() {
        for p in Profile::ALL {
            assert_eq!(Profile::from_wire(p.wire()), Some(p));
        }
        assert_eq!(
            serde_json::to_string(&Profile::PowerSaver).unwrap(),
            "\"power-saver\""
        );
    }
}

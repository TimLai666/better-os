//! Power mode: what power-profiles-daemon says and what the card offers.

use crate::charge_state::Language;

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

    /// The mode's name, as the System page, the taskbar item and its popup
    /// all write it (GNOME Settings' words).
    pub fn name(self, language: Language) -> &'static str {
        let zh = language == Language::TraditionalChinese;
        match (self, zh) {
            (Profile::PowerSaver, false) => "Best power efficiency",
            (Profile::PowerSaver, true) => "最佳電源效率",
            (Profile::Balanced, false) => "Balanced",
            (Profile::Balanced, true) => "平衡",
            (Profile::Performance, false) => "Best performance",
            (Profile::Performance, true) => "最佳效能",
        }
    }
}

/// The mode in force, as a place that only shows it (the taskbar item)
/// reads it: `None` when the machine offers no modes or the daemon's active
/// mode is not one it lists, so nothing is guessed.
pub fn mode_in_force(facts: &PowerProfilesFacts) -> Option<Profile> {
    power_mode_view(facts).selected
}

/// What a place that shows the mode without changing it says (Better
/// Monitor's taskbar item, its tooltip and its popup, ticket 295): the mode
/// in force by name, the modes the machine offers with the one in force
/// marked, and the System page's caution sentences. One rule for all of
/// them, so they never disagree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModeShown {
    pub mode: Profile,
    pub name: &'static str,
    /// Each mode the daemon lists that this version knows, in the card's
    /// order, with whether it is in force.
    pub offered: Vec<(Profile, &'static str, bool)>,
    pub cautions: Vec<String>,
}

/// The mode to show, or `None`: no daemon, no modes, or an active mode this
/// version does not know or the daemon does not list. A mode is only ever
/// shown by name in the reader's language, never as the daemon's own word.
pub fn mode_shown(facts: &PowerProfilesFacts, language: Language) -> Option<ModeShown> {
    let view = power_mode_view(facts);
    let mode = view.selected?;
    Some(ModeShown {
        mode,
        name: mode.name(language),
        offered: view
            .options
            .iter()
            .map(|option| (*option, option.name(language), *option == mode))
            .collect(),
        cautions: view
            .cautions
            .iter()
            .map(|caution| caution.words(language))
            .collect(),
    })
}

/// What a place that shows the mode says when the daemon answers and could
/// not be read.
pub fn unreadable_words(language: Language) -> &'static str {
    match language {
        Language::English => "could not be read",
        Language::TraditionalChinese => "無法讀取",
    }
}

impl Caution {
    /// The System page's sentence for it, which every place that shows the
    /// mode uses.
    pub fn words(&self, language: Language) -> String {
        let zh = language == Language::TraditionalChinese;
        match self {
            Caution::Held(hold) => {
                // A mode this version does not know is said as the daemon
                // wrote it.
                let mode = Profile::from_wire(&hold.profile)
                    .map(|p| p.name(language).to_string())
                    .unwrap_or_else(|| hold.profile.clone());
                if zh {
                    format!("{} 正在要求「{mode}」模式。", hold.application)
                } else {
                    format!("{} is asking for {mode}.", hold.application)
                }
            }
            Caution::Degraded(Degradation::LapDetected) => if zh {
                "電腦放在腿上時，效能模式會降低以免過熱。"
            } else {
                "Performance is reduced while the PC is on your lap, to keep it cool."
            }
            .to_string(),
            Caution::Degraded(Degradation::HighTemperature) => if zh {
                "溫度太高，效能模式暫時降低。"
            } else {
                "It's running hot, so performance is reduced for now."
            }
            .to_string(),
            Caution::Degraded(Degradation::Other(reason)) => {
                if zh {
                    format!("效能模式暫時降低（{reason}）。")
                } else {
                    format!("Performance is reduced for now ({reason}).")
                }
            }
        }
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
    fn every_mode_has_a_name_in_both_languages() {
        use crate::charge_state::Language;
        let english: Vec<&str> = Profile::ALL
            .iter()
            .map(|p| p.name(Language::English))
            .collect();
        assert_eq!(
            english,
            ["Best power efficiency", "Balanced", "Best performance"]
        );
        let chinese: Vec<&str> = Profile::ALL
            .iter()
            .map(|p| p.name(Language::TraditionalChinese))
            .collect();
        assert_eq!(chinese, ["最佳電源效率", "平衡", "最佳效能"]);
    }

    #[test]
    fn the_mode_in_force_is_only_a_listed_one() {
        assert_eq!(
            mode_in_force(&facts(&["power-saver", "balanced"], "power-saver")),
            Some(Profile::PowerSaver)
        );
        assert_eq!(mode_in_force(&facts(&[], "")), None);
        assert_eq!(mode_in_force(&facts(&["balanced"], "performance")), None);
        assert_eq!(mode_in_force(&PowerProfilesFacts::default()), None);
    }

    #[test]
    fn a_mode_is_shown_only_by_name_and_only_when_known_and_listed() {
        let all = ["power-saver", "balanced", "performance"];
        let shown = mode_shown(&facts(&all, "performance"), Language::TraditionalChinese).unwrap();
        assert_eq!(shown.name, "最佳效能");
        assert_eq!(
            shown.offered,
            vec![
                (Profile::PowerSaver, "最佳電源效率", false),
                (Profile::Balanced, "平衡", false),
                (Profile::Performance, "最佳效能", true),
            ]
        );
        // Only modes this version knows: never the daemon's English word.
        assert_eq!(
            mode_shown(&facts(&["turbo"], "turbo"), Language::English),
            None
        );
        assert_eq!(
            mode_shown(
                &facts(&["balanced"], "performance"),
                Language::TraditionalChinese
            ),
            None
        );
        assert_eq!(mode_shown(&facts(&[], ""), Language::English), None);
        let two = mode_shown(
            &facts(&["balanced", "turbo"], "balanced"),
            Language::English,
        )
        .unwrap();
        assert_eq!(two.offered, vec![(Profile::Balanced, "Balanced", true)]);
    }

    #[test]
    fn the_cautions_are_the_system_pages_sentences() {
        let mut f = facts(&["power-saver", "balanced", "performance"], "performance");
        f.degraded = "high-operating-temperature".into();
        f.holds = vec![Hold {
            application: "GNOME Boxes".into(),
            profile: "performance".into(),
        }];
        assert_eq!(
            mode_shown(&f, Language::English).unwrap().cautions,
            vec![
                "GNOME Boxes is asking for Best performance.".to_string(),
                "It's running hot, so performance is reduced for now.".to_string(),
            ]
        );
        assert_eq!(
            Caution::Degraded(Degradation::LapDetected).words(Language::TraditionalChinese),
            "電腦放在腿上時，效能模式會降低以免過熱。"
        );
        assert_eq!(
            Caution::Degraded(Degradation::Other("x".into())).words(Language::English),
            "Performance is reduced for now (x)."
        );
        assert_eq!(unreadable_words(Language::TraditionalChinese), "無法讀取");
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

//! The battery charge limit: the three choices, and the closed set the
//! daemon accepts.

/// The only values `org.betteros.Hardware1.SetChargeLimit` accepts, in the
/// card's order.
pub const CHARGE_LIMITS: [u8; 3] = [100, 80, 60];

/// Whether the daemon accepts `percent`.
pub fn accepts(percent: u8) -> bool {
    CHARGE_LIMITS.contains(&percent)
}

/// One entry of the card's list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChargeChoice {
    /// One of [`CHARGE_LIMITS`].
    Limit(u8),
    /// A value set by something else, shown so the card does not lie about
    /// it, and not offered once another is chosen.
    SetElsewhere(u8),
}

impl ChargeChoice {
    pub fn percent(self) -> u8 {
        match self {
            ChargeChoice::Limit(p) | ChargeChoice::SetElsewhere(p) => p,
        }
    }
}

/// What the card offers and which entry is selected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargeLimitView {
    pub choices: Vec<ChargeChoice>,
    pub selected: usize,
}

/// The card for a limit read as `current`.
pub fn charge_limit_view(current: u8) -> ChargeLimitView {
    let mut choices: Vec<ChargeChoice> = CHARGE_LIMITS
        .iter()
        .map(|p| ChargeChoice::Limit(*p))
        .collect();
    let selected = match CHARGE_LIMITS.iter().position(|p| *p == current) {
        Some(index) => index,
        None => {
            choices.push(ChargeChoice::SetElsewhere(current));
            choices.len() - 1
        }
    };
    ChargeLimitView { choices, selected }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_accepts_exactly_sixty_eighty_and_one_hundred() {
        for p in 0..=u8::MAX {
            assert_eq!(accepts(p), p == 60 || p == 80 || p == 100, "{p}");
        }
    }

    #[test]
    fn a_known_value_selects_its_choice() {
        let view = charge_limit_view(80);
        assert_eq!(
            view.choices,
            vec![
                ChargeChoice::Limit(100),
                ChargeChoice::Limit(80),
                ChargeChoice::Limit(60)
            ]
        );
        assert_eq!(view.selected, 1);
    }

    #[test]
    fn a_value_set_elsewhere_is_shown_as_its_own_entry() {
        let view = charge_limit_view(75);
        assert_eq!(view.choices.len(), 4);
        assert_eq!(view.choices[view.selected], ChargeChoice::SetElsewhere(75));
        assert_eq!(view.choices[view.selected].percent(), 75);
    }
}

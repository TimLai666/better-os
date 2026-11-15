//! The battery: what its card says and when Home calls it worn.

/// What `status` says, in the words the card has for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl BatteryStatus {
    pub fn parse(raw: &str) -> BatteryStatus {
        match raw.trim() {
            "Charging" => BatteryStatus::Charging,
            "Discharging" => BatteryStatus::Discharging,
            "Full" => BatteryStatus::Full,
            "Not charging" => BatteryStatus::NotCharging,
            _ => BatteryStatus::Unknown,
        }
    }
}

/// The machine's own battery, read.
#[derive(Clone, Debug, PartialEq)]
pub struct BatteryFacts {
    /// `capacity`, 0–100; `None` when it could not be read.
    pub capacity: Option<u8>,
    pub status: BatteryStatus,
    /// `charge_full` (or `energy_full`), in the file's own unit.
    pub full: Option<f64>,
    /// `charge_full_design` (or `energy_full_design`), in the same unit.
    pub full_design: Option<f64>,
    pub cycles: Option<u64>,
    pub model: Option<String>,
}

/// Home's "Battery is wearing out" item starts below this share of the
/// design capacity (spec section 3).
pub const WEARING_OUT_BELOW_PERCENT: u8 = 80;

impl BatteryFacts {
    /// What it holds now as a share of what it held new, rounded, at most
    /// 100. `None` when either figure is missing or the design is zero.
    pub fn health_percent(&self) -> Option<u8> {
        let design = self.full_design.filter(|d| *d > 0.0)?;
        let full = self.full.filter(|f| *f >= 0.0)?;
        Some(((full / design) * 100.0).round().clamp(0.0, 100.0) as u8)
    }

    /// Whether Home raises its battery item.
    pub fn is_wearing_out(&self) -> bool {
        self.health_percent()
            .is_some_and(|percent| percent < WEARING_OUT_BELOW_PERCENT)
    }

    /// The rows of the Battery expander, in order. A row whose value is
    /// missing is left out, and the cycle count is left out when it reads 0,
    /// because firmware that does not count reports 0.
    pub fn rows(&self) -> Vec<BatteryRow> {
        let mut rows = Vec::new();
        if let Some(percent) = self.health_percent() {
            rows.push(BatteryRow::Health(percent));
        }
        if let Some(cycles) = self.cycles.filter(|c| *c > 0) {
            rows.push(BatteryRow::Cycles(cycles));
        }
        if let Some(model) = self.model.clone().filter(|m| !m.trim().is_empty()) {
            rows.push(BatteryRow::Model(model));
        }
        rows
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatteryRow {
    Health(u8),
    Cycles(u64),
    Model(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> BatteryFacts {
        // BAT1 on the owner's machine, 2026-11-12.
        BatteryFacts {
            capacity: Some(87),
            status: BatteryStatus::Discharging,
            full: Some(4_712_000.0),
            full_design: Some(4_998_000.0),
            cycles: Some(0),
            model: Some("ASUS Battery".into()),
        }
    }

    #[test]
    fn status_words_cover_every_kernel_value() {
        assert_eq!(BatteryStatus::parse("Charging\n"), BatteryStatus::Charging);
        assert_eq!(
            BatteryStatus::parse("Discharging"),
            BatteryStatus::Discharging
        );
        assert_eq!(BatteryStatus::parse("Full"), BatteryStatus::Full);
        assert_eq!(
            BatteryStatus::parse("Not charging"),
            BatteryStatus::NotCharging
        );
        assert_eq!(BatteryStatus::parse("Unknown"), BatteryStatus::Unknown);
        assert_eq!(BatteryStatus::parse("whatever"), BatteryStatus::Unknown);
    }

    #[test]
    fn health_is_full_over_design_and_zero_cycles_are_hidden() {
        let battery = owner();
        assert_eq!(battery.health_percent(), Some(94));
        assert!(!battery.is_wearing_out());
        assert_eq!(
            battery.rows(),
            vec![
                BatteryRow::Health(94),
                BatteryRow::Model("ASUS Battery".into())
            ]
        );
        let counted = BatteryFacts {
            cycles: Some(212),
            ..owner()
        };
        assert!(counted.rows().contains(&BatteryRow::Cycles(212)));
    }

    #[test]
    fn missing_figures_leave_their_rows_out() {
        let battery = BatteryFacts {
            full_design: Some(0.0),
            cycles: None,
            model: None,
            ..owner()
        };
        assert_eq!(battery.health_percent(), None);
        assert!(battery.rows().is_empty());
        assert!(!battery.is_wearing_out());
    }

    #[test]
    fn under_eighty_percent_is_wearing_out() {
        let worn = BatteryFacts {
            full: Some(3_900_000.0),
            ..owner()
        };
        assert_eq!(worn.health_percent(), Some(78));
        assert!(worn.is_wearing_out());
        let exactly = BatteryFacts {
            full: Some(80.0),
            full_design: Some(100.0),
            ..owner()
        };
        assert!(!exactly.is_wearing_out());
    }

    #[test]
    fn a_battery_over_its_design_reads_one_hundred() {
        let fresh = BatteryFacts {
            full: Some(105.0),
            full_design: Some(100.0),
            ..owner()
        };
        assert_eq!(fresh.health_percent(), Some(100));
    }
}

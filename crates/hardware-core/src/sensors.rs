//! Temperatures and fan speeds: which readings are shown and what they are
//! called.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensorKind {
    Temperature,
    Fan,
}

/// One hwmon input, as `hardware-platform` read it.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorInput {
    /// The chip's `name` (`k10temp`, `asus`, `acpi_fan`).
    pub chip: String,
    /// The driver's label (`Tctl`, `cpu_fan`).
    pub label: Option<String>,
    pub kind: SensorKind,
    /// Degrees Celsius or RPM; `None` when the input could not be read.
    pub value: Option<f64>,
}

/// What a reading is called on the card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SensorLabel {
    Processor,
    Graphics,
    Ssd,
    Motherboard,
    WiFi,
    CpuFan,
    /// Nothing in the table: the hwmon name, and the driver's label after it
    /// when it gave one, unchanged.
    Raw(String),
}

fn is_wireless(chip: &str) -> bool {
    chip.starts_with("iwlwifi") || chip.contains("_phy")
}

/// The table in spec section 4, Cooling.
pub fn label(input: &SensorInput) -> SensorLabel {
    let label = input.label.as_deref().map(str::trim);
    match (input.kind, input.chip.as_str(), label) {
        (SensorKind::Temperature, "k10temp", Some("Tctl"))
        | (SensorKind::Temperature, "coretemp", Some("Package id 0")) => SensorLabel::Processor,
        (SensorKind::Temperature, "amdgpu", Some("edge")) => SensorLabel::Graphics,
        (SensorKind::Temperature, "nvme", Some("Composite")) => SensorLabel::Ssd,
        (SensorKind::Temperature, "acpitz", _) => SensorLabel::Motherboard,
        (SensorKind::Temperature, chip, _) if is_wireless(chip) => SensorLabel::WiFi,
        (SensorKind::Fan, _, Some(l)) if l.to_ascii_lowercase().contains("cpu") => {
            SensorLabel::CpuFan
        }
        (_, chip, Some(l)) if !l.is_empty() => SensorLabel::Raw(format!("{chip} {l}")),
        (_, chip, _) => SensorLabel::Raw(chip.to_string()),
    }
}

/// One row of the expander.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorRow {
    pub label: SensorLabel,
    pub kind: SensorKind,
    /// `None`: the input exists and could not be read.
    pub value: Option<f64>,
}

/// The rows, temperatures first and fans after, one reading per physical
/// fan: when a platform driver reports a labelled fan, the unlabelled
/// `acpi_fan` readings of the same machine are the same fan and are left
/// out.
pub fn rows(inputs: &[SensorInput]) -> Vec<SensorRow> {
    let platform_fan = inputs.iter().any(|i| {
        i.kind == SensorKind::Fan
            && i.chip != "acpi_fan"
            && i.label.as_deref().is_some_and(|l| !l.trim().is_empty())
    });
    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    for input in inputs {
        if input.kind == SensorKind::Fan && platform_fan && input.chip == "acpi_fan" {
            continue;
        }
        let row = SensorRow {
            label: label(input),
            kind: input.kind,
            value: input.value,
        };
        match input.kind {
            SensorKind::Temperature => temperatures.push(row),
            SensorKind::Fan => fans.push(row),
        }
    }
    temperatures.extend(fans);
    temperatures
}

/// The expander's description: the hottest temperature with its label, and
/// the first fan's speed. Either half is left out when there is nothing to
/// say about it.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorSummary {
    pub hottest: Option<(SensorLabel, f64)>,
    pub fan_rpm: Option<f64>,
}

pub fn summary(rows: &[SensorRow]) -> SensorSummary {
    let hottest = rows
        .iter()
        .filter(|r| r.kind == SensorKind::Temperature)
        .filter_map(|r| r.value.map(|v| (r.label.clone(), v)))
        .fold(
            None::<(SensorLabel, f64)>,
            |best, (label, value)| match best {
                Some((_, b)) if b >= value => best,
                _ => Some((label, value)),
            },
        );
    let fan_rpm = rows
        .iter()
        .filter(|r| r.kind == SensorKind::Fan)
        .find_map(|r| r.value);
    SensorSummary { hottest, fan_rpm }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(chip: &str, label: Option<&str>, value: f64) -> SensorInput {
        SensorInput {
            chip: chip.into(),
            label: label.map(str::to_string),
            kind: SensorKind::Temperature,
            value: Some(value),
        }
    }

    fn fan(chip: &str, label: Option<&str>, value: f64) -> SensorInput {
        SensorInput {
            kind: SensorKind::Fan,
            ..temp(chip, label, value)
        }
    }

    #[test]
    fn every_row_of_the_label_table() {
        assert_eq!(
            label(&temp("k10temp", Some("Tctl"), 1.0)),
            SensorLabel::Processor
        );
        assert_eq!(
            label(&temp("coretemp", Some("Package id 0"), 1.0)),
            SensorLabel::Processor
        );
        assert_eq!(
            label(&temp("amdgpu", Some("edge"), 1.0)),
            SensorLabel::Graphics
        );
        assert_eq!(
            label(&temp("nvme", Some("Composite"), 1.0)),
            SensorLabel::Ssd
        );
        assert_eq!(label(&temp("acpitz", None, 1.0)), SensorLabel::Motherboard);
        assert_eq!(label(&temp("mt7921_phy0", None, 1.0)), SensorLabel::WiFi);
        assert_eq!(label(&temp("iwlwifi_1", None, 1.0)), SensorLabel::WiFi);
        assert_eq!(
            label(&fan("asus", Some("cpu_fan"), 1.0)),
            SensorLabel::CpuFan
        );
        assert_eq!(
            label(&temp("coretemp", Some("Core 0"), 1.0)),
            SensorLabel::Raw("coretemp Core 0".into())
        );
        assert_eq!(
            label(&fan("acpi_fan", None, 1.0)),
            SensorLabel::Raw("acpi_fan".into())
        );
        assert_eq!(
            label(&temp("spd5118", None, 1.0)),
            SensorLabel::Raw("spd5118".into())
        );
    }

    #[test]
    fn the_acpi_fan_is_dropped_when_the_platform_driver_names_the_fan() {
        // The owner's machine: asus cpu_fan and acpi_fan both read ~4,800 RPM.
        let inputs = vec![
            temp("k10temp", Some("Tctl"), 52.0),
            fan("acpi_fan", None, 4_790.0),
            fan("asus", Some("cpu_fan"), 4_800.0),
        ];
        let rows = rows(&inputs);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].label, SensorLabel::CpuFan);
        assert!(
            rows.iter()
                .all(|r| r.label != SensorLabel::Raw("acpi_fan".into()))
        );
    }

    #[test]
    fn an_acpi_fan_alone_is_kept() {
        let rows = rows(&[fan("acpi_fan", None, 2_000.0)]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn temperatures_come_first_and_the_summary_takes_the_hottest() {
        let rows = rows(&[
            fan("asus", Some("cpu_fan"), 4_800.0),
            temp("nvme", Some("Composite"), 41.0),
            temp("k10temp", Some("Tctl"), 52.4),
            SensorInput {
                value: None,
                ..temp("amdgpu", Some("edge"), 0.0)
            },
        ]);
        assert_eq!(rows[0].kind, SensorKind::Temperature);
        assert_eq!(rows.last().unwrap().kind, SensorKind::Fan);
        let s = summary(&rows);
        assert_eq!(s.hottest, Some((SensorLabel::Processor, 52.4)));
        assert_eq!(s.fan_rpm, Some(4_800.0));
    }

    #[test]
    fn a_machine_with_no_fan_summarises_only_its_temperature() {
        let s = summary(&rows(&[temp("coretemp", Some("Package id 0"), 61.0)]));
        assert_eq!(s.hottest, Some((SensorLabel::Processor, 61.0)));
        assert_eq!(s.fan_rpm, None);
        assert_eq!(
            summary(&[]),
            SensorSummary {
                hottest: None,
                fan_rpm: None
            }
        );
    }
}

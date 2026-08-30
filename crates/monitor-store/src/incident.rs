//! "The system was just slow."
//!
//! An incident is a bookmark with evidence attached. The bookmark is the
//! moment the user pressed the button; the evidence is the sample taken at
//! that moment, the collector states behind it, and how far the recorded
//! numbers had moved from the preceding baseline. The window before and after
//! is stored on the record rather than applied at query time, so an incident
//! marked with a five-minute window still means five minutes a week later,
//! after the default has changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::sample::Sample;

/// The longest note a marker may carry. Long enough for a sentence about what
/// the machine was doing, short enough that it is not a place to paste a log.
pub const MAX_NOTE_LENGTH: usize = 500;

/// The smallest and largest windows an incident may ask for. A window of zero
/// captures nothing; a window of a day would outrun the retention period.
pub const MIN_WINDOW_SECONDS: u64 = 10;
pub const MAX_WINDOW_SECONDS: u64 = 3_600;

pub const DEFAULT_WINDOW_BEFORE_SECONDS: u64 = 300;
pub const DEFAULT_WINDOW_AFTER_SECONDS: u64 = 120;

/// How much history around the marker belongs to the incident.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IncidentWindow {
    pub before_seconds: u64,
    pub after_seconds: u64,
}

impl Default for IncidentWindow {
    fn default() -> Self {
        Self {
            before_seconds: DEFAULT_WINDOW_BEFORE_SECONDS,
            after_seconds: DEFAULT_WINDOW_AFTER_SECONDS,
        }
    }
}

impl IncidentWindow {
    /// Whether both halves are inside the accepted range. The protocol edge and
    /// the store apply the same rule, so a CLI and the GUI cannot disagree
    /// about what a legal window is.
    pub fn is_valid(&self) -> bool {
        (MIN_WINDOW_SECONDS..=MAX_WINDOW_SECONDS).contains(&self.before_seconds)
            && (MIN_WINDOW_SECONDS..=MAX_WINDOW_SECONDS).contains(&self.after_seconds)
    }

    pub fn clamped(self) -> Self {
        Self {
            before_seconds: self
                .before_seconds
                .clamp(MIN_WINDOW_SECONDS, MAX_WINDOW_SECONDS),
            after_seconds: self
                .after_seconds
                .clamp(MIN_WINDOW_SECONDS, MAX_WINDOW_SECONDS),
        }
    }
}

/// How far a metric had moved from its recent baseline when the marker was
/// pressed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BaselineShift {
    pub baseline: f64,
    pub at_marker: f64,
    /// Samples the baseline was averaged over. One sample is not a baseline,
    /// and a reader has to be able to see that.
    pub baseline_samples: u32,
}

impl BaselineShift {
    pub fn delta(&self) -> f64 {
        self.at_marker - self.baseline
    }
}

/// One marked moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Incident {
    pub id: u64,
    pub marked_at_unix_ms: u64,
    pub monotonic_ns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub window: IncidentWindow,
    /// The sample taken at the marker: resources, entities, the busiest
    /// processes, and every collector's health.
    pub snapshot: Box<Sample>,
    /// System-level metrics that had a value both at the marker and across the
    /// preceding baseline window.
    #[serde(default)]
    pub baseline: BTreeMap<String, BaselineShift>,
    /// The process the marker was tied to, when it was raised from a selected
    /// row rather than from the toolbar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about_pid: Option<u32>,
}

impl Incident {
    /// The wall-clock interval the incident covers.
    pub fn range_unix_ms(&self) -> (u64, u64) {
        (
            self.marked_at_unix_ms
                .saturating_sub(self.window.before_seconds * 1_000),
            self.marked_at_unix_ms
                .saturating_add(self.window.after_seconds * 1_000),
        )
    }

    /// The metrics that moved most from the baseline, largest first. This is
    /// what an Incidents page leads with, because "CPU pressure went from 3 to
    /// 71" is the sentence a user is looking for.
    pub fn largest_shifts(&self, limit: usize) -> Vec<(&str, &BaselineShift)> {
        let mut shifts: Vec<(&str, &BaselineShift)> = self
            .baseline
            .iter()
            .map(|(key, shift)| (key.as_str(), shift))
            .collect();
        shifts.sort_by(|left, right| {
            right
                .1
                .delta()
                .abs()
                .total_cmp(&left.1.delta().abs())
                .then(left.0.cmp(right.0))
        });
        shifts.truncate(limit);
        shifts
    }
}

/// Compare the marker's readings with the mean of the baseline samples.
///
/// Only metrics that had a real value on both sides are compared. A metric
/// that was unsupported during the baseline has no baseline, and inventing one
/// from zero would produce exactly the fake result the whole crate exists to
/// avoid.
pub fn baseline_shifts(at_marker: &Sample, baseline: &[Sample]) -> BTreeMap<String, BaselineShift> {
    let mut shifts = BTreeMap::new();
    for (id, observation) in at_marker.metrics.iter() {
        let Some(current) = observation.as_f64() else {
            continue;
        };
        let mut total = 0.0;
        let mut count = 0u32;
        for sample in baseline {
            if let Some(value) = sample.value_of(id) {
                total += value;
                count += 1;
            }
        }
        if count == 0 {
            continue;
        }
        shifts.insert(
            id.to_string(),
            BaselineShift {
                baseline: total / count as f64,
                at_marker: current,
                baseline_samples: count,
            },
        );
    }
    shifts
}

/// Trim a note to the accepted length and strip the control characters a
/// terminal or an export reader would misinterpret.
pub fn sanitize_note(note: &str) -> Option<String> {
    let cleaned: String = note
        .chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .take(MAX_NOTE_LENGTH)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{
        CollectorId, CollectorReport, MetricId, Observation, Timestamp, UnsupportedReason,
    };

    fn id(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn sample(at_ms: u64, busy: f64) -> Sample {
        let mut report = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: at_ms,
                monotonic_ns: at_ms * 1_000_000,
            },
        );
        report
            .metrics
            .insert(id("cpu.utilization.busy"), Observation::float(busy));
        report.metrics.insert(
            id("cpu.temperature"),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "no sensor".into(),
            }),
        );
        Sample::from_reports(&[report], 4)
    }

    fn incident() -> Incident {
        let marker = sample(10_000, 0.9);
        let baseline: Vec<Sample> = (0..5).map(|index| sample(index * 1_000, 0.1)).collect();
        Incident {
            id: 1,
            marked_at_unix_ms: 10_000,
            monotonic_ns: 10_000_000_000,
            note: Some("everything froze while saving".into()),
            window: IncidentWindow::default(),
            baseline: baseline_shifts(&marker, &baseline),
            snapshot: Box::new(marker),
            about_pid: None,
        }
    }

    #[test]
    fn the_window_is_stored_on_the_record_rather_than_recomputed() {
        let mut marked = incident();
        marked.marked_at_unix_ms = 1_000_000;
        marked.window = IncidentWindow {
            before_seconds: 60,
            after_seconds: 30,
        };
        assert_eq!(marked.range_unix_ms(), (940_000, 1_030_000));
    }

    #[test]
    fn a_window_before_the_epoch_saturates_instead_of_wrapping() {
        let mut marked = incident();
        marked.marked_at_unix_ms = 5;
        assert_eq!(marked.range_unix_ms().0, 0);
    }

    #[test]
    fn a_baseline_shift_records_how_many_samples_it_averaged() {
        let marked = incident();
        let shift = &marked.baseline["cpu.utilization.busy"];
        assert_eq!(shift.baseline_samples, 5);
        assert!((shift.baseline - 0.1).abs() < 1e-9);
        assert!((shift.at_marker - 0.9).abs() < 1e-9);
        assert!((shift.delta() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_metric_with_no_baseline_reading_gets_no_invented_one() {
        let marked = incident();
        assert!(!marked.baseline.contains_key("cpu.temperature"));
    }

    #[test]
    fn an_empty_baseline_produces_no_comparisons_at_all() {
        assert!(baseline_shifts(&sample(1, 0.5), &[]).is_empty());
    }

    #[test]
    fn the_largest_shift_leads_the_summary() {
        let mut marked = incident();
        marked.baseline.insert(
            "memory.available".into(),
            BaselineShift {
                baseline: 100.0,
                at_marker: 40.0,
                baseline_samples: 5,
            },
        );
        let ranked = marked.largest_shifts(2);
        assert_eq!(ranked[0].0, "memory.available");
        assert_eq!(ranked[1].0, "cpu.utilization.busy");
    }

    #[test]
    fn a_note_is_trimmed_and_stripped_of_control_characters() {
        assert_eq!(sanitize_note("  slow  "), Some("slow".to_string()));
        assert_eq!(sanitize_note("a\u{0}b"), Some("ab".to_string()));
        assert_eq!(sanitize_note("   "), None);
        let long = "x".repeat(MAX_NOTE_LENGTH + 50);
        assert_eq!(sanitize_note(&long).unwrap().len(), MAX_NOTE_LENGTH);
    }

    #[test]
    fn a_window_outside_the_accepted_range_is_rejected_and_clamped_the_same_way() {
        let too_long = IncidentWindow {
            before_seconds: MAX_WINDOW_SECONDS + 1,
            after_seconds: 60,
        };
        assert!(!too_long.is_valid());
        assert_eq!(too_long.clamped().before_seconds, MAX_WINDOW_SECONDS);
        let zero = IncidentWindow {
            before_seconds: 0,
            after_seconds: 0,
        };
        assert!(!zero.is_valid());
        assert_eq!(zero.clamped().after_seconds, MIN_WINDOW_SECONDS);
        assert!(IncidentWindow::default().is_valid());
    }

    #[test]
    fn an_incident_round_trips_through_json() {
        let marked = incident();
        let encoded = serde_json::to_string(&marked).unwrap();
        assert_eq!(serde_json::from_str::<Incident>(&encoded).unwrap(), marked);
    }
}

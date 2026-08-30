//! In-memory retention and the export redaction boundary.
//!
//! The durable time-series store is a separate decision that needs an ADR and
//! benchmarks, so this stays an in-memory holder with a stable shape. What it
//! must not lose is the observation state: an export that turned an
//! unsupported metric into a zero would let an analyst conclude the machine
//! was idle.

use crate::MonitorError;
use crate::collector::CollectorReport;
use crate::observation::ObservationState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Incident {
    pub timestamp_unix_ms: u64,
    pub title: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InventoryItem {
    pub key: String,
    pub value: String,
    pub sensitive: bool,
}

/// How many observations of each state a metric produced over the retained
/// window. This is what makes an observation gap visible instead of implied.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoverageCounts {
    pub value: u32,
    pub stale: u32,
    pub unknown: u32,
    pub unsupported: u32,
    pub permission_denied: u32,
}

impl CoverageCounts {
    fn record(&mut self, state: ObservationState) {
        match state {
            ObservationState::Value => self.value += 1,
            ObservationState::Stale => self.stale += 1,
            ObservationState::Unknown => self.unknown += 1,
            ObservationState::Unsupported => self.unsupported += 1,
            ObservationState::PermissionDenied => self.permission_denied += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.value + self.stale + self.unknown + self.unsupported + self.permission_denied
    }
}

#[derive(Clone, Debug, Default)]
pub struct MonitorStore {
    reports: Vec<CollectorReport>,
    incidents: Vec<Incident>,
    inventory: Vec<InventoryItem>,
}

impl MonitorStore {
    pub fn record_report(&mut self, report: CollectorReport) {
        self.reports.push(report);
    }

    pub fn record_incident(&mut self, incident: Incident) {
        self.incidents.push(incident);
    }

    pub fn add_inventory(&mut self, item: InventoryItem) {
        self.inventory.push(item);
    }

    pub fn reports(&self) -> &[CollectorReport] {
        &self.reports
    }

    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    /// Per-metric observation coverage across everything retained.
    pub fn coverage(&self) -> BTreeMap<String, CoverageCounts> {
        let mut coverage: BTreeMap<String, CoverageCounts> = BTreeMap::new();
        for report in &self.reports {
            for (_, id, observation) in report.observations() {
                coverage
                    .entry(id.to_string())
                    .or_default()
                    .record(observation.state());
            }
        }
        coverage
    }

    /// A redacted export. Sensitive inventory values are replaced rather than
    /// dropped, so the reader can see that something was withheld.
    pub fn export_redacted(&self) -> Result<String, MonitorError> {
        let inventory = self
            .inventory
            .iter()
            .map(|item| {
                let value = if item.sensitive {
                    "[REDACTED]".to_string()
                } else {
                    item.value.clone()
                };
                (item.key.clone(), value)
            })
            .collect::<BTreeMap<_, _>>();
        serde_json::to_string(&serde_json::json!({
            "reports": self.reports,
            "incidents": self.incidents,
            "inventory": inventory,
            "coverage": self.coverage(),
        }))
        .map_err(|error| MonitorError::Export(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::{CollectorId, Entity, EntityId, EntityKind, Timestamp};
    use crate::metric::MetricId;
    use crate::observation::{MetricSet, Observation, UnsupportedReason};

    fn metric(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn report() -> CollectorReport {
        let mut report = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: 1,
                monotonic_ns: 0,
            },
        );
        report
            .metrics
            .insert(metric("cpu.utilization.busy"), Observation::float(0.0));
        let mut cpu0 = MetricSet::new();
        cpu0.insert(
            metric("cpu.temperature"),
            Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "k10temp exposes Tctl only".into(),
            }),
        );
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::LogicalCpu, "0"),
            cpu0,
        ));
        report
    }

    #[test]
    fn stores_reports_and_redacts_sensitive_inventory() {
        let mut store = MonitorStore::default();
        store.record_report(report());
        store.add_inventory(InventoryItem {
            key: "command".into(),
            value: "--token secret".into(),
            sensitive: true,
        });
        let export = store.export_redacted().unwrap();
        assert_eq!(store.reports().len(), 1);
        assert!(export.contains("[REDACTED]"));
        assert!(!export.contains("secret"));
    }

    #[test]
    fn an_export_keeps_a_measured_zero_apart_from_an_unsupported_metric() {
        let mut store = MonitorStore::default();
        store.record_report(report());
        let export = store.export_redacted().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&export).unwrap();
        let busy = &parsed["reports"][0]["metrics"]["cpu.utilization.busy"];
        let temperature = &parsed["reports"][0]["entities"][0]["metrics"]["cpu.temperature"];
        assert_eq!(busy["value"]["float"], 0.0);
        assert!(temperature.get("value").is_none());
        assert!(temperature["unsupported"]["not_reported"].is_object());
    }

    #[test]
    fn coverage_counts_every_observation_state_it_retained() {
        let mut store = MonitorStore::default();
        store.record_report(report());
        store.record_report(report());
        let coverage = store.coverage();
        assert_eq!(coverage["cpu.utilization.busy"].value, 2);
        assert_eq!(coverage["cpu.temperature"].unsupported, 2);
        assert_eq!(coverage["cpu.temperature"].value, 0);
        assert_eq!(coverage["cpu.temperature"].total(), 2);
    }

    #[test]
    fn incidents_are_kept_alongside_the_reports_that_explain_them() {
        let mut store = MonitorStore::default();
        store.record_incident(Incident {
            timestamp_unix_ms: 42,
            title: "The system was just slow".into(),
            note: None,
        });
        assert_eq!(store.incidents().len(), 1);
        assert!(store.export_redacted().unwrap().contains("just slow"));
    }
}

//! Observation contracts for Better Monitor.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub timestamp_unix_ms: u64,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub psi_some_percent: Option<f32>,
}

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

pub trait Collector {
    fn collect(&mut self) -> Result<Sample, MonitorError>;
}

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("collector failed: {0}")]
    Collector(String),
    #[error("export failed: {0}")]
    Export(String),
}

#[derive(Clone, Debug, Default)]
pub struct MonitorStore {
    samples: Vec<Sample>,
    incidents: Vec<Incident>,
    inventory: Vec<InventoryItem>,
}

impl MonitorStore {
    pub fn record_sample(&mut self, sample: Sample) {
        self.samples.push(sample);
    }

    pub fn record_incident(&mut self, incident: Incident) {
        self.incidents.push(incident);
    }

    pub fn add_inventory(&mut self, item: InventoryItem) {
        self.inventory.push(item);
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

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
            "samples": self.samples,
            "incidents": self.incidents,
            "inventory": inventory,
        }))
        .map_err(|error| MonitorError::Export(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_samples_and_redacts_sensitive_inventory() {
        let mut store = MonitorStore::default();
        store.record_sample(Sample {
            timestamp_unix_ms: 1,
            cpu_percent: 10.0,
            memory_percent: 20.0,
            psi_some_percent: None,
        });
        store.add_inventory(InventoryItem {
            key: "command".into(),
            value: "--token secret".into(),
            sensitive: true,
        });
        let export = store.export_redacted().unwrap();
        assert_eq!(store.samples().len(), 1);
        assert!(export.contains("[REDACTED]"));
        assert!(!export.contains("secret"));
    }
}

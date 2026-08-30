//! Network interfaces.
//!
//! Upstream interfaces: `/proc/net/dev` for counters, documented in the
//! kernel's `Documentation/filesystems/proc.rst`, and `/sys/class/net/*` for
//! link attributes, documented in `Documentation/ABI/testing/sysfs-class-net`.
//!
//! `/sys/class/net` is the authority on which interfaces exist, because it is
//! the one that also carries the link state, and `/proc/net/dev` supplies the
//! counters. An interface in one and not the other keeps whichever half is
//! real and reports the other half as unknown rather than as zero traffic.
//!
//! Two parsing details matter. `/proc/net/dev` pads the interface name into a
//! fixed column, so `tailscale0:    1692` has no space before its first value
//! and the line must be split on the colon rather than on whitespace. And
//! `speed` is only meaningful for a link that has one: a wireless or virtual
//! interface returns `EINVAL` on read, and a down Ethernet link returns `-1`,
//! both of which are "not reported" rather than a speed of zero.
//!
//! Per-process network attribution is deliberately absent. The specification
//! defers it until a trustworthy mechanism is chosen, and dividing interface
//! traffic among processes by name would be a guess presented as data.

use crate::catalog::{
    MINIMUM_DELTA_INTERVAL, collector_id, counter, errors, gauge, identity, metric_id, proc_source,
    rate, sys_source,
};
use crate::fsread::{MalformedInput, field_u64, list_dir, read_attribute, read_text};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::BTreeMap;

/// The sixteen counters of one `/proc/net/dev` line.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterfaceCounters {
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    pub rx_fifo: u64,
    pub rx_frame: u64,
    pub rx_compressed: u64,
    pub rx_multicast: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
    pub tx_fifo: u64,
    pub tx_collisions: u64,
    pub tx_carrier: u64,
    pub tx_compressed: u64,
}

const PROC_NET_DEV: &str = "/proc/net/dev";

/// Parse `/proc/net/dev`, keyed by interface name.
pub fn parse_net_dev(input: &str) -> Result<BTreeMap<String, InterfaceCounters>, MalformedInput> {
    let mut interfaces = BTreeMap::new();
    for line in input.lines() {
        // The first two lines are the two-row column header, and neither
        // contains a colon followed by numbers.
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 16 {
            return Err(MalformedInput::new(
                PROC_NET_DEV,
                format!("{name} has {} of 16 counters", fields.len()),
            ));
        }
        let at = |index: usize| field_u64(PROC_NET_DEV, &fields, index);
        interfaces.insert(
            name.to_string(),
            InterfaceCounters {
                rx_bytes: at(0)?,
                rx_packets: at(1)?,
                rx_errors: at(2)?,
                rx_dropped: at(3)?,
                rx_fifo: at(4)?,
                rx_frame: at(5)?,
                rx_compressed: at(6)?,
                rx_multicast: at(7)?,
                tx_bytes: at(8)?,
                tx_packets: at(9)?,
                tx_errors: at(10)?,
                tx_dropped: at(11)?,
                tx_fifo: at(12)?,
                tx_collisions: at(13)?,
                tx_carrier: at(14)?,
                tx_compressed: at(15)?,
            },
        );
    }
    if interfaces.is_empty() {
        return Err(MalformedInput::new(PROC_NET_DEV, "no interface lines"));
    }
    Ok(interfaces)
}

/// The `ARPHRD_*` values a desktop actually meets, from the kernel's
/// `include/uapi/linux/if_arp.h`.
fn link_type_name(arp_type: u64, wireless: bool) -> &'static str {
    match arp_type {
        // Wireless netdevs present themselves as Ethernet, so the `wireless`
        // directory is what separates them.
        1 if wireless => "wifi",
        1 => "ethernet",
        24 => "firewire",
        512 => "ppp",
        768 => "tunnel",
        772 => "loopback",
        776 => "sit",
        778 => "gre",
        801 => "wifi",
        823 => "ieee802154",
        65534 => "tun",
        65535 => "void",
        _ => "unknown",
    }
}

const NETWORK_COLLECTOR: &str = "linux.network";

/// `(metric, receive extractor, transmit extractor)` for the counters that
/// become rates.
type CounterExtractor = fn(&InterfaceCounters) -> u64;
const RATE_COUNTERS: [(&str, CounterExtractor); 8] = [
    ("network.rx.bytes.rate", |c| c.rx_bytes),
    ("network.tx.bytes.rate", |c| c.tx_bytes),
    ("network.rx.packets.rate", |c| c.rx_packets),
    ("network.tx.packets.rate", |c| c.tx_packets),
    ("network.rx.errors.rate", |c| c.rx_errors),
    ("network.tx.errors.rate", |c| c.tx_errors),
    ("network.rx.drops.rate", |c| c.rx_dropped),
    ("network.tx.drops.rate", |c| c.tx_dropped),
];

const TOTAL_COUNTERS: [(&str, CounterExtractor); 4] = [
    ("network.rx.bytes.total", |c| c.rx_bytes),
    ("network.tx.bytes.total", |c| c.tx_bytes),
    ("network.rx.packets.total", |c| c.rx_packets),
    ("network.tx.packets.total", |c| c.tx_packets),
];

struct NetworkSnapshot {
    at: Timestamp,
    interfaces: BTreeMap<String, InterfaceCounters>,
}

/// Per-interface throughput, errors, and link state.
pub struct NetworkCollector {
    roots: Roots,
    previous: Option<NetworkSnapshot>,
}

impl NetworkCollector {
    pub fn new(roots: Roots) -> Self {
        Self {
            roots,
            previous: None,
        }
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        let mut descriptors = vec![
            rate(
                "network.rx.bytes.rate",
                Unit::BytesPerSecond,
                proc_source("net/dev"),
                "bytes received per second",
            ),
            rate(
                "network.tx.bytes.rate",
                Unit::BytesPerSecond,
                proc_source("net/dev"),
                "bytes transmitted per second",
            ),
            rate(
                "network.rx.packets.rate",
                Unit::CountPerSecond,
                proc_source("net/dev"),
                "packets received per second",
            ),
            rate(
                "network.tx.packets.rate",
                Unit::CountPerSecond,
                proc_source("net/dev"),
                "packets transmitted per second",
            ),
            errors(
                "network.rx.errors.rate",
                proc_source("net/dev"),
                "receive errors per second",
            ),
            errors(
                "network.tx.errors.rate",
                proc_source("net/dev"),
                "transmit errors per second",
            ),
            errors(
                "network.rx.drops.rate",
                proc_source("net/dev"),
                "received packets dropped per second",
            ),
            errors(
                "network.tx.drops.rate",
                proc_source("net/dev"),
                "outgoing packets dropped per second",
            ),
        ];
        descriptors.extend(TOTAL_COUNTERS.iter().map(|(metric, _)| {
            counter(
                metric,
                if metric.contains("bytes") {
                    Unit::Bytes
                } else {
                    Unit::Count
                },
                proc_source("net/dev"),
                "counter since the interface came up",
            )
        }));
        descriptors.extend([
            gauge(
                "network.link.speed",
                Unit::BitsPerSecond,
                sys_source("class/net/{interface}/speed"),
                "negotiated link speed, converted from the megabits sysfs reports",
            ),
            gauge(
                "network.link.mtu",
                Unit::Bytes,
                sys_source("class/net/{interface}/mtu"),
                "maximum transmission unit",
            ),
            gauge(
                "network.link.carrier",
                Unit::None,
                sys_source("class/net/{interface}/carrier"),
                "whether the interface has a physical link",
            ),
            identity(
                "network.link.state",
                sys_source("class/net/{interface}/operstate"),
                "RFC 2863 operational state",
            ),
            identity(
                "network.link.type",
                sys_source("class/net/{interface}/type"),
                "link type from ARPHRD, with wireless separated from Ethernet",
            ),
        ]);
        descriptors
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(NETWORK_COLLECTOR), at);
        let counters = match read_text(&roots.proc("net/dev")).map(|raw| parse_net_dev(&raw)) {
            Ok(Ok(counters)) => Some(counters),
            Ok(Err(error)) => {
                report.health = CollectorHealth::Degraded {
                    detail: format!("{}: {}", error.context, error.detail),
                };
                None
            }
            Err(error) => {
                report.health = CollectorHealth::Degraded {
                    detail: format!("{} unreadable", error.path().display()),
                };
                None
            }
        };

        let sys_root = roots.sys("class/net");
        let names: Vec<String> = match list_dir(&sys_root) {
            Ok(entries) => entries
                .iter()
                .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
                .collect(),
            Err(error) => {
                // Without /sys/class/net the counters are still real; only the
                // link attributes are lost.
                report.health = CollectorHealth::Degraded {
                    detail: format!("{} unreadable", error.path().display()),
                };
                counters
                    .as_ref()
                    .map(|counters| counters.keys().cloned().collect())
                    .unwrap_or_default()
            }
        };

        if names.is_empty() && counters.is_none() {
            report.health = CollectorHealth::Failed {
                detail: "neither /sys/class/net nor /proc/net/dev is readable".into(),
            };
            return report;
        }

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at))
            .filter(|seconds| *seconds >= MINIMUM_DELTA_INTERVAL.as_secs_f64());

        for name in &names {
            let mut metrics = MetricSet::new();
            let later = counters.as_ref().and_then(|counters| counters.get(name));
            let earlier = self
                .previous
                .as_ref()
                .and_then(|previous| previous.interfaces.get(name));
            report_counters(later, earlier, seconds, &mut metrics);
            read_link_attributes(roots, name, &mut metrics);
            report.entities.push(Entity::new(
                EntityId::new(EntityKind::NetworkInterface, name.clone()),
                metrics,
            ));
        }

        self.previous = Some(NetworkSnapshot {
            at,
            interfaces: counters.unwrap_or_default(),
        });
        report
    }
}

fn report_counters(
    later: Option<&InterfaceCounters>,
    earlier: Option<&InterfaceCounters>,
    seconds: Option<f64>,
    metrics: &mut MetricSet,
) {
    let Some(later) = later else {
        // The interface exists in sysfs but /proc/net/dev has no line for it.
        let missing = Observation::Unknown(UnknownReason::ReadFailed {
            detail: "no /proc/net/dev line for this interface".into(),
        });
        for (metric, _) in RATE_COUNTERS.iter().chain(TOTAL_COUNTERS.iter()) {
            metrics.insert(metric_id(metric), missing.clone());
        }
        return;
    };
    for (metric, extract) in TOTAL_COUNTERS {
        metrics.insert(metric_id(metric), Observation::unsigned(extract(later)));
    }
    for (metric, extract) in RATE_COUNTERS {
        let observation = match earlier.zip(seconds) {
            Some((earlier, seconds)) => {
                Observation::float(extract(later).saturating_sub(extract(earlier)) as f64 / seconds)
            }
            None if earlier.is_none() => Observation::Unknown(UnknownReason::NotYetSampled),
            None => Observation::Unknown(UnknownReason::IntervalTooShort),
        };
        metrics.insert(metric_id(metric), observation);
    }
}

fn read_link_attributes(roots: &Roots, name: &str, metrics: &mut MetricSet) {
    let base = format!("class/net/{name}");

    // A link with no negotiated speed reads as EINVAL or -1. Both mean the
    // driver does not know, not that the link runs at zero.
    let speed_path = roots.sys(&format!("{base}/speed"));
    let speed = match read_attribute(&speed_path) {
        Ok(raw) => match raw.parse::<i64>() {
            Ok(megabits) if megabits > 0 => Observation::unsigned(megabits as u64 * 1_000_000),
            _ => Observation::Unsupported(UnsupportedReason::NotReported {
                detail: format!("speed reads {raw:?}"),
            }),
        },
        Err(error) => Observation::Unsupported(UnsupportedReason::NotReported {
            detail: format!("{} is not readable for this link", error.path().display()),
        }),
    };
    metrics.insert(metric_id("network.link.speed"), speed);

    let mtu_path = roots.sys(&format!("{base}/mtu"));
    metrics.insert(
        metric_id("network.link.mtu"),
        match read_attribute(&mtu_path).map(|raw| raw.parse::<u64>()) {
            Ok(Ok(mtu)) => Observation::unsigned(mtu),
            Ok(Err(_)) => Observation::Unknown(UnknownReason::Malformed {
                detail: "mtu is not an unsigned integer".into(),
            }),
            Err(error) => error.into_observation(),
        },
    );

    let carrier_path = roots.sys(&format!("{base}/carrier"));
    metrics.insert(
        metric_id("network.link.carrier"),
        match read_attribute(&carrier_path).map(|raw| raw.parse::<u64>()) {
            Ok(Ok(carrier)) => Observation::boolean(carrier != 0),
            // A down interface makes `carrier` return EINVAL rather than 0.
            Ok(Err(_)) => Observation::Unsupported(UnsupportedReason::NotReported {
                detail: "carrier is only readable while the interface is up".into(),
            }),
            Err(error) => error.into_observation(),
        },
    );

    let state_path = roots.sys(&format!("{base}/operstate"));
    metrics.insert(
        metric_id("network.link.state"),
        match read_attribute(&state_path) {
            Ok(state) => Observation::text(state),
            Err(error) => error.into_observation(),
        },
    );

    let type_path = roots.sys(&format!("{base}/type"));
    let wireless = roots.sys(&format!("{base}/wireless")).exists()
        || roots.sys(&format!("{base}/phy80211")).exists();
    metrics.insert(
        metric_id("network.link.type"),
        match read_attribute(&type_path).map(|raw| raw.parse::<u64>()) {
            Ok(Ok(arp_type)) => Observation::text(link_type_name(arp_type, wireless)),
            Ok(Err(_)) => Observation::Unknown(UnknownReason::Malformed {
                detail: "type is not an unsigned integer".into(),
            }),
            Err(error) => error.into_observation(),
        },
    );
}

impl Collector for NetworkCollector {
    fn id(&self) -> CollectorId {
        collector_id(NETWORK_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        NetworkCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample(&roots, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempTree, at, fixture};
    use monitor_core::ObservationState;

    fn interface<'a>(report: &'a CollectorReport, name: &str) -> &'a Entity {
        report
            .entities
            .iter()
            .find(|entity| entity.id.key == name)
            .unwrap_or_else(|| panic!("no interface {name} in the report"))
    }

    #[test]
    fn parses_a_captured_net_dev_including_a_name_that_fills_its_column() {
        // `tailscale0:` leaves no space before its first counter, which a
        // whitespace split would fold into the name.
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/net/dev")).unwrap();
        let interfaces = parse_net_dev(&raw).unwrap();
        assert_eq!(interfaces.len(), 4);
        assert_eq!(interfaces["tailscale0"].rx_bytes, 1692);
        assert_eq!(interfaces["tailscale0"].tx_packets, 157);
        assert_eq!(interfaces["wlp98s0"].rx_bytes, 2_205_098_804);
        assert_eq!(interfaces["wlp98s0"].rx_dropped, 273);
        assert_eq!(interfaces["virbr0"].tx_dropped, 104);
    }

    #[test]
    fn the_two_header_rows_are_not_mistaken_for_interfaces() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/net/dev")).unwrap();
        let interfaces = parse_net_dev(&raw).unwrap();
        assert!(!interfaces.contains_key("Inter-|"));
        assert!(!interfaces.contains_key("face"));
    }

    #[test]
    fn a_truncated_net_dev_line_is_malformed() {
        let raw = std::fs::read_to_string(fixture("truncated").join("proc/net/dev")).unwrap();
        let error = parse_net_dev(&raw).unwrap_err();
        assert!(error.detail.contains("of 16 counters"));
    }

    #[test]
    fn a_malformed_net_dev_counter_is_rejected() {
        let raw = std::fs::read_to_string(fixture("malformed").join("proc/net/dev")).unwrap();
        let error = parse_net_dev(&raw).unwrap_err();
        assert!(error.detail.contains("not a number"));
    }

    #[test]
    fn two_samples_produce_the_throughput_the_counter_deltas_imply() {
        // eth0 receives 4000 bytes and sends 2000 over one second.
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = NetworkCollector::new(a.clone());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let eth0 = interface(&report, "eth0");
        assert_eq!(
            eth0.metrics
                .get(&metric_id("network.rx.bytes.rate"))
                .unwrap()
                .as_f64(),
            Some(4000.0)
        );
        assert_eq!(
            eth0.metrics
                .get(&metric_id("network.tx.bytes.rate"))
                .unwrap()
                .as_f64(),
            Some(2000.0)
        );
        assert_eq!(
            eth0.metrics
                .get(&metric_id("network.tx.errors.rate"))
                .unwrap()
                .as_f64(),
            Some(2.0)
        );
        // Drops did not change: a real zero, not a missing reading.
        let drops = eth0
            .metrics
            .get(&metric_id("network.tx.drops.rate"))
            .unwrap();
        assert_eq!(drops.state(), ObservationState::Value);
        assert_eq!(drops.as_f64(), Some(0.0));
    }

    #[test]
    fn totals_are_available_on_the_first_round_even_though_rates_are_not() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = NetworkCollector::new(a.clone());
        let report = collector.sample(&a, at(0));
        let eth0 = interface(&report, "eth0");
        assert_eq!(
            eth0.metrics
                .get(&metric_id("network.rx.bytes.total"))
                .unwrap()
                .as_f64(),
            Some(1000.0)
        );
        assert_eq!(
            eth0.metrics.state_of(&metric_id("network.rx.bytes.rate")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn a_link_speed_in_megabits_becomes_bits_per_second() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = NetworkCollector::new(a.clone());
        let report = collector.sample(&a, at(0));
        assert_eq!(
            interface(&report, "eth0")
                .metrics
                .get(&metric_id("network.link.speed"))
                .unwrap()
                .as_f64(),
            Some(1_000_000_000.0)
        );
    }

    #[test]
    fn a_link_with_no_speed_is_unsupported_rather_than_zero() {
        // The captured wireless interface has no readable `speed`.
        let roots = Roots::at(fixture("snapshot-a"));
        let mut collector = NetworkCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            interface(&report, "wlp98s0")
                .metrics
                .state_of(&metric_id("network.link.speed")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_speed_of_minus_one_is_a_driver_saying_it_does_not_know() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(temporary.path().join("sys/class/net/eth0/speed"), "-1\n").unwrap();
        let mut collector = NetworkCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert_eq!(
            interface(&report, "eth0")
                .metrics
                .state_of(&metric_id("network.link.speed")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_wireless_interface_is_not_reported_as_ethernet() {
        // Wireless netdevs report ARPHRD_ETHER; only the phy80211 link tells
        // them apart.
        let roots = Roots::at(fixture("snapshot-a"));
        let mut collector = NetworkCollector::new(roots.clone());
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            interface(&report, "wlp98s0")
                .metrics
                .get(&metric_id("network.link.type"))
                .unwrap()
                .as_text(),
            Some("wifi")
        );
        assert_eq!(
            interface(&report, "lo")
                .metrics
                .get(&metric_id("network.link.type"))
                .unwrap()
                .as_text(),
            Some("loopback")
        );
        assert_eq!(
            interface(&report, "tailscale0")
                .metrics
                .get(&metric_id("network.link.type"))
                .unwrap()
                .as_text(),
            Some("tun")
        );
    }

    #[test]
    fn every_sysfs_interface_appears_even_without_a_proc_line() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::create_dir_all(temporary.path().join("sys/class/net/eth9")).unwrap();
        std::fs::write(temporary.path().join("sys/class/net/eth9/type"), "1\n").unwrap();
        let mut collector = NetworkCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        let eth9 = interface(&report, "eth9");
        assert_eq!(
            eth9.metrics.state_of(&metric_id("network.rx.bytes.total")),
            ObservationState::Unknown
        );
        assert_eq!(
            eth9.metrics
                .get(&metric_id("network.link.type"))
                .unwrap()
                .as_text(),
            Some("ethernet")
        );
    }

    #[test]
    fn losing_sysfs_degrades_the_collector_but_keeps_the_counters() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::remove_dir_all(temporary.path().join("sys/class/net")).unwrap();
        let mut collector = NetworkCollector::new(temporary.roots());
        let report = collector.sample(&temporary.roots(), at(0));
        assert!(matches!(report.health, CollectorHealth::Degraded { .. }));
        assert_eq!(
            interface(&report, "eth0")
                .metrics
                .get(&metric_id("network.rx.bytes.total"))
                .unwrap()
                .as_f64(),
            Some(1000.0)
        );
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = NetworkCollector::descriptors();
        let mut seen = std::collections::BTreeSet::new();
        for descriptor in &descriptors {
            assert!(
                seen.insert(descriptor.id.clone()),
                "duplicate metric {}",
                descriptor.id
            );
        }
        assert_eq!(seen.len(), descriptors.len());
    }
}

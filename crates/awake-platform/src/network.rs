//! Network throughput and interface state, from `/proc/net/dev` and
//! `/sys/class/net`.
//!
//! Throughput, like CPU utilization, is a rate between two samples, so the first
//! call after startup reports nothing rather than a zero. The loopback interface
//! is excluded from the total: a rule meant to keep the machine awake while a
//! download runs must not be held open by two local processes talking to each
//! other.

use std::collections::BTreeMap;
use std::path::PathBuf;

use awake_core::{Observations, ProviderKind};

use crate::provider::{Cadence, NETWORK_POLL_SECONDS, TriggerProvider};
use crate::roots::{ReadError, Roots, list_dir, read_attribute, read_text};

/// Bytes seen on one interface since boot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterfaceBytes {
    pub received: u64,
    pub transmitted: u64,
}

impl InterfaceBytes {
    fn total(&self) -> u64 {
        self.received.saturating_add(self.transmitted)
    }
}

/// Parses `/proc/net/dev` into per-interface byte counters.
///
/// The format is two header lines and then `name: rx_bytes rx_packets ...`,
/// where the colon may or may not have a space after it depending on how wide
/// the name is. Splitting on the colon rather than on whitespace is what makes a
/// long interface name parse the same as a short one.
pub fn parse_net_dev(text: &str) -> BTreeMap<String, InterfaceBytes> {
    let mut interfaces = BTreeMap::new();
    for line in text.lines().skip(2) {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let fields: Vec<&str> = counters.split_whitespace().collect();
        // Sixteen counters: eight receive then eight transmit. Bytes are the
        // first of each group.
        if fields.len() < 9 {
            continue;
        }
        let Ok(received) = fields[0].parse::<u64>() else {
            continue;
        };
        let Ok(transmitted) = fields[8].parse::<u64>() else {
            continue;
        };
        interfaces.insert(
            name.trim().to_string(),
            InterfaceBytes {
                received,
                transmitted,
            },
        );
    }
    interfaces
}

/// Whether an interface's traffic counts toward the machine's throughput.
///
/// Loopback never does. A virtual bridge does, because a virtual machine pulling
/// a disk image over `virbr0` is real work the user may well want the machine
/// awake for, and excluding it would be a guess about their intent.
fn counts_toward_throughput(name: &str) -> bool {
    name != "lo"
}

/// Reads throughput and which interfaces are up.
#[derive(Clone, Debug)]
pub struct NetworkProvider {
    roots: Roots,
    kind: ProviderKind,
    previous: Option<(u64, u64)>,
}

impl NetworkProvider {
    /// The throughput half.
    pub fn throughput(roots: Roots) -> Self {
        Self {
            roots,
            kind: ProviderKind::NetworkThroughput,
            previous: None,
        }
    }

    /// The "is this interface up" half.
    pub fn interfaces(roots: Roots) -> Self {
        Self {
            roots,
            kind: ProviderKind::NetworkInterface,
            previous: None,
        }
    }

    fn net_dev(&self) -> PathBuf {
        self.roots.proc_path("net/dev")
    }

    /// Total bytes across every interface that counts.
    pub fn total_bytes(&self) -> Result<u64, ReadError> {
        let text = read_text(&self.net_dev())?;
        Ok(parse_net_dev(&text)
            .iter()
            .filter(|(name, _)| counts_toward_throughput(name))
            .map(|(_, bytes)| bytes.total())
            .fold(0u64, u64::saturating_add))
    }

    /// Every interface whose `operstate` says it is carrying traffic.
    ///
    /// `unknown` counts as up: it is what the kernel reports for interfaces with
    /// no carrier concept, including the WireGuard and Tailscale devices a
    /// "keep awake while the VPN is up" rule is most likely to name.
    pub fn up_interfaces(&self) -> Result<Vec<String>, ReadError> {
        let entries = list_dir(&self.roots.sys_path("class/net"))?;
        let mut up = Vec::new();
        for entry in entries {
            let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(state) = read_attribute(&entry.join("operstate")) else {
                continue;
            };
            if state == "up" || state == "unknown" {
                up.push(name.to_string());
            }
        }
        up.sort();
        Ok(up)
    }
}

impl TriggerProvider for NetworkProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn cadence(&self) -> Cadence {
        Cadence::Poll {
            seconds: NETWORK_POLL_SECONDS,
        }
    }

    fn sample(&mut self, now_unix_seconds: u64, into: &mut Observations) {
        match self.kind {
            ProviderKind::NetworkInterface => match self.up_interfaces() {
                Ok(up) => {
                    into.interfaces_up = Some(up);
                    into.mark_available(ProviderKind::NetworkInterface);
                }
                Err(error) => {
                    into.mark_unavailable(ProviderKind::NetworkInterface, error.explanation())
                }
            },
            ProviderKind::NetworkThroughput => {
                let total = match self.total_bytes() {
                    Ok(total) => total,
                    Err(error) => {
                        into.mark_unavailable(ProviderKind::NetworkThroughput, error.explanation());
                        return;
                    }
                };
                match self.previous.replace((now_unix_seconds, total)) {
                    None => into.mark_unavailable(
                        ProviderKind::NetworkThroughput,
                        "awake.provider.awaiting_second_sample",
                    ),
                    Some((then, previous_total)) => {
                        let elapsed = now_unix_seconds.saturating_sub(then);
                        // Two samples in the same second give no rate, and a
                        // counter that went backwards means the interface was
                        // recreated. Neither is zero traffic.
                        match (elapsed, total.checked_sub(previous_total)) {
                            (0, _) | (_, None) => into.mark_unavailable(
                                ProviderKind::NetworkThroughput,
                                "awake.provider.counters_did_not_advance",
                            ),
                            (elapsed, Some(delta)) => {
                                into.network_kibibytes_per_second = Some(delta / 1_024 / elapsed);
                                into.mark_available(ProviderKind::NetworkThroughput);
                            }
                        }
                    }
                }
            }
            other => into.mark_unavailable(other, "awake.provider.wrong_provider"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Inter-|   Receive                                                |  Transmit\n \
         face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n";

    fn fixture(net_dev: &str) -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc/net");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(proc.join("dev"), net_dev).unwrap();
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn line(name: &str, received: u64, transmitted: u64) -> String {
        format!("{name:>7}: {received} 0 0 0 0 0 0 0 {transmitted} 0 0 0 0 0 0 0\n")
    }

    fn write_net_dev(roots: &Roots, body: &str) {
        std::fs::write(roots.proc_path("net/dev"), format!("{HEADER}{body}")).unwrap();
    }

    #[test]
    fn throughput_is_a_rate_between_two_samples() {
        let (_directory, roots) =
            fixture(&format!("{HEADER}{}", line("wlp98s0", 1_000_000, 500_000)));
        let mut provider = NetworkProvider::throughput(roots.clone());

        let mut first = Observations::at(1_000);
        provider.sample(1_000, &mut first);
        assert_eq!(first.network_kibibytes_per_second, None);

        // Ten mebibytes more across five seconds is two mebibytes a second.
        write_net_dev(
            &roots,
            &line("wlp98s0", 1_000_000 + 10 * 1_024 * 1_024, 500_000),
        );
        let mut second = Observations::at(1_005);
        provider.sample(1_005, &mut second);
        assert_eq!(second.network_kibibytes_per_second, Some(2 * 1_024));
    }

    #[test]
    fn loopback_traffic_never_keeps_the_machine_awake() {
        let (_directory, roots) = fixture(&format!("{HEADER}{}", line("lo", 0, 0)));
        let mut provider = NetworkProvider::throughput(roots.clone());
        provider.sample(1_000, &mut Observations::at(1_000));

        write_net_dev(
            &roots,
            &line("lo", 100 * 1_024 * 1_024, 100 * 1_024 * 1_024),
        );
        let mut observations = Observations::at(1_005);
        provider.sample(1_005, &mut observations);
        assert_eq!(
            observations.network_kibibytes_per_second,
            Some(0),
            "two local processes talking to each other is not a download"
        );
    }

    #[test]
    fn a_long_interface_name_with_no_space_after_the_colon_still_parses() {
        let text = format!("{HEADER}enp0s31f6xyz:1000 0 0 0 0 0 0 0 2000 0 0 0 0 0 0 0\n");
        let parsed = parse_net_dev(&text);
        assert_eq!(
            parsed.get("enp0s31f6xyz"),
            Some(&InterfaceBytes {
                received: 1_000,
                transmitted: 2_000
            })
        );
    }

    #[test]
    fn a_truncated_line_is_skipped_rather_than_corrupting_the_total() {
        let text = format!("{HEADER}  eth0: 1 2 3\n{}", line("wlan0", 500, 500));
        let parsed = parse_net_dev(&text);
        assert!(!parsed.contains_key("eth0"));
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn an_interface_that_was_recreated_reports_unknown_rather_than_negative_traffic() {
        let (_directory, roots) = fixture(&format!("{HEADER}{}", line("wlp98s0", 5_000_000, 0)));
        let mut provider = NetworkProvider::throughput(roots.clone());
        provider.sample(1_000, &mut Observations::at(1_000));

        write_net_dev(&roots, &line("wlp98s0", 10, 0));
        let mut observations = Observations::at(1_005);
        provider.sample(1_005, &mut observations);
        assert_eq!(observations.network_kibibytes_per_second, None);
        assert_eq!(
            observations
                .availability_of(ProviderKind::NetworkThroughput)
                .explanation(),
            Some("awake.provider.counters_did_not_advance")
        );
    }

    #[test]
    fn an_interface_reporting_unknown_carrier_counts_as_up_because_vpns_do_that() {
        let directory = tempfile::tempdir().unwrap();
        let net = directory.path().join("sys/class/net");
        for (name, state) in [
            ("lo", "unknown"),
            ("wlp98s0", "up"),
            ("tailscale0", "unknown"),
            ("virbr0", "down"),
        ] {
            std::fs::create_dir_all(net.join(name)).unwrap();
            std::fs::write(net.join(name).join("operstate"), format!("{state}\n")).unwrap();
        }

        let mut observations = Observations::at(1_000);
        NetworkProvider::interfaces(Roots::at(directory.path())).sample(1_000, &mut observations);
        assert_eq!(
            observations.interfaces_up,
            Some(vec![
                "lo".to_string(),
                "tailscale0".to_string(),
                "wlp98s0".to_string()
            ]),
            "a WireGuard or Tailscale device has no carrier concept and reports unknown"
        );
    }

    #[test]
    fn a_missing_proc_net_dev_names_the_path_rather_than_reporting_no_traffic() {
        let directory = tempfile::tempdir().unwrap();
        let mut provider = NetworkProvider::throughput(Roots::at(directory.path()));
        let mut observations = Observations::at(1_000);
        provider.sample(1_000, &mut observations);
        let explanation = observations
            .availability_of(ProviderKind::NetworkThroughput)
            .explanation()
            .unwrap()
            .to_string();
        assert!(explanation.contains("net/dev"), "{explanation}");
    }
}

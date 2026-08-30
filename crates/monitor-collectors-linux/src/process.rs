//! Processes.
//!
//! Upstream interface: `/proc/[pid]/stat`, `status`, `cmdline`, `cgroup`, and
//! `fd`, documented in the kernel's `Documentation/filesystems/proc.rst`,
//! chapter 3.
//!
//! Four things here are easy to get wrong and are handled deliberately.
//!
//! The second field of `/proc/[pid]/stat` is the executable name in
//! parentheses and may itself contain spaces and parentheses, so the line is
//! split at the *last* closing parenthesis rather than on whitespace.
//!
//! A PID is reused. Comparing CPU time against the previous round without
//! checking the process start time would attribute the old process's counters
//! to the new one, so a start time that changed resets the delta to
//! `NotYetSampled` instead of producing a fabricated spike.
//!
//! `/proc/[pid]/fd` is readable only for a process the caller owns, so the
//! descriptor count is `PermissionDenied` for other users' processes. It is
//! never zero, which would say the process has no open files.
//!
//! Command lines are withheld by default. The specification requires no
//! persistent command-line capture by default, and a command line routinely
//! contains tokens and personal paths. Withholding is reported as
//! `PolicyWithheld` so the reason is visible rather than looking like a
//! kernel that stayed silent.

use crate::catalog::{
    MINIMUM_DELTA_INTERVAL, collector_id, derived_source, gauge, identity, metric_id, proc_source,
    utilization,
};
use crate::cpu::{USER_HZ, parse_proc_stat};
use crate::fsread::{MalformedInput, ReadError, count_dir_entries, list_dir, read_text};
use crate::roots::Roots;
use monitor_core::{
    Collector, CollectorHealth, CollectorId, CollectorReport, Entity, EntityId, EntityKind,
    MetricDescriptor, MetricSet, Observation, Timestamp, Unit, UnknownReason, UnsupportedReason,
};
use std::collections::HashMap;

/// Whether collection may record process command lines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessPrivacy {
    /// Off by default. Turning it on is a user decision, and the resulting
    /// readings are still subject to export redaction.
    pub include_command_line: bool,
}

/// The fields of `/proc/[pid]/stat` Better Monitor uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessStat {
    pub pid: u32,
    /// The executable name the kernel puts in parentheses, without them.
    pub comm: String,
    pub state: char,
    pub parent_pid: u32,
    /// User-mode CPU time in USER_HZ ticks.
    pub user_ticks: u64,
    /// Kernel-mode CPU time in USER_HZ ticks.
    pub system_ticks: u64,
    pub priority: i64,
    pub nice: i64,
    pub threads: u64,
    /// Ticks after boot at which the process started.
    pub start_ticks: u64,
    pub virtual_bytes: u64,
}

const PID_STAT: &str = "/proc/[pid]/stat";

pub fn parse_pid_stat(input: &str) -> Result<ProcessStat, MalformedInput> {
    let open = input
        .find('(')
        .ok_or_else(|| MalformedInput::new(PID_STAT, "no comm field"))?;
    let close = input
        .rfind(')')
        .ok_or_else(|| MalformedInput::new(PID_STAT, "unterminated comm field"))?;
    if close < open {
        return Err(MalformedInput::new(
            PID_STAT,
            "comm parentheses are inverted",
        ));
    }
    let pid = input[..open]
        .trim()
        .parse::<u32>()
        .map_err(|_| MalformedInput::new(PID_STAT, format!("bad pid {:?}", &input[..open])))?;
    let comm = input[open + 1..close].to_string();
    let fields: Vec<&str> = input[close + 1..].split_whitespace().collect();
    // The remainder starts at field 3 (state), so field N is at index N - 3.
    if fields.len() < 22 {
        return Err(MalformedInput::new(
            PID_STAT,
            format!("{} fields after comm, need at least 22", fields.len()),
        ));
    }
    let unsigned = |index: usize| -> Result<u64, MalformedInput> {
        fields[index].parse::<u64>().map_err(|_| {
            MalformedInput::new(
                PID_STAT,
                format!("field {} is not unsigned: {:?}", index + 3, fields[index]),
            )
        })
    };
    let signed = |index: usize| -> Result<i64, MalformedInput> {
        fields[index].parse::<i64>().map_err(|_| {
            MalformedInput::new(
                PID_STAT,
                format!("field {} is not an integer: {:?}", index + 3, fields[index]),
            )
        })
    };
    Ok(ProcessStat {
        pid,
        comm,
        state: fields[0]
            .chars()
            .next()
            .ok_or_else(|| MalformedInput::new(PID_STAT, "empty state field"))?,
        parent_pid: unsigned(1)? as u32,
        user_ticks: unsigned(11)?,
        system_ticks: unsigned(12)?,
        priority: signed(15)?,
        nice: signed(16)?,
        threads: unsigned(17)?,
        start_ticks: unsigned(19)?,
        virtual_bytes: unsigned(20)?,
    })
}

/// The fields of `/proc/[pid]/status` Better Monitor uses.
///
/// Every one is optional: a kernel thread has no `VmRSS`, and a kernel without
/// swap has no `VmSwap`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessStatus {
    pub name: Option<String>,
    /// Real UID, the first of the four on the `Uid` line.
    pub real_uid: Option<u32>,
    pub resident_bytes: Option<u64>,
    pub swap_bytes: Option<u64>,
    pub threads: Option<u64>,
}

const PID_STATUS: &str = "/proc/[pid]/status";

pub fn parse_pid_status(input: &str) -> Result<ProcessStatus, MalformedInput> {
    let mut status = ProcessStatus::default();
    let mut saw_any = false;
    for line in input.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        saw_any = true;
        let rest = rest.trim();
        match key {
            "Name" => status.name = Some(rest.to_string()),
            // A key whose value was cut off by a truncated read leaves that
            // one field unknown; the rest of the file is still good. A value
            // that is present but not a number means the parser and the kernel
            // disagree, which is malformed.
            "Uid" => {
                if let Some(first) = rest.split_whitespace().next() {
                    status.real_uid = Some(first.parse::<u32>().map_err(|_| {
                        MalformedInput::new(PID_STATUS, format!("bad uid {first:?}"))
                    })?);
                }
            }
            "Threads" => {
                if !rest.is_empty() {
                    status.threads = Some(rest.parse::<u64>().map_err(|_| {
                        MalformedInput::new(PID_STATUS, format!("bad thread count {rest:?}"))
                    })?);
                }
            }
            "VmRSS" => status.resident_bytes = kibibytes(PID_STATUS, rest)?,
            "VmSwap" => status.swap_bytes = kibibytes(PID_STATUS, rest)?,
            _ => {}
        }
    }
    if !saw_any {
        return Err(MalformedInput::new(PID_STATUS, "no key: value lines"));
    }
    Ok(status)
}

fn kibibytes(context: &'static str, rest: &str) -> Result<Option<u64>, MalformedInput> {
    let Some(raw) = rest.split_whitespace().next() else {
        return Ok(None);
    };
    let value = raw
        .parse::<u64>()
        .map_err(|_| MalformedInput::new(context, format!("bad size {raw:?}")))?;
    Ok(Some(value.saturating_mul(1024)))
}

/// The cgroup a process belongs to.
///
/// cgroup v2 puts everything on the single `0::` line, which is what a modern
/// systemd desktop uses and what app grouping will read in ticket 23. A v1
/// hierarchy falls back to the first line's path so the value is never empty
/// when the kernel did say something.
pub fn parse_pid_cgroup(input: &str) -> Option<String> {
    let mut fallback = None;
    for line in input.lines() {
        let mut parts = line.splitn(3, ':');
        let hierarchy = parts.next()?;
        let _controllers = parts.next()?;
        let path = parts.next()?;
        if hierarchy == "0" {
            return Some(path.to_string());
        }
        fallback.get_or_insert_with(|| path.to_string());
    }
    fallback
}

/// `/proc/[pid]/cmdline` is NUL-separated, with a trailing NUL.
pub fn parse_cmdline(input: &str) -> Option<String> {
    let joined = input
        .split('\0')
        .filter(|argument| !argument.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        // A kernel thread has an empty command line. That is a fact about the
        // process, not a failed read.
        None
    } else {
        Some(joined)
    }
}

/// A minimal `/etc/passwd` reader: UID to login name.
pub fn parse_passwd(input: &str) -> HashMap<u32, String> {
    let mut users = HashMap::new();
    for line in input.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 3 {
            continue;
        }
        if let Ok(uid) = fields[2].parse::<u32>() {
            users.insert(uid, fields[0].to_string());
        }
    }
    users
}

/// The single-letter process states of `Documentation/filesystems/proc.rst`.
pub fn state_name(state: char) -> &'static str {
    match state {
        'R' => "running",
        'S' => "sleeping",
        'D' => "uninterruptible",
        'Z' => "zombie",
        'T' => "stopped",
        't' => "tracing-stop",
        'X' | 'x' => "dead",
        'I' => "idle",
        'W' => "paging",
        'K' => "wakekill",
        'P' => "parked",
        _ => "unknown",
    }
}

const PROCESS_COLLECTOR: &str = "linux.process";

#[derive(Clone, Copy)]
struct ProcessDelta {
    start_ticks: u64,
    cpu_ticks: u64,
}

struct ProcessSnapshot {
    at: Timestamp,
    processes: HashMap<u32, ProcessDelta>,
}

/// The process table.
pub struct ProcessCollector {
    roots: Roots,
    privacy: ProcessPrivacy,
    previous: Option<ProcessSnapshot>,
}

impl ProcessCollector {
    pub fn new(roots: Roots, privacy: ProcessPrivacy) -> Self {
        Self {
            roots,
            privacy,
            previous: None,
        }
    }

    pub fn privacy(&self) -> ProcessPrivacy {
        self.privacy
    }

    pub fn descriptors() -> Vec<MetricDescriptor> {
        vec![
            gauge(
                "process.count",
                Unit::Count,
                proc_source("[pid]"),
                "processes the collector could enumerate this round",
            ),
            gauge(
                "process.pid",
                Unit::Count,
                proc_source("[pid]/stat"),
                "process identifier",
            ),
            gauge(
                "process.parent.pid",
                Unit::Count,
                proc_source("[pid]/stat"),
                "parent process identifier",
            ),
            identity(
                "process.name",
                proc_source("[pid]/stat"),
                "executable name as the kernel records it",
            ),
            identity(
                "process.state",
                proc_source("[pid]/stat"),
                "scheduler state, expanded from the kernel's single letter",
            ),
            gauge(
                "process.uid",
                Unit::Count,
                proc_source("[pid]/status"),
                "real user identifier",
            ),
            identity(
                "process.user",
                derived_source("/proc/[pid]/status Uid resolved through /etc/passwd"),
                "login name of the owning user, unknown when the UID has no entry",
            ),
            gauge(
                "process.cpu.time.total",
                Unit::Seconds,
                derived_source("/proc/[pid]/stat utime + stime divided by USER_HZ"),
                "accumulated user and kernel CPU time",
            ),
            utilization(
                "process.cpu.utilization",
                derived_source("/proc/[pid]/stat CPU time delta over elapsed time"),
                "CPU time per second of wall time, as a fraction of one logical CPU",
            ),
            gauge(
                "process.memory.resident",
                Unit::Bytes,
                proc_source("[pid]/status"),
                "VmRSS, absent for a kernel thread",
            ),
            gauge(
                "process.memory.swap",
                Unit::Bytes,
                proc_source("[pid]/status"),
                "VmSwap, absent when the kernel has no swap accounting",
            ),
            gauge(
                "process.memory.virtual",
                Unit::Bytes,
                proc_source("[pid]/stat"),
                "virtual address space size",
            ),
            gauge(
                "process.threads",
                Unit::Count,
                proc_source("[pid]/status"),
                "threads in the thread group",
            ),
            gauge(
                "process.file_descriptors",
                Unit::Count,
                proc_source("[pid]/fd"),
                "open file descriptors, readable only for the caller's own processes",
            ),
            gauge(
                "process.start_time",
                Unit::Seconds,
                derived_source("/proc/stat btime plus /proc/[pid]/stat starttime"),
                "start time as a Unix timestamp",
            ),
            gauge(
                "process.runtime",
                Unit::Seconds,
                derived_source("/proc/uptime minus the process start time"),
                "how long the process has been running",
            ),
            gauge(
                "process.nice",
                Unit::Count,
                proc_source("[pid]/stat"),
                "nice value",
            ),
            gauge(
                "process.priority",
                Unit::Count,
                proc_source("[pid]/stat"),
                "scheduler priority",
            ),
            identity(
                "process.cgroup",
                proc_source("[pid]/cgroup"),
                "cgroup v2 path, or the first v1 hierarchy path",
            ),
            identity(
                "process.command_line",
                proc_source("[pid]/cmdline"),
                "full command line, withheld unless collection is configured to include it",
            ),
        ]
    }

    pub fn sample(&mut self, roots: &Roots, at: Timestamp) -> CollectorReport {
        let mut report = CollectorReport::new(collector_id(PROCESS_COLLECTOR), at);
        let entries = match list_dir(roots.proc_dir()) {
            Ok(entries) => entries,
            Err(error) => {
                report.health = match &error {
                    ReadError::Missing { path } => {
                        CollectorHealth::Unsupported(UnsupportedReason::InterfaceMissing {
                            path: path.display().to_string(),
                        })
                    }
                    other => CollectorHealth::Failed {
                        detail: other.path().display().to_string(),
                    },
                };
                return report;
            }
        };

        let boot_time = read_text(&roots.proc("stat"))
            .ok()
            .and_then(|raw| parse_proc_stat(&raw).ok())
            .and_then(|stat| stat.boot_time_unix_s);
        let uptime_seconds = read_text(&roots.proc("uptime"))
            .ok()
            .and_then(|raw| raw.split_whitespace().next()?.parse::<f64>().ok());
        let users = read_text(roots.passwd_path())
            .map(|raw| parse_passwd(&raw))
            .unwrap_or_default();

        let seconds = self
            .previous
            .as_ref()
            .and_then(|previous| Timestamp::interval_seconds(previous.at, at))
            .filter(|seconds| *seconds >= MINIMUM_DELTA_INTERVAL.as_secs_f64());

        let mut current = HashMap::new();
        let mut malformed = 0usize;
        for entry in entries {
            let Some(pid) = entry
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            match self.read_process(roots, pid, &users, boot_time, uptime_seconds, seconds) {
                Some((metrics, delta)) => {
                    current.insert(pid, delta);
                    report.entities.push(Entity::new(
                        EntityId::new(EntityKind::Process, pid.to_string()),
                        metrics,
                    ));
                }
                None => malformed += 1,
            }
        }

        report.metrics.insert(
            metric_id("process.count"),
            Observation::unsigned(report.entities.len() as u64),
        );
        if report.entities.is_empty() {
            report.health = CollectorHealth::Failed {
                detail: format!("no readable process under {}", roots.proc_dir().display()),
            };
        } else if malformed > 0 {
            report.health = CollectorHealth::Degraded {
                detail: format!("{malformed} processes could not be read"),
            };
        }

        self.previous = Some(ProcessSnapshot {
            at,
            processes: current,
        });
        report
    }

    #[allow(clippy::too_many_arguments)]
    fn read_process(
        &self,
        roots: &Roots,
        pid: u32,
        users: &HashMap<u32, String>,
        boot_time: Option<u64>,
        uptime_seconds: Option<f64>,
        seconds: Option<f64>,
    ) -> Option<(MetricSet, ProcessDelta)> {
        let stat_raw = read_text(&roots.proc(&format!("{pid}/stat"))).ok()?;
        let stat = parse_pid_stat(&stat_raw).ok()?;
        let mut metrics = MetricSet::new();

        metrics.insert(metric_id("process.pid"), Observation::unsigned(pid as u64));
        metrics.insert(
            metric_id("process.parent.pid"),
            Observation::unsigned(stat.parent_pid as u64),
        );
        metrics.insert(
            metric_id("process.state"),
            Observation::text(state_name(stat.state)),
        );
        metrics.insert(
            metric_id("process.nice"),
            Observation::Value(monitor_core::MetricScalar::Signed(stat.nice)),
        );
        metrics.insert(
            metric_id("process.priority"),
            Observation::Value(monitor_core::MetricScalar::Signed(stat.priority)),
        );
        metrics.insert(
            metric_id("process.memory.virtual"),
            Observation::unsigned(stat.virtual_bytes),
        );

        let cpu_ticks = stat.user_ticks.saturating_add(stat.system_ticks);
        metrics.insert(
            metric_id("process.cpu.time.total"),
            Observation::float(cpu_ticks as f64 / USER_HZ as f64),
        );

        // A reused PID has a different start time. Without this check the new
        // process would inherit the old one's counters.
        let utilization = match (self.previous.as_ref(), seconds) {
            (Some(previous), Some(seconds)) => match previous.processes.get(&pid) {
                Some(earlier) if earlier.start_ticks == stat.start_ticks => {
                    let ticks = cpu_ticks.saturating_sub(earlier.cpu_ticks);
                    Observation::float(ticks as f64 / USER_HZ as f64 / seconds)
                }
                _ => Observation::Unknown(UnknownReason::NotYetSampled),
            },
            (None, _) => Observation::Unknown(UnknownReason::NotYetSampled),
            (_, None) => Observation::Unknown(UnknownReason::IntervalTooShort),
        };
        metrics.insert(metric_id("process.cpu.utilization"), utilization);

        let start_seconds = stat.start_ticks as f64 / USER_HZ as f64;
        metrics.insert(
            metric_id("process.start_time"),
            match boot_time {
                Some(boot) => Observation::float(boot as f64 + start_seconds),
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "btime absent from /proc/stat".into(),
                }),
            },
        );
        metrics.insert(
            metric_id("process.runtime"),
            match uptime_seconds {
                Some(uptime) => Observation::float((uptime - start_seconds).max(0.0)),
                None => Observation::Unsupported(UnsupportedReason::NotReported {
                    detail: "/proc/uptime is not readable".into(),
                }),
            },
        );

        self.read_status(roots, pid, &stat, users, &mut metrics);
        self.read_descriptor_count(roots, pid, &mut metrics);
        self.read_cgroup(roots, pid, &mut metrics);
        self.read_command_line(roots, pid, &mut metrics);

        Some((
            metrics,
            ProcessDelta {
                start_ticks: stat.start_ticks,
                cpu_ticks,
            },
        ))
    }

    fn read_status(
        &self,
        roots: &Roots,
        pid: u32,
        stat: &ProcessStat,
        users: &HashMap<u32, String>,
        metrics: &mut MetricSet,
    ) {
        let status = match read_text(&roots.proc(&format!("{pid}/status"))) {
            Ok(raw) => parse_pid_status(&raw).ok(),
            Err(_) => None,
        };
        let name = status
            .as_ref()
            .and_then(|status| status.name.clone())
            .unwrap_or_else(|| stat.comm.clone());
        metrics.insert(metric_id("process.name"), Observation::text(name));
        metrics.insert(
            metric_id("process.threads"),
            Observation::unsigned(
                status
                    .as_ref()
                    .and_then(|status| status.threads)
                    .unwrap_or(stat.threads),
            ),
        );

        let uid = status.as_ref().and_then(|status| status.real_uid);
        metrics.insert(
            metric_id("process.uid"),
            match uid {
                Some(uid) => Observation::unsigned(uid as u64),
                None => Observation::Unknown(UnknownReason::EntityDisappeared),
            },
        );
        metrics.insert(
            metric_id("process.user"),
            match uid.and_then(|uid| users.get(&uid)) {
                Some(name) => Observation::text(name.clone()),
                None => Observation::Unknown(UnknownReason::ReadFailed {
                    detail: "no /etc/passwd entry for this uid".into(),
                }),
            },
        );

        for (id, value, absent) in [
            (
                "process.memory.resident",
                status.as_ref().and_then(|status| status.resident_bytes),
                "VmRSS absent; a kernel thread has no user address space",
            ),
            (
                "process.memory.swap",
                status.as_ref().and_then(|status| status.swap_bytes),
                "VmSwap absent from /proc/[pid]/status",
            ),
        ] {
            metrics.insert(
                metric_id(id),
                match value {
                    Some(bytes) => Observation::unsigned(bytes),
                    None => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: absent.to_string(),
                    }),
                },
            );
        }
    }

    fn read_descriptor_count(&self, roots: &Roots, pid: u32, metrics: &mut MetricSet) {
        let path = roots.proc(&format!("{pid}/fd"));
        metrics.insert(
            metric_id("process.file_descriptors"),
            match count_dir_entries(&path) {
                Ok(count) => Observation::unsigned(count),
                Err(error) => error.into_entity_observation(),
            },
        );
    }

    fn read_cgroup(&self, roots: &Roots, pid: u32, metrics: &mut MetricSet) {
        let path = roots.proc(&format!("{pid}/cgroup"));
        metrics.insert(
            metric_id("process.cgroup"),
            match read_text(&path) {
                Ok(raw) => match parse_pid_cgroup(&raw) {
                    Some(cgroup) => Observation::text(cgroup),
                    None => Observation::Unknown(UnknownReason::Malformed {
                        detail: "no hierarchy:controllers:path line".into(),
                    }),
                },
                Err(error) => error.into_entity_observation(),
            },
        );
    }

    fn read_command_line(&self, roots: &Roots, pid: u32, metrics: &mut MetricSet) {
        if !self.privacy.include_command_line {
            metrics.insert(
                metric_id("process.command_line"),
                Observation::Unsupported(UnsupportedReason::PolicyWithheld {
                    policy: "command lines are not collected unless explicitly enabled".into(),
                }),
            );
            return;
        }
        let path = roots.proc(&format!("{pid}/cmdline"));
        metrics.insert(
            metric_id("process.command_line"),
            match read_text(&path) {
                Ok(raw) => match parse_cmdline(&raw) {
                    Some(command) => Observation::text(command),
                    None => Observation::Unsupported(UnsupportedReason::NotReported {
                        detail: "empty command line; this is a kernel thread".into(),
                    }),
                },
                Err(error) => error.into_entity_observation(),
            },
        );
    }
}

impl Collector for ProcessCollector {
    fn id(&self) -> CollectorId {
        collector_id(PROCESS_COLLECTOR)
    }

    fn descriptors(&self) -> Vec<MetricDescriptor> {
        ProcessCollector::descriptors()
    }

    fn collect(&mut self, at: Timestamp) -> CollectorReport {
        let roots = self.roots.clone();
        self.sample(&roots, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempTree, at, fixture, running_as_root};
    use monitor_core::ObservationState;

    fn process<'a>(report: &'a CollectorReport, pid: &str) -> &'a Entity {
        report
            .entities
            .iter()
            .find(|entity| entity.id.key == pid)
            .unwrap_or_else(|| panic!("no process {pid} in the report"))
    }

    #[test]
    fn parses_a_captured_pid_stat() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/1/stat")).unwrap();
        let stat = parse_pid_stat(&raw).unwrap();
        assert_eq!(stat.pid, 1);
        assert_eq!(stat.comm, "systemd");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.parent_pid, 0);
        assert_eq!(stat.user_ticks, 59);
        assert_eq!(stat.system_ticks, 75);
        assert_eq!(stat.priority, 20);
        assert_eq!(stat.nice, 0);
        assert_eq!(stat.threads, 1);
        assert_eq!(stat.start_ticks, 9);
        assert_eq!(stat.virtual_bytes, 23_728_128);
    }

    #[test]
    fn an_executable_name_with_spaces_and_parentheses_does_not_shift_every_field() {
        let stat = parse_pid_stat(
            "7 (odd (name) here) S 1 7 7 0 -1 4194560 100 0 5 0 100 50 0 0 20 0 4 0 900 4096000 250\n",
        )
        .unwrap();
        assert_eq!(stat.comm, "odd (name) here");
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.parent_pid, 1);
        assert_eq!(stat.user_ticks, 100);
        assert_eq!(stat.system_ticks, 50);
        assert_eq!(stat.threads, 4);
        assert_eq!(stat.start_ticks, 900);
    }

    #[test]
    fn a_truncated_pid_stat_is_malformed() {
        let raw = std::fs::read_to_string(fixture("truncated").join("proc/9/stat")).unwrap();
        let error = parse_pid_stat(&raw).unwrap_err();
        assert!(error.detail.contains("need at least 22"));
    }

    #[test]
    fn a_pid_stat_with_a_non_numeric_pid_is_malformed() {
        let raw = std::fs::read_to_string(fixture("malformed").join("proc/9/stat")).unwrap();
        let error = parse_pid_stat(&raw).unwrap_err();
        assert!(error.detail.contains("bad pid"));
    }

    #[test]
    fn a_pid_stat_without_a_comm_field_is_malformed() {
        let error = parse_pid_stat("1 systemd S 0 1\n").unwrap_err();
        assert!(error.detail.contains("no comm field"));
    }

    #[test]
    fn parses_a_captured_pid_status() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("proc/1/status")).unwrap();
        let status = parse_pid_status(&raw).unwrap();
        assert_eq!(status.name.as_deref(), Some("systemd"));
        assert_eq!(status.real_uid, Some(0));
        assert_eq!(status.threads, Some(1));
        assert!(status.resident_bytes.is_some());
    }

    #[test]
    fn a_truncated_pid_status_still_yields_what_it_carried() {
        let raw = std::fs::read_to_string(fixture("truncated").join("proc/9/status")).unwrap();
        let status = parse_pid_status(&raw).unwrap();
        assert_eq!(status.name.as_deref(), Some("short"));
        assert_eq!(status.real_uid, None);
        assert_eq!(status.resident_bytes, None);
    }

    #[test]
    fn a_malformed_uid_line_is_rejected_rather_than_defaulted_to_root() {
        let raw = std::fs::read_to_string(fixture("malformed").join("proc/9/status")).unwrap();
        let error = parse_pid_status(&raw).unwrap_err();
        assert!(error.detail.contains("bad uid"));
    }

    #[test]
    fn a_status_file_with_no_key_value_lines_is_malformed() {
        let error = parse_pid_status("garbage\n").unwrap_err();
        assert!(error.detail.contains("no key: value"));
    }

    #[test]
    fn a_cgroup_v2_line_wins_over_any_v1_hierarchy() {
        assert_eq!(
            parse_pid_cgroup("5:cpu:/legacy\n0::/user.slice/app.scope\n").as_deref(),
            Some("/user.slice/app.scope")
        );
        assert_eq!(
            parse_pid_cgroup("5:cpu:/legacy\n3:memory:/other\n").as_deref(),
            Some("/legacy")
        );
        assert_eq!(parse_pid_cgroup("no-colons-here\n"), None);
    }

    #[test]
    fn a_nul_separated_command_line_becomes_one_readable_string() {
        assert_eq!(
            parse_cmdline("synthetic\0--flag\0").as_deref(),
            Some("synthetic --flag")
        );
        // A kernel thread has an empty command line.
        assert_eq!(parse_cmdline(""), None);
        assert_eq!(parse_cmdline("\0\0"), None);
    }

    #[test]
    fn passwd_maps_uids_and_skips_lines_it_cannot_parse() {
        let raw = std::fs::read_to_string(fixture("snapshot-a").join("etc/passwd")).unwrap();
        let users = parse_passwd(&raw);
        assert_eq!(users.get(&0).map(String::as_str), Some("root"));
        assert_eq!(users.get(&65534).map(String::as_str), Some("nobody"));

        let malformed = std::fs::read_to_string(fixture("malformed").join("etc/passwd")).unwrap();
        assert!(parse_passwd(&malformed).is_empty());
    }

    #[test]
    fn every_documented_state_letter_has_a_name() {
        assert_eq!(state_name('R'), "running");
        assert_eq!(state_name('D'), "uninterruptible");
        assert_eq!(state_name('Z'), "zombie");
        assert_eq!(state_name('?'), "unknown");
    }

    #[test]
    fn command_lines_are_withheld_unless_collection_is_configured_to_include_them() {
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        assert!(!collector.privacy().include_command_line);
        let report = collector.sample(&roots, at(0));
        let observation = process(&report, "7")
            .metrics
            .get(&metric_id("process.command_line"))
            .unwrap();
        assert_eq!(observation.state(), ObservationState::Unsupported);
        assert!(matches!(
            observation,
            Observation::Unsupported(UnsupportedReason::PolicyWithheld { .. })
        ));
    }

    #[test]
    fn an_enabled_command_line_is_collected() {
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = ProcessCollector::new(
            roots.clone(),
            ProcessPrivacy {
                include_command_line: true,
            },
        );
        let report = collector.sample(&roots, at(0));
        assert_eq!(
            process(&report, "7")
                .metrics
                .get(&metric_id("process.command_line"))
                .unwrap()
                .as_text(),
            Some("synthetic --flag")
        );
    }

    #[test]
    fn a_process_reports_its_identity_memory_and_cgroup() {
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        let report = collector.sample(&roots, at(0));
        let seven = process(&report, "7");
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.name"))
                .unwrap()
                .as_text(),
            Some("odd (name) here")
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.state"))
                .unwrap()
                .as_text(),
            Some("sleeping")
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.user"))
                .unwrap()
                .as_text(),
            Some("root")
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.memory.resident"))
                .unwrap()
                .as_f64(),
            Some(1024.0 * 1024.0)
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.memory.swap"))
                .unwrap()
                .as_f64(),
            Some(512.0 * 1024.0)
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.cgroup"))
                .unwrap()
                .as_text(),
            Some("/user.slice/synthetic.scope")
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.file_descriptors"))
                .unwrap()
                .as_f64(),
            Some(3.0)
        );
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.threads"))
                .unwrap()
                .as_f64(),
            Some(4.0)
        );
        assert_eq!(
            report
                .metrics
                .get(&metric_id("process.count"))
                .unwrap()
                .as_f64(),
            Some(1.0)
        );
    }

    #[test]
    fn cpu_time_accumulates_in_seconds_and_utilization_needs_two_rounds() {
        let a = Roots::at(fixture("synthetic-a"));
        let b = Roots::at(fixture("synthetic-b"));
        let mut collector = ProcessCollector::new(a.clone(), ProcessPrivacy::default());
        let first = collector.sample(&a, at(0));
        assert_eq!(
            process(&first, "7")
                .metrics
                .get(&metric_id("process.cpu.time.total"))
                .unwrap()
                .as_f64(),
            Some(1.5)
        );
        assert_eq!(
            process(&first, "7")
                .metrics
                .state_of(&metric_id("process.cpu.utilization")),
            ObservationState::Unknown
        );

        // 40 more user ticks and 10 more system ticks over one second is half
        // of one logical CPU.
        let second = collector.sample(&b, at(1_000));
        let utilization = process(&second, "7")
            .metrics
            .get(&metric_id("process.cpu.utilization"))
            .unwrap()
            .as_f64()
            .unwrap();
        assert!((utilization - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_reused_pid_does_not_inherit_the_previous_process_cpu_time() {
        let a = Roots::at(fixture("synthetic-a"));
        let mut collector = ProcessCollector::new(a.clone(), ProcessPrivacy::default());
        collector.sample(&a, at(0));

        // Same PID, a later start time, and a CPU time that went backwards.
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("7/stat"),
            "7 (reused) S 1 7 7 0 -1 4194560 100 0 5 0 5 5 0 0 20 0 1 0 999 4096000 250\n",
        )
        .unwrap();
        let report = collector.sample(&temporary.roots(), at(1_000));
        assert_eq!(
            process(&report, "7")
                .metrics
                .state_of(&metric_id("process.cpu.utilization")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn start_time_and_runtime_come_from_boot_time_and_uptime() {
        let roots = Roots::at(fixture("synthetic-a"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        let report = collector.sample(&roots, at(0));
        let seven = process(&report, "7");
        // btime 1700000000 plus 900 ticks at 100 Hz.
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.start_time"))
                .unwrap()
                .as_f64(),
            Some(1_700_000_009.0)
        );
        // uptime 1000 seconds minus 9.
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.runtime"))
                .unwrap()
                .as_f64(),
            Some(991.0)
        );
    }

    #[test]
    fn a_kernel_thread_without_vmrss_is_unsupported_rather_than_zero_memory() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("7/status"),
            "Name:\tkworker\nUid:\t0\t0\t0\t0\nThreads:\t1\n",
        )
        .unwrap();
        let mut collector = ProcessCollector::new(temporary.roots(), ProcessPrivacy::default());
        let report = collector.sample(&temporary.roots(), at(0));
        assert_eq!(
            process(&report, "7")
                .metrics
                .state_of(&metric_id("process.memory.resident")),
            ObservationState::Unsupported
        );
    }

    #[test]
    fn a_uid_with_no_passwd_entry_is_unknown_rather_than_blank() {
        let temporary = TempTree::copy_of("synthetic-a");
        std::fs::write(
            temporary.roots().proc("7/status"),
            "Name:\todd\nUid:\t4242\t4242\t4242\t4242\nThreads:\t1\nVmRSS:\t 1024 kB\n",
        )
        .unwrap();
        let mut collector = ProcessCollector::new(temporary.roots(), ProcessPrivacy::default());
        let report = collector.sample(&temporary.roots(), at(0));
        let seven = process(&report, "7");
        assert_eq!(
            seven
                .metrics
                .get(&metric_id("process.uid"))
                .unwrap()
                .as_f64(),
            Some(4242.0)
        );
        assert_eq!(
            seven.metrics.state_of(&metric_id("process.user")),
            ObservationState::Unknown
        );
    }

    #[test]
    fn an_unreadable_fd_directory_is_permission_denied_not_a_count_of_zero() {
        if running_as_root() {
            // Root can read any descriptor directory, so the distinction this
            // test exists for cannot be produced here.
            return;
        }
        let temporary = TempTree::copy_of("synthetic-a");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            temporary.roots().proc("7/fd"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let mut collector = ProcessCollector::new(temporary.roots(), ProcessPrivacy::default());
        let report = collector.sample(&temporary.roots(), at(0));
        let observation = process(&report, "7")
            .metrics
            .get(&metric_id("process.file_descriptors"))
            .unwrap();
        assert_eq!(observation.state(), ObservationState::PermissionDenied);
        assert_eq!(observation.as_f64(), None);
    }

    #[test]
    fn the_captured_host_snapshot_yields_a_real_process_table() {
        let roots = Roots::at(fixture("snapshot-a"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        let report = collector.sample(&roots, at(0));
        assert!(report.entities.len() >= 2);
        assert_eq!(
            process(&report, "1")
                .metrics
                .get(&metric_id("process.name"))
                .unwrap()
                .as_text(),
            Some("systemd")
        );
        assert_eq!(
            process(&report, "1")
                .metrics
                .get(&metric_id("process.cgroup"))
                .unwrap()
                .as_text(),
            Some("/init.scope")
        );
    }

    #[test]
    fn a_process_whose_cpu_time_did_not_move_reports_a_real_zero() {
        // pid 1 is identical in both captured snapshots.
        let a = Roots::at(fixture("snapshot-a"));
        let b = Roots::at(fixture("snapshot-b"));
        let mut collector = ProcessCollector::new(a.clone(), ProcessPrivacy::default());
        collector.sample(&a, at(0));
        let report = collector.sample(&b, at(1_000));
        let observation = process(&report, "1")
            .metrics
            .get(&metric_id("process.cpu.utilization"))
            .unwrap();
        assert_eq!(observation.state(), ObservationState::Value);
        assert_eq!(observation.as_f64(), Some(0.0));
    }

    #[test]
    fn a_malformed_process_is_skipped_and_the_collector_says_so() {
        let roots = Roots::at(fixture("malformed"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        let report = collector.sample(&roots, at(0));
        assert!(report.entities.is_empty());
        assert!(matches!(report.health, CollectorHealth::Failed { .. }));
    }

    #[test]
    fn a_host_without_proc_reports_the_subsystem_unsupported() {
        let roots = Roots::at(fixture("does-not-exist"));
        let mut collector = ProcessCollector::new(roots.clone(), ProcessPrivacy::default());
        let report = collector.sample(&roots, at(0));
        assert!(matches!(report.health, CollectorHealth::Unsupported(_)));
    }

    #[test]
    fn the_catalog_is_well_formed_and_free_of_duplicates() {
        let descriptors = ProcessCollector::descriptors();
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

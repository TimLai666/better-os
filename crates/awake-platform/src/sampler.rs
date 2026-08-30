//! One object that owns every provider and produces one [`Observations`].
//!
//! The sampler is where "bounded polling, no busy loops" stops being a promise
//! and becomes arithmetic. Each provider names its cadence; the sampler
//! remembers when it last read each one and re-reads only the ones whose
//! interval has elapsed. A provider no enabled rule needs is not sampled at all,
//! so a machine with no rules does no filesystem I/O for triggers whatsoever.
//!
//! The previous sample is kept and carried forward, so a provider that was not
//! due this tick still contributes the answer it gave last time rather than
//! going briefly unknown between polls.

use std::collections::BTreeMap;

use awake_core::{Observations, ProviderKind, RuleSet, WatchedPath};

use crate::audio::AudioProvider;
use crate::cpu::CpuProvider;
use crate::display::DisplayProvider;
use crate::fullscreen::FullscreenProvider;
use crate::network::NetworkProvider;
use crate::power::PowerProvider;
use crate::process::ProcessProvider;
use crate::provider::{Cadence, ProviderReport, TriggerProvider};
use crate::roots::Roots;
use crate::schedule::ScheduleProvider;
use crate::watch::WatchProvider;

/// Every provider, and when each was last read.
pub struct ProviderSet {
    process: ProcessProvider,
    ac: PowerProvider,
    battery: PowerProvider,
    display: DisplayProvider,
    audio: AudioProvider,
    cpu: CpuProvider,
    throughput: NetworkProvider,
    interfaces: NetworkProvider,
    schedule: ScheduleProvider,
    watch: WatchProvider,
    fullscreen: FullscreenProvider,
    /// When each provider was last sampled, so a cadence can be honoured.
    last_sampled: BTreeMap<ProviderKind, u64>,
    /// The last complete sample, carried forward between polls.
    previous: Observations,
    roots: Roots,
}

impl ProviderSet {
    /// The production set, reading the real machine.
    pub fn system() -> Self {
        Self::new(Roots::system(), || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default()
        })
    }

    /// A set reading a captured tree, which is how every provider is tested
    /// through the same code path production uses.
    pub fn new(roots: Roots, clock: impl Fn() -> u64 + Send + 'static) -> Self {
        Self {
            process: ProcessProvider::new(roots.clone()),
            ac: PowerProvider::ac(roots.clone()),
            battery: PowerProvider::battery(roots.clone()),
            display: DisplayProvider::new(roots.clone()),
            audio: AudioProvider::new(roots.clone()),
            cpu: CpuProvider::new(roots.clone()),
            throughput: NetworkProvider::throughput(roots.clone()),
            interfaces: NetworkProvider::interfaces(roots.clone()),
            schedule: ScheduleProvider,
            watch: WatchProvider::new(clock),
            fullscreen: FullscreenProvider,
            last_sampled: BTreeMap::new(),
            previous: Observations::default(),
            roots,
        }
    }

    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Whether this machine has a battery, which decides whether the battery
    /// stop threshold is enabled by default.
    pub fn has_battery(&self) -> bool {
        self.ac.has_battery()
    }

    /// The cadence of one provider kind, so Diagnostics can show the table
    /// rather than repeat it in prose.
    pub fn cadence(&self, kind: ProviderKind) -> Cadence {
        match kind {
            ProviderKind::ProcessRunning => self.process.cadence(),
            ProviderKind::AcPower => self.ac.cadence(),
            ProviderKind::BatteryPercent => self.battery.cadence(),
            ProviderKind::ExternalDisplay => self.display.cadence(),
            ProviderKind::AudioPlayback => self.audio.cadence(),
            ProviderKind::CpuUtilization => self.cpu.cadence(),
            ProviderKind::NetworkThroughput => self.throughput.cadence(),
            ProviderKind::NetworkInterface => self.interfaces.cadence(),
            ProviderKind::TimeSchedule => self.schedule.cadence(),
            ProviderKind::WatchedPath => self.watch.cadence(),
            ProviderKind::Fullscreen => self.fullscreen.cadence(),
        }
    }

    /// Registers the paths the rules watch. Called whenever the rules change.
    pub fn watch_paths(&mut self, rules: &RuleSet) {
        let mut wanted: Vec<WatchedPath> = Vec::new();
        for rule in rules.enabled_rules() {
            for group in &rule.groups {
                for condition in &group.conditions {
                    if let awake_core::Condition::WatchedPathActive { path, .. } = condition
                        && !wanted.contains(path)
                    {
                        wanted.push(path.clone());
                    }
                }
            }
        }
        self.watch.watch_only(&wanted);
    }

    /// Samples every provider whose cadence has elapsed and that some enabled
    /// rule needs, and returns the complete picture.
    pub fn sample(&mut self, rules: &RuleSet, now_unix_seconds: u64) -> Observations {
        self.sample_with(rules, &[], now_unix_seconds)
    }

    /// Samples what the rules need, plus a set the caller always wants.
    ///
    /// The `always` list exists for battery protection. That is a safety
    /// guarantee the user gets whether or not any rule mentions the battery, so
    /// deciding to read the battery from "does a rule ask for it" would make the
    /// protection depend on the rules a user happens to have written. Cadence is
    /// still honoured: always-sampled does not mean sampled every tick.
    pub fn sample_with(
        &mut self,
        rules: &RuleSet,
        always: &[ProviderKind],
        now_unix_seconds: u64,
    ) -> Observations {
        let mut required = rules.required_providers();
        for kind in always {
            if !required.contains(kind) {
                required.push(*kind);
            }
        }
        let mut observations = self.previous.clone();
        observations.sampled_at_unix_seconds = now_unix_seconds;

        for kind in ProviderKind::ALL {
            if !required.contains(&kind) {
                continue;
            }
            if !self.is_due(kind, now_unix_seconds) {
                continue;
            }
            self.sample_one(kind, now_unix_seconds, &mut observations);
            self.last_sampled.insert(kind, now_unix_seconds);
        }

        self.previous = observations.clone();
        observations
    }

    /// Samples one provider whatever its cadence says, used when a rule was just
    /// edited and its answer is wanted now rather than at the next interval.
    pub fn sample_now(&mut self, kind: ProviderKind, now_unix_seconds: u64) -> Observations {
        let mut observations = self.previous.clone();
        observations.sampled_at_unix_seconds = now_unix_seconds;
        self.sample_one(kind, now_unix_seconds, &mut observations);
        self.last_sampled.insert(kind, now_unix_seconds);
        self.previous = observations.clone();
        observations
    }

    /// Samples every provider regardless of cadence. Used by the rule editor's
    /// test mode, which must give an answer now, and by Diagnostics.
    pub fn sample_all(&mut self, now_unix_seconds: u64) -> Observations {
        let mut observations = self.previous.clone();
        observations.sampled_at_unix_seconds = now_unix_seconds;
        for kind in ProviderKind::ALL {
            self.sample_one(kind, now_unix_seconds, &mut observations);
            self.last_sampled.insert(kind, now_unix_seconds);
        }
        self.previous = observations.clone();
        observations
    }

    /// What every provider says about itself, for Diagnostics and for the rule
    /// editor's unavailable-control explanations.
    pub fn reports(&mut self, now_unix_seconds: u64) -> Vec<ProviderReport> {
        let observations = self.sample_all(now_unix_seconds);
        ProviderKind::ALL
            .into_iter()
            .map(|kind| {
                let availability = observations.availability_of(kind);
                ProviderReport {
                    kind,
                    cadence: self.cadence(kind),
                    available: availability.is_available(),
                    explanation: availability.explanation().map(str::to_string),
                }
            })
            .collect()
    }

    fn is_due(&self, kind: ProviderKind, now_unix_seconds: u64) -> bool {
        let Some(interval) = self.cadence(kind).poll_seconds() else {
            // Free and event-driven providers are read every tick because
            // reading them costs nothing; the cost the cadence bounds is I/O.
            return true;
        };
        match self.last_sampled.get(&kind) {
            None => true,
            Some(last) => now_unix_seconds.saturating_sub(*last) >= interval,
        }
    }

    fn sample_one(&mut self, kind: ProviderKind, now: u64, into: &mut Observations) {
        match kind {
            ProviderKind::ProcessRunning => self.process.sample(now, into),
            ProviderKind::AcPower => self.ac.sample(now, into),
            ProviderKind::BatteryPercent => self.battery.sample(now, into),
            ProviderKind::ExternalDisplay => self.display.sample(now, into),
            ProviderKind::AudioPlayback => self.audio.sample(now, into),
            ProviderKind::CpuUtilization => self.cpu.sample(now, into),
            ProviderKind::NetworkThroughput => self.throughput.sample(now, into),
            ProviderKind::NetworkInterface => self.interfaces.sample(now, into),
            ProviderKind::TimeSchedule => self.schedule.sample(now, into),
            ProviderKind::WatchedPath => self.watch.sample(now, into),
            ProviderKind::Fullscreen => self.fullscreen.sample(now, into),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use awake_core::{Combine, Condition, ConditionGroup, Reason, Rule, RuleId, RuleSet};

    fn fixture() -> (tempfile::TempDir, Roots) {
        let directory = tempfile::tempdir().unwrap();
        let proc = directory.path().join("proc");
        let sys = directory.path().join("sys");
        std::fs::create_dir_all(&proc).unwrap();
        crate::power::write_supply(&sys, "ACAD", &[("type", "Mains"), ("online", "1")]);
        crate::power::write_supply(&sys, "BAT1", &[("type", "Battery"), ("capacity", "65")]);
        crate::display::write_connector(&sys, "card1-eDP-1", "connected");
        crate::process::write_process(&proc, 1, "systemd", None);
        std::fs::write(proc.join("stat"), "cpu  100 0 100 800 0 0 0 0\n").unwrap();
        let roots = Roots::at(directory.path());
        (directory, roots)
    }

    fn set() -> ProviderSet {
        let (directory, roots) = fixture();
        // The fixture directory must outlive the set, so it is leaked here
        // rather than dropped at the end of this function.
        std::mem::forget(directory);
        ProviderSet::new(roots, || 1_000)
    }

    fn rules_needing(condition: Condition) -> RuleSet {
        let mut rules = RuleSet::new();
        rules
            .add(
                Rule::new(
                    RuleId(0),
                    Reason::new("Test rule").unwrap(),
                    Combine::All,
                    [ConditionGroup::one(condition).unwrap()],
                )
                .unwrap(),
            )
            .unwrap();
        rules
    }

    #[test]
    fn a_machine_with_no_rules_samples_nothing_at_all() {
        let mut set = set();
        let observations = set.sample(&RuleSet::new(), 1_000);
        assert!(
            observations.availability.is_empty(),
            "no rule means no reason to read a single file"
        );
    }

    #[test]
    fn only_the_providers_the_rules_need_are_sampled() {
        let mut set = set();
        let rules = rules_needing(Condition::AcPower { connected: true });
        let observations = set.sample(&rules, 1_000);

        assert_eq!(observations.ac_power_connected, Some(true));
        assert_eq!(
            observations.running_processes, None,
            "no rule mentions a process, so `/proc` is never walked"
        );
        assert_eq!(observations.external_display_connected, None);
    }

    #[test]
    fn a_provider_is_not_re_read_before_its_cadence_has_elapsed() {
        let mut set = set();
        let rules = rules_needing(Condition::AcPower { connected: true });

        set.sample(&rules, 1_000);
        // Unplug the charger behind the sampler's back.
        std::fs::write(
            set.roots().sys_path("class/power_supply/ACAD/online"),
            b"0\n",
        )
        .unwrap();

        let too_soon = set.sample(&rules, 1_005);
        assert_eq!(
            too_soon.ac_power_connected,
            Some(true),
            "the previous answer is carried forward rather than going briefly unknown"
        );

        let due = set.sample(&rules, 1_010);
        assert_eq!(due.ac_power_connected, Some(false));
    }

    #[test]
    fn sampling_everything_ignores_the_cadence_because_test_mode_needs_an_answer_now() {
        let mut set = set();
        let rules = rules_needing(Condition::AcPower { connected: true });
        set.sample(&rules, 1_000);
        std::fs::write(
            set.roots().sys_path("class/power_supply/ACAD/online"),
            b"0\n",
        )
        .unwrap();

        let all = set.sample_all(1_001);
        assert_eq!(all.ac_power_connected, Some(false));
    }

    #[test]
    fn every_provider_reports_itself_and_the_unavailable_ones_say_why() {
        let mut set = set();
        let reports = set.reports(1_000);
        assert_eq!(reports.len(), ProviderKind::ALL.len());

        for report in &reports {
            assert_eq!(
                report.available,
                report.explanation.is_none(),
                "{:?} must either work or explain itself, never both and never neither",
                report.kind
            );
        }

        let fullscreen = reports
            .iter()
            .find(|report| report.kind == ProviderKind::Fullscreen)
            .unwrap();
        assert!(!fullscreen.available);
        assert_eq!(
            fullscreen.explanation.as_deref(),
            Some(crate::fullscreen::FULLSCREEN_UNAVAILABLE)
        );
    }

    #[test]
    fn no_provider_that_reads_a_file_is_read_more_than_once_every_five_seconds() {
        let set = set();
        for kind in ProviderKind::ALL {
            if let Some(seconds) = set.cadence(kind).poll_seconds() {
                assert!(
                    seconds >= 5,
                    "{kind:?} polls every {seconds}s, which is more I/O than its answer is worth"
                );
            }
        }
    }

    #[test]
    fn the_watched_paths_follow_the_rules_that_name_them() {
        let directory = tempfile::tempdir().unwrap();
        let mut set = set();
        let rules = rules_needing(Condition::WatchedPathActive {
            path: WatchedPath::new(directory.path()).unwrap(),
            within_seconds: 60,
        });

        set.watch_paths(&rules);
        assert_eq!(
            set.watch.log().tracked(),
            vec![directory.path().to_path_buf()]
        );

        set.watch_paths(&RuleSet::new());
        assert!(
            set.watch.log().tracked().is_empty(),
            "a path no rule names any more must stop costing a watch descriptor"
        );
    }

    #[test]
    fn a_battery_powered_machine_is_recognized_from_its_hardware() {
        assert!(set().has_battery());

        let directory = tempfile::tempdir().unwrap();
        crate::power::write_supply(
            &directory.path().join("sys"),
            "ACAD",
            &[("type", "Mains"), ("online", "1")],
        );
        let desktop = ProviderSet::new(Roots::at(directory.path()), || 1_000);
        assert!(!desktop.has_battery());
    }
}

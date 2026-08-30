//! Deciding which rules are active, and explaining the answer.
//!
//! Evaluation is three-valued. A condition is true, false, or unknown, and
//! unknown is what a provider that cannot read its source produces. Unknown
//! never becomes true, so a rule whose provider is unavailable does not keep the
//! machine awake — and it never becomes a silent false either, so the UI can say
//! *why* the rule did not fire instead of showing it as simply not matching.
//!
//! Nothing here acquires anything. [`RuleSet::evaluate`] and
//! [`RuleSet::test_rule`] both take `&self` and return values, which is what
//! makes the ticket's "test a rule without acquiring an inhibitor" a property of
//! the type rather than a promise about the call site.

use std::path::Path;

use crate::policy::{SessionPolicy, merge_battery_threshold};
use crate::rules::{
    Combine, Condition, ConditionGroup, LocalTime, ProviderKind, Rule, RuleError, RuleId, RuleSet,
    Suppression,
};

/// A condition's answer, including "the provider could not tell us".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Truth {
    False,
    /// The provider that would answer this is unavailable or has not reported.
    Unknown,
    True,
}

impl Truth {
    pub fn as_key(self) -> &'static str {
        match self {
            Truth::False => "false",
            Truth::Unknown => "unknown",
            Truth::True => "true",
        }
    }

    pub fn is_true(self) -> bool {
        self == Truth::True
    }

    fn from_bool(value: bool) -> Self {
        if value { Truth::True } else { Truth::False }
    }
}

/// Whether a provider could be read, and why not when it could not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderAvailability {
    Available,
    /// A stable key naming what is missing. Never a localized sentence.
    Unavailable {
        explanation_key: String,
    },
}

impl ProviderAvailability {
    pub fn unavailable(explanation_key: impl Into<String>) -> Self {
        ProviderAvailability::Unavailable {
            explanation_key: explanation_key.into(),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, ProviderAvailability::Available)
    }

    pub fn explanation(&self) -> Option<&str> {
        match self {
            ProviderAvailability::Available => None,
            ProviderAvailability::Unavailable { explanation_key } => Some(explanation_key),
        }
    }
}

/// One watched path and when it last changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchActivity {
    pub path: std::path::PathBuf,
    pub last_change_unix_seconds: u64,
}

/// Everything the providers reported on one sample.
///
/// A `None` field means that provider could not be read on this sample; the
/// reason is in [`Observations::availability`]. The two are kept together so a
/// missing reading can never be presented without its explanation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Observations {
    /// Executable names from `/proc/<pid>/comm`, plus desktop identifiers where
    /// the session knows them. Never command lines.
    pub running_processes: Option<Vec<String>>,
    pub running_desktop_ids: Option<Vec<String>>,
    pub ac_power_connected: Option<bool>,
    pub battery_percent: Option<u8>,
    pub external_display_connected: Option<bool>,
    pub audio_playing: Option<bool>,
    pub cpu_utilization_percent: Option<u8>,
    pub network_kibibytes_per_second: Option<u64>,
    pub interfaces_up: Option<Vec<String>>,
    pub local_time: Option<LocalTime>,
    pub watch_activity: Option<Vec<WatchActivity>>,
    pub fullscreen_active: Option<bool>,
    /// One entry per provider that could not be read, keyed by kind.
    pub availability: Vec<(ProviderKind, ProviderAvailability)>,
    /// The moment this sample was taken, used by the watched-path window.
    pub sampled_at_unix_seconds: u64,
}

impl Observations {
    pub fn at(sampled_at_unix_seconds: u64) -> Self {
        Self {
            sampled_at_unix_seconds,
            ..Self::default()
        }
    }

    /// Records that a provider could not be read.
    pub fn mark_unavailable(&mut self, kind: ProviderKind, explanation_key: impl Into<String>) {
        self.availability.retain(|(existing, _)| *existing != kind);
        self.availability
            .push((kind, ProviderAvailability::unavailable(explanation_key)));
    }

    pub fn mark_available(&mut self, kind: ProviderKind) {
        self.availability.retain(|(existing, _)| *existing != kind);
        self.availability
            .push((kind, ProviderAvailability::Available));
    }

    /// What is known about one provider. A kind nobody reported on is treated as
    /// unavailable with a key saying exactly that, rather than as available with
    /// no data.
    pub fn availability_of(&self, kind: ProviderKind) -> ProviderAvailability {
        self.availability
            .iter()
            .find(|(existing, _)| *existing == kind)
            .map(|(_, availability)| availability.clone())
            .unwrap_or_else(|| ProviderAvailability::unavailable("awake.provider.not_sampled"))
    }

    fn watch_within(&self, path: &Path, within_seconds: u64) -> Option<bool> {
        let activity = self.watch_activity.as_ref()?;
        let Some(entry) = activity.iter().find(|entry| entry.path == path) else {
            // The provider is running but is not watching this path, which is
            // not the same as "no activity": it cannot answer this condition.
            return None;
        };
        Some(
            self.sampled_at_unix_seconds
                .saturating_sub(entry.last_change_unix_seconds)
                <= within_seconds,
        )
    }
}

/// Evaluates one condition against a sample.
pub fn evaluate_condition(condition: &Condition, observations: &Observations) -> Truth {
    match condition {
        Condition::ProcessRunning { matcher } => {
            use crate::rules::ProcessMatchKind;
            let names = match matcher.kind {
                ProcessMatchKind::ExecutableName => observations.running_processes.as_ref(),
                ProcessMatchKind::DesktopId => observations.running_desktop_ids.as_ref(),
            };
            match names {
                None => Truth::Unknown,
                Some(names) => Truth::from_bool(names.iter().any(|name| matcher.matches(name))),
            }
        }
        Condition::AcPower { connected } => match observations.ac_power_connected {
            None => Truth::Unknown,
            Some(actual) => Truth::from_bool(actual == *connected),
        },
        Condition::BatteryPercent { at_least, at_most } => match observations.battery_percent {
            None => Truth::Unknown,
            Some(percent) => Truth::from_bool((*at_least..=*at_most).contains(&percent)),
        },
        Condition::ExternalDisplay { connected } => match observations.external_display_connected {
            None => Truth::Unknown,
            Some(actual) => Truth::from_bool(actual == *connected),
        },
        Condition::AudioPlayback { playing } => match observations.audio_playing {
            None => Truth::Unknown,
            Some(actual) => Truth::from_bool(actual == *playing),
        },
        Condition::CpuUtilizationAtLeast { percent } => {
            match observations.cpu_utilization_percent {
                None => Truth::Unknown,
                Some(actual) => Truth::from_bool(actual >= *percent),
            }
        }
        Condition::NetworkThroughputAtLeast {
            kibibytes_per_second,
        } => match observations.network_kibibytes_per_second {
            None => Truth::Unknown,
            Some(actual) => Truth::from_bool(actual >= *kibibytes_per_second),
        },
        Condition::NetworkInterfaceUp { interface } => match &observations.interfaces_up {
            None => Truth::Unknown,
            Some(up) => Truth::from_bool(up.iter().any(|name| name == interface.as_str())),
        },
        Condition::TimeSchedule { schedule } => match observations.local_time {
            None => Truth::Unknown,
            Some(now) => Truth::from_bool(schedule.contains(now)),
        },
        Condition::WatchedPathActive {
            path,
            within_seconds,
        } => match observations.watch_within(path.as_path(), *within_seconds) {
            None => Truth::Unknown,
            Some(active) => Truth::from_bool(active),
        },
        Condition::Fullscreen { active } => match observations.fullscreen_active {
            None => Truth::Unknown,
            Some(actual) => Truth::from_bool(actual == *active),
        },
    }
}

/// Evaluates one AND/OR group.
///
/// The short-circuit that matters is the decisive one: an AND with a false
/// member is false even if another member is unknown, and an OR with a true
/// member is true even if another member is unknown. Only when the unknown could
/// still change the answer does the group report unknown.
pub fn evaluate_group(group: &ConditionGroup, observations: &Observations) -> Truth {
    let truths: Vec<Truth> = group
        .conditions
        .iter()
        .map(|condition| evaluate_condition(condition, observations))
        .collect();
    if truths.is_empty() {
        // Validation refuses this shape; if one reaches here from a file written
        // by hand, it must not be vacuously true.
        return Truth::False;
    }
    match group.combine {
        Combine::All => {
            if truths.contains(&Truth::False) {
                Truth::False
            } else if truths.contains(&Truth::Unknown) {
                Truth::Unknown
            } else {
                Truth::True
            }
        }
        Combine::Any => {
            if truths.contains(&Truth::True) {
                Truth::True
            } else if truths.contains(&Truth::Unknown) {
                Truth::Unknown
            } else {
                Truth::False
            }
        }
    }
}

/// The result of evaluating one rule, with enough detail to explain it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleOutcome {
    pub rule: RuleId,
    pub truth: Truth,
    /// One entry per group, in the rule's own order.
    pub group_truths: Vec<Truth>,
    /// Providers this rule needs that could not be read, with their reasons.
    pub unavailable_providers: Vec<(ProviderKind, String)>,
}

impl RuleOutcome {
    /// Whether the only thing between this rule and an answer is a provider that
    /// could not be read.
    pub fn blocked_by_provider(&self) -> bool {
        self.truth == Truth::Unknown && !self.unavailable_providers.is_empty()
    }
}

/// One policy field two active rules disagreed about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyField {
    SystemSuspend,
    Idle,
    DisplaySleep,
    AutomaticLock,
    BatteryThreshold,
}

impl PolicyField {
    pub fn as_key(self) -> &'static str {
        match self {
            PolicyField::SystemSuspend => "prevent_system_suspend",
            PolicyField::Idle => "prevent_idle",
            PolicyField::DisplaySleep => "prevent_display_sleep",
            PolicyField::AutomaticLock => "prevent_automatic_lock",
            PolicyField::BatteryThreshold => "battery_stop_percent",
        }
    }
}

/// How a disagreement between active rules was settled.
///
/// Priority decides which rule is *named* as the source of the effective answer.
/// It does not decide the answer itself: a policy field is the union of what
/// every active rule asked for, and the battery threshold is the one that stops
/// first. A high-priority rule therefore cannot switch protection off that a
/// low-priority rule asked for, which is the whole point of stating this rather
/// than leaving it implied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Conflict {
    pub field: PolicyField,
    /// The highest-priority active rule that asked for the answer that won.
    pub winner: RuleId,
    /// Active rules that asked for something weaker in this field.
    pub overridden: Vec<RuleId>,
    /// A stable key naming the rule that settled it.
    pub resolution_key: &'static str,
}

/// Resolution keys, so a presentation layer never has to parse a sentence.
pub const RESOLUTION_STRONGEST_WINS: &str = "awake.conflict.strongest_protection_wins";
pub const RESOLUTION_EARLIEST_BATTERY_STOP: &str = "awake.conflict.earliest_battery_stop_wins";

/// A rule that is currently satisfied and should be holding a session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveRule {
    pub rule: RuleId,
    pub priority: u8,
    pub policy: SessionPolicy,
    pub battery_stop_percent: Option<u8>,
}

/// Everything one evaluation decided, and why.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct Evaluation {
    /// Every enabled rule's outcome, ordered by priority descending and then by
    /// the user's own ordering.
    pub outcomes: Vec<RuleOutcome>,
    /// The rules that should be holding a session, in the same order.
    pub active: Vec<ActiveRule>,
    /// Why nothing is active, when rules are suspended.
    pub suppression: Option<Suppression>,
    pub conflicts: Vec<Conflict>,
}

impl Evaluation {
    /// The merged policy the active rules together ask for.
    pub fn merged_policy(&self) -> SessionPolicy {
        self.active
            .iter()
            .fold(SessionPolicy::default(), |merged, rule| {
                merged.union(rule.policy)
            })
    }

    /// The battery threshold the active rules together ask for, which is the one
    /// that stops first.
    pub fn merged_battery_stop_percent(&self) -> Option<u8> {
        self.active.iter().fold(None, |merged, rule| {
            merge_battery_threshold(merged, rule.battery_stop_percent)
        })
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppression.is_some()
    }

    pub fn outcome(&self, rule: RuleId) -> Option<&RuleOutcome> {
        self.outcomes.iter().find(|outcome| outcome.rule == rule)
    }
}

/// What testing one rule reported, without anything having been started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleTest {
    pub outcome: RuleOutcome,
    /// Whether this rule would hold a session right now if it were enabled and
    /// rules were not suspended.
    pub would_be_active: bool,
    /// Present when the rule is satisfied but something is stopping every rule.
    pub suppression: Option<Suppression>,
    /// Present when the rule itself is switched off. Testing a disabled rule is
    /// allowed, because that is when a person most wants to know whether it
    /// works.
    pub rule_disabled: bool,
}

fn outcome_for(rule: &Rule, observations: &Observations) -> RuleOutcome {
    let group_truths: Vec<Truth> = rule
        .groups
        .iter()
        .map(|group| evaluate_group(group, observations))
        .collect();

    let truth = if group_truths.is_empty() {
        Truth::False
    } else {
        match rule.combine {
            Combine::All => {
                if group_truths.contains(&Truth::False) {
                    Truth::False
                } else if group_truths.contains(&Truth::Unknown) {
                    Truth::Unknown
                } else {
                    Truth::True
                }
            }
            Combine::Any => {
                if group_truths.contains(&Truth::True) {
                    Truth::True
                } else if group_truths.contains(&Truth::Unknown) {
                    Truth::Unknown
                } else {
                    Truth::False
                }
            }
        }
    };

    let unavailable_providers = rule
        .providers()
        .into_iter()
        .filter_map(|kind| {
            observations
                .availability_of(kind)
                .explanation()
                .map(|explanation| (kind, explanation.to_string()))
        })
        .collect();

    RuleOutcome {
        rule: rule.id,
        truth,
        group_truths,
        unavailable_providers,
    }
}

/// Explains every field the active rules disagreed about.
fn conflicts_for(active: &[ActiveRule]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    if active.len() < 2 {
        return conflicts;
    }

    /// One policy flag paired with the way to read it, so the four boolean
    /// fields are compared by one piece of code rather than four copies of it.
    type FieldReader = (PolicyField, fn(&SessionPolicy) -> bool);

    let fields: [FieldReader; 4] = [
        (PolicyField::SystemSuspend, |policy| {
            policy.prevent_system_suspend
        }),
        (PolicyField::Idle, |policy| policy.prevent_idle),
        (PolicyField::DisplaySleep, |policy| {
            policy.prevent_display_sleep
        }),
        (PolicyField::AutomaticLock, |policy| {
            policy.prevent_automatic_lock
        }),
    ];

    for (field, read) in fields {
        let asking: Vec<&ActiveRule> = active.iter().filter(|rule| read(&rule.policy)).collect();
        let not_asking: Vec<RuleId> = active
            .iter()
            .filter(|rule| !read(&rule.policy))
            .map(|rule| rule.rule)
            .collect();
        // A disagreement only exists when some rule wants it held and another
        // does not. Everyone agreeing is not a conflict, and neither is a field
        // nobody asked for.
        if asking.is_empty() || not_asking.is_empty() {
            continue;
        }
        // `active` is already priority-ordered, so the first asker is the
        // highest-priority one.
        conflicts.push(Conflict {
            field,
            winner: asking[0].rule,
            overridden: not_asking,
            resolution_key: RESOLUTION_STRONGEST_WINS,
        });
    }

    // Battery: the threshold that stops first wins, whatever the priorities.
    let strictest = active
        .iter()
        .filter_map(|rule| {
            rule.battery_stop_percent
                .map(|percent| (percent, rule.rule))
        })
        .max_by_key(|(percent, _)| *percent);
    if let Some((strictest_percent, winner)) = strictest {
        let overridden: Vec<RuleId> = active
            .iter()
            .filter(|rule| rule.battery_stop_percent != Some(strictest_percent))
            .map(|rule| rule.rule)
            .collect();
        if !overridden.is_empty() {
            conflicts.push(Conflict {
                field: PolicyField::BatteryThreshold,
                winner,
                overridden,
                resolution_key: RESOLUTION_EARLIEST_BATTERY_STOP,
            });
        }
    }

    conflicts
}

impl RuleSet {
    /// Evaluates every enabled rule against one sample.
    ///
    /// Takes `&self`: evaluating decides nothing and starts nothing. The caller
    /// turns the returned active list into sessions, which is the only place an
    /// inhibitor can be involved.
    pub fn evaluate(&self, observations: &Observations, now_unix_seconds: u64) -> Evaluation {
        let suppression = self.suppression(now_unix_seconds);

        // Priority first, then the order the user put them in, so the answer is
        // deterministic whatever order the rules were added.
        let mut ordered: Vec<(usize, &Rule)> = self.enabled_rules().enumerate().collect();
        ordered.sort_by(|(left_index, left), (right_index, right)| {
            right
                .priority
                .cmp(&left.priority)
                .then(left_index.cmp(right_index))
        });

        let outcomes: Vec<RuleOutcome> = ordered
            .iter()
            .map(|(_, rule)| outcome_for(rule, observations))
            .collect();

        let active: Vec<ActiveRule> = if suppression.is_some() {
            Vec::new()
        } else {
            ordered
                .iter()
                .zip(&outcomes)
                .filter(|(_, outcome)| outcome.truth.is_true())
                .map(|((_, rule), _)| ActiveRule {
                    rule: rule.id,
                    priority: rule.priority,
                    policy: rule.policy,
                    battery_stop_percent: rule.battery_stop_percent,
                })
                .collect()
        };

        let conflicts = conflicts_for(&active);

        Evaluation {
            outcomes,
            active,
            suppression,
            conflicts,
        }
    }

    /// Evaluates one rule and reports what it would do, without doing it.
    ///
    /// Works on a disabled rule, and on a rule that is currently suppressed,
    /// because both are exactly when someone needs to know whether the rule they
    /// wrote is right. Nothing is started, nothing is stored, and `self` is not
    /// modified.
    pub fn test_rule(
        &self,
        id: RuleId,
        observations: &Observations,
        now_unix_seconds: u64,
    ) -> Result<RuleTest, RuleError> {
        let rule = self.rule(id).ok_or(RuleError::UnknownRule)?;
        let outcome = outcome_for(rule, observations);
        Ok(RuleTest {
            would_be_active: outcome.truth.is_true(),
            outcome,
            suppression: self.suppression(now_unix_seconds),
            rule_disabled: !rule.enabled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        ConditionGroup, PAUSE_SHORT_SECONDS, ProcessMatchKind, ProcessMatcher, Rule, RuleId,
        Schedule, Weekday,
    };
    use crate::session::Reason;

    const NOW: u64 = 1_700_000_000;

    fn process(name: &str) -> Condition {
        Condition::ProcessRunning {
            matcher: ProcessMatcher::new(ProcessMatchKind::ExecutableName, name).unwrap(),
        }
    }

    fn ac(connected: bool) -> Condition {
        Condition::AcPower { connected }
    }

    fn display(connected: bool) -> Condition {
        Condition::ExternalDisplay { connected }
    }

    /// A sample where everything is available and nothing is happening.
    fn quiet() -> Observations {
        let mut observations = Observations::at(NOW);
        observations.running_processes = Some(Vec::new());
        observations.running_desktop_ids = Some(Vec::new());
        observations.ac_power_connected = Some(false);
        observations.battery_percent = Some(80);
        observations.external_display_connected = Some(false);
        observations.audio_playing = Some(false);
        observations.cpu_utilization_percent = Some(3);
        observations.network_kibibytes_per_second = Some(0);
        observations.interfaces_up = Some(vec!["lo".to_string()]);
        observations.local_time = Some(LocalTime {
            weekday: Weekday::Monday,
            minute_of_day: 10 * 60,
        });
        observations.watch_activity = Some(Vec::new());
        observations.fullscreen_active = Some(false);
        for kind in ProviderKind::ALL {
            observations.mark_available(kind);
        }
        observations
    }

    fn rule_with(name: &str, combine: Combine, groups: Vec<ConditionGroup>) -> Rule {
        Rule::new(RuleId(0), Reason::new(name).unwrap(), combine, groups).unwrap()
    }

    fn one_group(name: &str, combine: Combine, conditions: Vec<Condition>) -> Rule {
        rule_with(
            name,
            Combine::All,
            vec![ConditionGroup::new(combine, conditions).unwrap()],
        )
    }

    // ---- Truth tables ----------------------------------------------------

    #[test]
    fn an_and_group_is_true_only_when_every_member_is() {
        let mut observations = quiet();
        let group = ConditionGroup::new(Combine::All, [ac(true), display(true)]).unwrap();

        assert_eq!(evaluate_group(&group, &observations), Truth::False);

        observations.ac_power_connected = Some(true);
        assert_eq!(evaluate_group(&group, &observations), Truth::False);

        observations.external_display_connected = Some(true);
        assert_eq!(evaluate_group(&group, &observations), Truth::True);
    }

    #[test]
    fn an_or_group_is_true_as_soon_as_one_member_is() {
        let mut observations = quiet();
        let group = ConditionGroup::new(Combine::Any, [ac(true), display(true)]).unwrap();

        assert_eq!(evaluate_group(&group, &observations), Truth::False);

        observations.external_display_connected = Some(true);
        assert_eq!(evaluate_group(&group, &observations), Truth::True);

        observations.ac_power_connected = Some(true);
        assert_eq!(evaluate_group(&group, &observations), Truth::True);
    }

    #[test]
    fn an_and_with_a_false_member_is_false_even_when_another_is_unknown() {
        let mut observations = quiet();
        observations.ac_power_connected = None;
        observations.mark_unavailable(ProviderKind::AcPower, "awake.provider.no_power_supply");
        // The display says no, so no amount of power information could make the
        // AND true. Reporting unknown here would hide a decided answer.
        let group = ConditionGroup::new(Combine::All, [ac(true), display(true)]).unwrap();
        assert_eq!(evaluate_group(&group, &observations), Truth::False);
    }

    #[test]
    fn an_and_whose_only_gap_is_unknown_reports_unknown_rather_than_false() {
        let mut observations = quiet();
        observations.external_display_connected = Some(true);
        observations.ac_power_connected = None;
        let group = ConditionGroup::new(Combine::All, [ac(true), display(true)]).unwrap();
        assert_eq!(evaluate_group(&group, &observations), Truth::Unknown);
    }

    #[test]
    fn an_or_with_a_true_member_is_true_even_when_another_is_unknown() {
        let mut observations = quiet();
        observations.external_display_connected = Some(true);
        observations.ac_power_connected = None;
        let group = ConditionGroup::new(Combine::Any, [ac(true), display(true)]).unwrap();
        assert_eq!(evaluate_group(&group, &observations), Truth::True);
    }

    #[test]
    fn an_or_with_no_true_member_and_an_unknown_one_reports_unknown() {
        let mut observations = quiet();
        observations.ac_power_connected = None;
        let group = ConditionGroup::new(Combine::Any, [ac(true), display(true)]).unwrap();
        assert_eq!(evaluate_group(&group, &observations), Truth::Unknown);
    }

    #[test]
    fn groups_combine_with_each_other_by_the_rules_own_operator() {
        let observations = {
            let mut observations = quiet();
            observations.ac_power_connected = Some(true);
            observations
        };
        let groups = vec![
            ConditionGroup::one(ac(true)).unwrap(),
            ConditionGroup::one(display(true)).unwrap(),
        ];

        let all = rule_with("All", Combine::All, groups.clone());
        assert_eq!(outcome_for(&all, &observations).truth, Truth::False);

        let any = rule_with("Any", Combine::Any, groups);
        assert_eq!(outcome_for(&any, &observations).truth, Truth::True);
    }

    #[test]
    fn an_unknown_rule_never_becomes_active() {
        let mut observations = quiet();
        observations.ac_power_connected = None;
        observations.mark_unavailable(ProviderKind::AcPower, "awake.provider.no_power_supply");

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert!(evaluation.active.is_empty());

        let outcome = evaluation.outcome(id).unwrap();
        assert_eq!(outcome.truth, Truth::Unknown);
        assert!(outcome.blocked_by_provider());
        assert_eq!(
            outcome.unavailable_providers,
            vec![(
                ProviderKind::AcPower,
                "awake.provider.no_power_supply".to_string()
            )],
            "a rule that cannot fire must say which provider stopped it"
        );
    }

    #[test]
    fn a_provider_nobody_reported_on_is_unavailable_rather_than_silently_false() {
        let observations = Observations::at(NOW);
        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        let outcome = evaluation.outcome(id).unwrap();
        assert_eq!(outcome.truth, Truth::Unknown);
        assert_eq!(
            outcome.unavailable_providers[0].1,
            "awake.provider.not_sampled"
        );
    }

    // ---- Individual conditions -------------------------------------------

    #[test]
    fn a_process_condition_matches_the_executable_name_case_insensitively() {
        let mut observations = quiet();
        observations.running_processes = Some(vec!["java".to_string(), "gnome-shell".to_string()]);
        assert_eq!(
            evaluate_condition(&process("JAVA"), &observations),
            Truth::True
        );
        assert_eq!(
            evaluate_condition(&process("rustc"), &observations),
            Truth::False
        );
    }

    #[test]
    fn a_desktop_id_condition_reads_the_desktop_list_not_the_process_list() {
        let mut observations = quiet();
        observations.running_processes = Some(vec!["gnome-builder".to_string()]);
        observations.running_desktop_ids = Some(vec!["org.gnome.Builder".to_string()]);

        let by_id = Condition::ProcessRunning {
            matcher: ProcessMatcher::new(ProcessMatchKind::DesktopId, "org.gnome.Builder").unwrap(),
        };
        assert_eq!(evaluate_condition(&by_id, &observations), Truth::True);

        let wrong_list = Condition::ProcessRunning {
            matcher: ProcessMatcher::new(ProcessMatchKind::DesktopId, "gnome-builder").unwrap(),
        };
        assert_eq!(evaluate_condition(&wrong_list, &observations), Truth::False);
    }

    #[test]
    fn a_battery_range_is_inclusive_at_both_ends() {
        let mut observations = quiet();
        let condition = Condition::BatteryPercent {
            at_least: 20,
            at_most: 80,
        };
        for (percent, expected) in [
            (19u8, Truth::False),
            (20, Truth::True),
            (50, Truth::True),
            (80, Truth::True),
            (81, Truth::False),
        ] {
            observations.battery_percent = Some(percent);
            assert_eq!(
                evaluate_condition(&condition, &observations),
                expected,
                "{percent}%"
            );
        }
    }

    #[test]
    fn a_threshold_condition_fires_at_the_threshold_not_above_it() {
        let mut observations = quiet();
        observations.cpu_utilization_percent = Some(50);
        assert_eq!(
            evaluate_condition(
                &Condition::CpuUtilizationAtLeast { percent: 50 },
                &observations
            ),
            Truth::True
        );
        assert_eq!(
            evaluate_condition(
                &Condition::CpuUtilizationAtLeast { percent: 51 },
                &observations
            ),
            Truth::False
        );

        observations.network_kibibytes_per_second = Some(1_024);
        assert_eq!(
            evaluate_condition(
                &Condition::NetworkThroughputAtLeast {
                    kibibytes_per_second: 1_024
                },
                &observations
            ),
            Truth::True
        );
    }

    #[test]
    fn a_watched_path_the_provider_is_not_watching_is_unknown_not_quiet() {
        use crate::rules::WatchedPath;
        let mut observations = quiet();
        observations.watch_activity = Some(vec![WatchActivity {
            path: std::path::PathBuf::from("/home/user/Downloads"),
            last_change_unix_seconds: NOW - 10,
        }]);

        let watched = Condition::WatchedPathActive {
            path: WatchedPath::new("/home/user/Downloads").unwrap(),
            within_seconds: 60,
        };
        assert_eq!(evaluate_condition(&watched, &observations), Truth::True);

        let stale = Condition::WatchedPathActive {
            path: WatchedPath::new("/home/user/Downloads").unwrap(),
            within_seconds: 5,
        };
        assert_eq!(evaluate_condition(&stale, &observations), Truth::False);

        let elsewhere = Condition::WatchedPathActive {
            path: WatchedPath::new("/home/user/Videos").unwrap(),
            within_seconds: 60,
        };
        assert_eq!(
            evaluate_condition(&elsewhere, &observations),
            Truth::Unknown,
            "not watching a path is not the same as knowing it is idle"
        );
    }

    #[test]
    fn a_schedule_condition_reads_the_local_clock_the_caller_supplied() {
        let mut observations = quiet();
        let condition = Condition::TimeSchedule {
            schedule: Schedule::new([Weekday::Monday], 9 * 60, 17 * 60).unwrap(),
        };
        assert_eq!(evaluate_condition(&condition, &observations), Truth::True);

        observations.local_time = Some(LocalTime {
            weekday: Weekday::Sunday,
            minute_of_day: 10 * 60,
        });
        assert_eq!(evaluate_condition(&condition, &observations), Truth::False);

        observations.local_time = None;
        assert_eq!(
            evaluate_condition(&condition, &observations),
            Truth::Unknown
        );
    }

    // ---- Priority, ordering, conflicts ------------------------------------

    #[test]
    fn outcomes_are_ordered_by_priority_and_then_by_the_users_own_order() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let low = set
            .add(one_group("Low", Combine::All, vec![ac(true)]))
            .unwrap();
        let high = set
            .add(one_group("High", Combine::All, vec![ac(true)]))
            .unwrap();
        let also_low = set
            .add(one_group("Also low", Combine::All, vec![ac(true)]))
            .unwrap();
        set.set_priority(high, 90).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert_eq!(
            evaluation
                .outcomes
                .iter()
                .map(|outcome| outcome.rule)
                .collect::<Vec<_>>(),
            vec![high, low, also_low]
        );
        assert_eq!(
            evaluation
                .active
                .iter()
                .map(|active| active.rule)
                .collect::<Vec<_>>(),
            vec![high, low, also_low]
        );
    }

    #[test]
    fn a_disagreement_names_the_winner_and_the_rules_it_outranked() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let quiet_rule = set
            .add(one_group("Quiet", Combine::All, vec![ac(true)]))
            .unwrap();

        let mut presenting = one_group("Presenting", Combine::All, vec![ac(true)]);
        presenting.policy = SessionPolicy {
            prevent_display_sleep: true,
            ..SessionPolicy::quick_default()
        };
        presenting.priority = 90;
        let presenting_id = set.add(presenting).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        let conflict = evaluation
            .conflicts
            .iter()
            .find(|conflict| conflict.field == PolicyField::DisplaySleep)
            .expect("the display disagreement must be explained");
        assert_eq!(conflict.winner, presenting_id);
        assert_eq!(conflict.overridden, vec![quiet_rule]);
        assert_eq!(conflict.resolution_key, RESOLUTION_STRONGEST_WINS);
        assert!(evaluation.merged_policy().prevent_display_sleep);
    }

    #[test]
    fn priority_never_weakens_battery_protection() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let mut cautious = one_group("Cautious", Combine::All, vec![ac(true)]);
        cautious.battery_stop_percent = Some(40);
        cautious.priority = 1;
        let cautious_id = set.add(cautious).unwrap();

        let mut reckless = one_group("Reckless", Combine::All, vec![ac(true)]);
        reckless.battery_stop_percent = Some(5);
        reckless.priority = 99;
        let reckless_id = set.add(reckless).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert_eq!(
            evaluation.merged_battery_stop_percent(),
            Some(40),
            "the highest-priority rule asked for 5%, and it must not win"
        );
        let conflict = evaluation
            .conflicts
            .iter()
            .find(|conflict| conflict.field == PolicyField::BatteryThreshold)
            .expect("the threshold disagreement must be explained");
        assert_eq!(conflict.winner, cautious_id);
        assert_eq!(conflict.overridden, vec![reckless_id]);
        assert_eq!(conflict.resolution_key, RESOLUTION_EARLIEST_BATTERY_STOP);
    }

    #[test]
    fn rules_that_agree_produce_no_conflict_to_explain() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        set.add(one_group("A", Combine::All, vec![ac(true)]))
            .unwrap();
        set.add(one_group("B", Combine::All, vec![ac(true)]))
            .unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert_eq!(evaluation.active.len(), 2);
        assert!(
            evaluation.conflicts.is_empty(),
            "two rules asking for the same thing did not disagree about anything"
        );
    }

    #[test]
    fn a_single_active_rule_has_nothing_to_conflict_with() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);
        let mut set = RuleSet::new();
        set.add(one_group("Only", Combine::All, vec![ac(true)]))
            .unwrap();
        assert!(set.evaluate(&observations, NOW).conflicts.is_empty());
    }

    // ---- Suppression -------------------------------------------------------

    #[test]
    fn a_paused_rule_set_activates_nothing_and_says_why() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        set.pause_for(PAUSE_SHORT_SECONDS, NOW).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert!(evaluation.active.is_empty());
        assert_eq!(
            evaluation.suppression,
            Some(Suppression::PausedUntil {
                unix_seconds: NOW + PAUSE_SHORT_SECONDS
            })
        );
        assert_eq!(
            evaluation.outcome(id).unwrap().truth,
            Truth::True,
            "the rule still matches; it is simply not allowed to act"
        );

        // Once the pause is over the rule takes hold again on its own.
        let later = set.evaluate(&observations, NOW + PAUSE_SHORT_SECONDS);
        assert_eq!(later.active.len(), 1);
        assert_eq!(later.suppression, None);
    }

    #[test]
    fn an_override_activates_nothing_until_it_is_resumed() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        set.add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        set.override_all(true).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert!(evaluation.active.is_empty());
        assert_eq!(evaluation.suppression, Some(Suppression::Overridden));

        set.resume();
        assert_eq!(set.evaluate(&observations, NOW).active.len(), 1);
    }

    #[test]
    fn a_disabled_rule_is_not_evaluated_at_all() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        set.set_enabled(id, false).unwrap();

        let evaluation = set.evaluate(&observations, NOW);
        assert!(evaluation.outcomes.is_empty());
        assert!(evaluation.active.is_empty());
    }

    // ---- Test mode ---------------------------------------------------------

    #[test]
    fn testing_a_rule_reports_what_it_would_do_and_changes_nothing() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        let before = set.clone();

        let test = set.test_rule(id, &observations, NOW).unwrap();
        assert!(test.would_be_active);
        assert_eq!(test.outcome.truth, Truth::True);
        assert!(!test.rule_disabled);
        assert_eq!(test.suppression, None);
        assert_eq!(set, before, "testing a rule must not modify the rule set");
    }

    #[test]
    fn a_disabled_rule_can_still_be_tested_and_says_it_is_switched_off() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        set.set_enabled(id, false).unwrap();

        let test = set.test_rule(id, &observations, NOW).unwrap();
        assert!(
            test.would_be_active,
            "the conditions are met; only the switch is off"
        );
        assert!(test.rule_disabled);
    }

    #[test]
    fn testing_a_rule_while_everything_is_paused_reports_the_pause_separately() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(one_group("On AC", Combine::All, vec![ac(true)]))
            .unwrap();
        set.pause_until_resumed();

        let test = set.test_rule(id, &observations, NOW).unwrap();
        assert!(test.would_be_active);
        assert_eq!(test.suppression, Some(Suppression::PausedUntilResumed));
    }

    #[test]
    fn testing_a_rule_that_is_not_there_is_refused() {
        let set = RuleSet::new();
        assert_eq!(
            set.test_rule(RuleId(1), &quiet(), NOW),
            Err(RuleError::UnknownRule)
        );
    }

    #[test]
    fn testing_a_rule_explains_each_group_separately() {
        let mut observations = quiet();
        observations.ac_power_connected = Some(true);

        let mut set = RuleSet::new();
        let id = set
            .add(rule_with(
                "Two groups",
                Combine::All,
                vec![
                    ConditionGroup::one(ac(true)).unwrap(),
                    ConditionGroup::one(display(true)).unwrap(),
                ],
            ))
            .unwrap();

        let test = set.test_rule(id, &observations, NOW).unwrap();
        assert_eq!(test.outcome.group_truths, vec![Truth::True, Truth::False]);
        assert!(!test.would_be_active);
    }
}

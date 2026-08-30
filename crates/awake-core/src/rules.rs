//! Automatic trigger rules: what one is, and what a set of them allows.
//!
//! A rule is data, never a command. Every condition is a closed enum variant
//! with validated operands, so nothing a user types can become a shell string,
//! a path traversal, or an unbounded process query. Issue #13 forbids arbitrary
//! shell commands as a rule action or condition in any form; the way that is
//! enforced here is that there is no variant capable of carrying one.
//!
//! Evaluation lives in [`crate::evaluate`]. This module owns the model, the
//! editing operations the GUI needs, and the two suspension mechanisms — pause
//! and override — that the tray drives.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::policy::SessionPolicy;
use crate::session::{DEFAULT_BATTERY_STOP_PERCENT, Reason, ReasonError};

/// More rules than a person will ever write by hand. A larger set is a
/// generated file or a mistake, and either way is refused rather than evaluated
/// on every tick.
pub const MAX_RULES: usize = 64;

/// Condition groups inside one rule.
pub const MAX_GROUPS_PER_RULE: usize = 8;

/// Conditions inside one group.
pub const MAX_CONDITIONS_PER_GROUP: usize = 16;

/// A process name or desktop identifier is a token, not a paragraph.
pub const MAX_MATCHER_CHARS: usize = 64;

/// Long enough for any real path, short enough that a rule file cannot become a
/// denial-of-service through a million-character string.
pub const MAX_WATCHED_PATH_CHARS: usize = 4_096;

/// `IFNAMSIZ` minus the terminator: the kernel cannot name an interface longer.
pub const MAX_INTERFACE_CHARS: usize = 15;

/// The two pause lengths Issue #13 names, plus "until resumed" as its own case.
pub const PAUSE_SHORT_SECONDS: u64 = 15 * 60;
pub const PAUSE_LONG_SECONDS: u64 = 60 * 60;

/// Identity of a rule, stable across edits so history and the tray can name the
/// same rule after it has been renamed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(pub u64);

impl std::fmt::Display for RuleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Where a condition's answer comes from.
///
/// This is the capability unit: a provider reports itself available or not, and
/// every condition naming an unavailable provider is explained rather than
/// silently treated as false.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    ProcessRunning,
    AcPower,
    BatteryPercent,
    ExternalDisplay,
    AudioPlayback,
    CpuUtilization,
    NetworkThroughput,
    NetworkInterface,
    TimeSchedule,
    WatchedPath,
    Fullscreen,
}

impl ProviderKind {
    /// Every provider Issue #13's initial list names, in the order the GUI
    /// offers them.
    pub const ALL: [ProviderKind; 11] = [
        ProviderKind::ProcessRunning,
        ProviderKind::AcPower,
        ProviderKind::BatteryPercent,
        ProviderKind::ExternalDisplay,
        ProviderKind::AudioPlayback,
        ProviderKind::CpuUtilization,
        ProviderKind::NetworkThroughput,
        ProviderKind::NetworkInterface,
        ProviderKind::TimeSchedule,
        ProviderKind::WatchedPath,
        ProviderKind::Fullscreen,
    ];

    /// A stable key. Presentation layers own the wording.
    pub fn as_key(self) -> &'static str {
        match self {
            ProviderKind::ProcessRunning => "process_running",
            ProviderKind::AcPower => "ac_power",
            ProviderKind::BatteryPercent => "battery_percent",
            ProviderKind::ExternalDisplay => "external_display",
            ProviderKind::AudioPlayback => "audio_playback",
            ProviderKind::CpuUtilization => "cpu_utilization",
            ProviderKind::NetworkThroughput => "network_throughput",
            ProviderKind::NetworkInterface => "network_interface",
            ProviderKind::TimeSchedule => "time_schedule",
            ProviderKind::WatchedPath => "watched_path",
            ProviderKind::Fullscreen => "fullscreen",
        }
    }
}

/// How a process is recognized.
///
/// Only the executable name and the desktop identifier are matchable. A command
/// line is deliberately not, because matching one would mean reading and
/// storing arguments that routinely carry tokens, file paths, and passwords.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMatchKind {
    /// `/proc/<pid>/comm`, which is the kernel's own short name.
    ExecutableName,
    /// A `.desktop` identifier such as `org.gnome.Builder`.
    DesktopId,
}

/// A validated process matcher.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessMatcher {
    pub kind: ProcessMatchKind,
    value: String,
}

impl ProcessMatcher {
    pub fn new(kind: ProcessMatchKind, value: impl Into<String>) -> Result<Self, RuleError> {
        let value: String = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(RuleError::EmptyMatcher);
        }
        if trimmed.chars().count() > MAX_MATCHER_CHARS {
            return Err(RuleError::MatcherTooLong);
        }
        // A name, never a path: no separator can appear, so no matcher can be
        // aimed at a directory walk or at anything outside `/proc/<pid>/comm`.
        if trimmed
            .chars()
            .any(|character| character.is_control() || character == '/' || character == '\\')
        {
            return Err(RuleError::InvalidMatcher);
        }
        Ok(Self {
            kind,
            value: trimmed.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Case-insensitive, because `Code` and `code` are the same application to
    /// the person writing the rule.
    pub fn matches(&self, candidate: &str) -> bool {
        self.value.eq_ignore_ascii_case(candidate.trim())
    }
}

/// A validated network interface name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InterfaceName(String);

impl InterfaceName {
    pub fn new(value: impl Into<String>) -> Result<Self, RuleError> {
        let value: String = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_INTERFACE_CHARS {
            return Err(RuleError::InvalidInterface);
        }
        if !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character))
        {
            return Err(RuleError::InvalidInterface);
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for InterfaceName {
    type Error = RuleError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        InterfaceName::new(value)
    }
}

impl From<InterfaceName> for String {
    fn from(name: InterfaceName) -> Self {
        name.0
    }
}

/// A validated absolute path to watch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PathBuf", into = "PathBuf")]
pub struct WatchedPath(PathBuf);

impl WatchedPath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, RuleError> {
        let path: PathBuf = path.into();
        let Some(text) = path.to_str() else {
            return Err(RuleError::InvalidWatchedPath);
        };
        if text.is_empty() || text.chars().count() > MAX_WATCHED_PATH_CHARS {
            return Err(RuleError::InvalidWatchedPath);
        }
        if text.chars().any(char::is_control) {
            return Err(RuleError::InvalidWatchedPath);
        }
        // Relative paths and `..` segments would resolve against whatever
        // directory the service happened to start in, which is not a thing a
        // user can reason about.
        if !path.is_absolute() || path.components().any(|c| c.as_os_str() == "..") {
            return Err(RuleError::InvalidWatchedPath);
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl TryFrom<PathBuf> for WatchedPath {
    type Error = RuleError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        WatchedPath::new(value)
    }
}

impl From<WatchedPath> for PathBuf {
    fn from(path: WatchedPath) -> Self {
        path.0
    }
}

/// Days of the week, Monday first, matching ISO 8601.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];

    pub fn index(self) -> u8 {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }

    /// The day before this one, needed because a window that crosses midnight
    /// belongs to the day it started on.
    pub fn previous(self) -> Weekday {
        Weekday::ALL[((self.index() + 6) % 7) as usize]
    }
}

/// Minutes in a day, so a schedule can be validated without a calendar.
pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// A recurring local-time window.
///
/// The window is half-open: it starts at `from_minute_of_day` and ends the
/// minute before `to_minute_of_day`, so two adjacent schedules never both claim
/// the same minute. A window whose end is before its start crosses midnight and
/// belongs, for the whole of its length, to the day it began on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    /// The days the window may start on. Empty is refused, because a schedule
    /// that can never start is a mistake, not a rule that is simply off.
    pub days: Vec<Weekday>,
    pub from_minute_of_day: u16,
    pub to_minute_of_day: u16,
}

impl Schedule {
    pub fn new(
        days: impl IntoIterator<Item = Weekday>,
        from_minute_of_day: u16,
        to_minute_of_day: u16,
    ) -> Result<Self, RuleError> {
        let mut days: Vec<Weekday> = days.into_iter().collect();
        days.sort();
        days.dedup();
        let schedule = Self {
            days,
            from_minute_of_day,
            to_minute_of_day,
        };
        schedule.validate()?;
        Ok(schedule)
    }

    pub fn validate(&self) -> Result<(), RuleError> {
        if self.days.is_empty() {
            return Err(RuleError::EmptySchedule);
        }
        if self.from_minute_of_day >= MINUTES_PER_DAY || self.to_minute_of_day >= MINUTES_PER_DAY {
            return Err(RuleError::InvalidSchedule);
        }
        if self.from_minute_of_day == self.to_minute_of_day {
            // Zero length, not "all day". All day is 0 to 1439 inclusive, which
            // is expressed as from 0 to 1439.
            return Err(RuleError::InvalidSchedule);
        }
        Ok(())
    }

    fn starts_on(&self, day: Weekday) -> bool {
        self.days.contains(&day)
    }

    /// Whether this local moment falls inside the window.
    pub fn contains(&self, now: LocalTime) -> bool {
        if self.from_minute_of_day < self.to_minute_of_day {
            self.starts_on(now.weekday)
                && (self.from_minute_of_day..self.to_minute_of_day).contains(&now.minute_of_day)
        } else {
            // Crosses midnight. Before midnight it belongs to today; after
            // midnight it still belongs to the day it started on, which is
            // yesterday.
            (self.starts_on(now.weekday) && now.minute_of_day >= self.from_minute_of_day)
                || (self.starts_on(now.weekday.previous())
                    && now.minute_of_day < self.to_minute_of_day)
        }
    }
}

/// The local wall clock, resolved outside this crate.
///
/// `awake-core` stays clock-free and timezone-free: whatever reads the system
/// clock converts once and passes the answer in, which is why every schedule
/// case here is testable without changing the machine's timezone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTime {
    pub weekday: Weekday,
    pub minute_of_day: u16,
}

/// One thing that must be true.
///
/// There is no variant that carries a command, a script, an interpreter, or a
/// format string. That is the enforcement of Issue #13's rule, not a convention
/// on top of a more general type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "condition", deny_unknown_fields)]
pub enum Condition {
    ProcessRunning {
        matcher: ProcessMatcher,
    },
    AcPower {
        connected: bool,
    },
    /// Inclusive on both ends, so `20..=20` means exactly twenty percent.
    BatteryPercent {
        at_least: u8,
        at_most: u8,
    },
    ExternalDisplay {
        connected: bool,
    },
    AudioPlayback {
        playing: bool,
    },
    CpuUtilizationAtLeast {
        percent: u8,
    },
    NetworkThroughputAtLeast {
        kibibytes_per_second: u64,
    },
    NetworkInterfaceUp {
        interface: InterfaceName,
    },
    TimeSchedule {
        schedule: Schedule,
    },
    /// True while the watched file or directory changed within the window.
    WatchedPathActive {
        path: WatchedPath,
        within_seconds: u64,
    },
    Fullscreen {
        active: bool,
    },
}

/// The longest quiet window a watched-path condition may wait out. Beyond this
/// the rule is not watching for activity, it is staying on.
pub const MAX_WATCH_WINDOW_SECONDS: u64 = 24 * 60 * 60;

impl Condition {
    pub fn provider(&self) -> ProviderKind {
        match self {
            Condition::ProcessRunning { .. } => ProviderKind::ProcessRunning,
            Condition::AcPower { .. } => ProviderKind::AcPower,
            Condition::BatteryPercent { .. } => ProviderKind::BatteryPercent,
            Condition::ExternalDisplay { .. } => ProviderKind::ExternalDisplay,
            Condition::AudioPlayback { .. } => ProviderKind::AudioPlayback,
            Condition::CpuUtilizationAtLeast { .. } => ProviderKind::CpuUtilization,
            Condition::NetworkThroughputAtLeast { .. } => ProviderKind::NetworkThroughput,
            Condition::NetworkInterfaceUp { .. } => ProviderKind::NetworkInterface,
            Condition::TimeSchedule { .. } => ProviderKind::TimeSchedule,
            Condition::WatchedPathActive { .. } => ProviderKind::WatchedPath,
            Condition::Fullscreen { .. } => ProviderKind::Fullscreen,
        }
    }

    pub fn validate(&self) -> Result<(), RuleError> {
        match self {
            Condition::BatteryPercent { at_least, at_most } => {
                if at_least > at_most || *at_most > 100 {
                    return Err(RuleError::InvalidBatteryRange);
                }
                Ok(())
            }
            Condition::CpuUtilizationAtLeast { percent } => {
                if *percent > 100 {
                    Err(RuleError::InvalidCpuThreshold)
                } else {
                    Ok(())
                }
            }
            Condition::TimeSchedule { schedule } => schedule.validate(),
            Condition::WatchedPathActive { within_seconds, .. } => {
                if *within_seconds == 0 || *within_seconds > MAX_WATCH_WINDOW_SECONDS {
                    Err(RuleError::InvalidWatchWindow)
                } else {
                    Ok(())
                }
            }
            Condition::ProcessRunning { .. }
            | Condition::AcPower { .. }
            | Condition::ExternalDisplay { .. }
            | Condition::AudioPlayback { .. }
            | Condition::NetworkThroughputAtLeast { .. }
            | Condition::NetworkInterfaceUp { .. }
            | Condition::Fullscreen { .. } => Ok(()),
        }
    }
}

/// How the members of a group, or the groups of a rule, combine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Combine {
    /// AND.
    All,
    /// OR.
    Any,
}

impl Combine {
    pub fn as_key(self) -> &'static str {
        match self {
            Combine::All => "all",
            Combine::Any => "any",
        }
    }
}

/// One AND/OR bracket of conditions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionGroup {
    pub combine: Combine,
    pub conditions: Vec<Condition>,
}

impl ConditionGroup {
    pub fn new(
        combine: Combine,
        conditions: impl IntoIterator<Item = Condition>,
    ) -> Result<Self, RuleError> {
        let group = Self {
            combine,
            conditions: conditions.into_iter().collect(),
        };
        group.validate()?;
        Ok(group)
    }

    /// A single condition, which is the shape most rules actually have.
    pub fn one(condition: Condition) -> Result<Self, RuleError> {
        ConditionGroup::new(Combine::All, [condition])
    }

    pub fn validate(&self) -> Result<(), RuleError> {
        if self.conditions.is_empty() {
            // An empty AND is vacuously true, which would keep the machine awake
            // forever for no stated reason. It is refused instead.
            return Err(RuleError::EmptyGroup);
        }
        if self.conditions.len() > MAX_CONDITIONS_PER_GROUP {
            return Err(RuleError::TooManyConditions);
        }
        for condition in &self.conditions {
            condition.validate()?;
        }
        Ok(())
    }

    pub fn providers(&self) -> Vec<ProviderKind> {
        let mut providers: Vec<ProviderKind> =
            self.conditions.iter().map(Condition::provider).collect();
        providers.sort();
        providers.dedup();
        providers
    }
}

/// Priority orders how rules are presented and which one is named as the source
/// of an effective policy. It never weakens a protection: see
/// [`crate::evaluate::Conflict`] for exactly what it does and does not decide.
pub const DEFAULT_PRIORITY: u8 = 50;

/// One automatic rule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: RuleId,
    /// The name is also the reason the resulting session records, which is why
    /// it is a [`Reason`]: it is already bounded, control-character free, and
    /// legal to hand to an inhibitor backend.
    pub name: Reason,
    pub enabled: bool,
    /// Higher is more important. Ties fall back to position in the list.
    pub priority: u8,
    /// How the groups combine with each other.
    pub combine: Combine,
    pub groups: Vec<ConditionGroup>,
    pub policy: SessionPolicy,
    #[serde(default)]
    pub battery_stop_percent: Option<u8>,
}

impl Rule {
    /// A rule with the default quick-session policy and battery protection on.
    pub fn new(
        id: RuleId,
        name: Reason,
        combine: Combine,
        groups: impl IntoIterator<Item = ConditionGroup>,
    ) -> Result<Self, RuleError> {
        let rule = Self {
            id,
            name,
            enabled: true,
            priority: DEFAULT_PRIORITY,
            combine,
            groups: groups.into_iter().collect(),
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(DEFAULT_BATTERY_STOP_PERCENT),
        };
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<(), RuleError> {
        if self.groups.is_empty() {
            return Err(RuleError::EmptyRule);
        }
        if self.groups.len() > MAX_GROUPS_PER_RULE {
            return Err(RuleError::TooManyGroups);
        }
        for group in &self.groups {
            group.validate()?;
        }
        if let Some(percent) = self.battery_stop_percent
            && (percent == 0 || percent >= 100)
        {
            return Err(RuleError::InvalidBatteryThreshold);
        }
        Ok(())
    }

    /// Every provider this rule needs a reading from.
    pub fn providers(&self) -> Vec<ProviderKind> {
        let mut providers: Vec<ProviderKind> = self
            .groups
            .iter()
            .flat_map(|group| group.conditions.iter().map(Condition::provider))
            .collect();
        providers.sort();
        providers.dedup();
        providers
    }

    /// The reason a session started by this rule records.
    pub fn reason(&self) -> Reason {
        self.name.clone()
    }
}

/// Why automatic rules are not producing sessions right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Suppression {
    /// Paused until a moment that has not arrived.
    PausedUntil { unix_seconds: u64 },
    /// Paused with no end, until someone resumes.
    PausedUntilResumed,
    /// Every rule is overridden, which only an explicitly confirmed request can
    /// do.
    Overridden,
}

impl Suppression {
    pub fn as_key(self) -> &'static str {
        match self {
            Suppression::PausedUntil { .. } => "paused_until",
            Suppression::PausedUntilResumed => "paused_until_resumed",
            Suppression::Overridden => "overridden",
        }
    }
}

/// How rules are currently suspended, if they are.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PauseState {
    #[default]
    Running,
    Until {
        unix_seconds: u64,
    },
    UntilResumed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum RuleError {
    #[error("awake.rule.error.unknown_rule")]
    UnknownRule,
    #[error("awake.rule.error.too_many_rules")]
    TooManyRules,
    #[error("awake.rule.error.empty_rule")]
    EmptyRule,
    #[error("awake.rule.error.empty_group")]
    EmptyGroup,
    #[error("awake.rule.error.too_many_groups")]
    TooManyGroups,
    #[error("awake.rule.error.too_many_conditions")]
    TooManyConditions,
    #[error("awake.rule.error.empty_matcher")]
    EmptyMatcher,
    #[error("awake.rule.error.matcher_too_long")]
    MatcherTooLong,
    #[error("awake.rule.error.invalid_matcher")]
    InvalidMatcher,
    #[error("awake.rule.error.invalid_interface")]
    InvalidInterface,
    #[error("awake.rule.error.invalid_watched_path")]
    InvalidWatchedPath,
    #[error("awake.rule.error.invalid_watch_window")]
    InvalidWatchWindow,
    #[error("awake.rule.error.invalid_battery_range")]
    InvalidBatteryRange,
    #[error("awake.rule.error.invalid_battery_threshold")]
    InvalidBatteryThreshold,
    #[error("awake.rule.error.invalid_cpu_threshold")]
    InvalidCpuThreshold,
    #[error("awake.rule.error.empty_schedule")]
    EmptySchedule,
    #[error("awake.rule.error.invalid_schedule")]
    InvalidSchedule,
    #[error("awake.rule.error.invalid_name:{0}")]
    InvalidName(ReasonError),
    #[error("awake.rule.error.invalid_position")]
    InvalidPosition,
    #[error("awake.rule.error.override_confirmation_required")]
    OverrideConfirmationRequired,
    #[error("awake.rule.error.invalid_pause_duration")]
    InvalidPauseDuration,
}

impl From<ReasonError> for RuleError {
    fn from(error: ReasonError) -> Self {
        RuleError::InvalidName(error)
    }
}

/// Every rule the user has, in the order they chose, plus the two ways the whole
/// set can be suspended.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    rules: Vec<Rule>,
    next_id: u64,
    #[serde(default)]
    pause: PauseState,
    /// Set only by an explicitly confirmed override. Cleared by resuming.
    #[serde(default)]
    overridden: bool,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleSet {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            next_id: 1,
            pause: PauseState::Running,
            overridden: false,
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn rule(&self, id: RuleId) -> Option<&Rule> {
        self.rules.iter().find(|rule| rule.id == id)
    }

    /// The rules that would be considered on the next evaluation, ignoring
    /// suspension. Enabled only.
    pub fn enabled_rules(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter().filter(|rule| rule.enabled)
    }

    /// The id the next added rule will receive, exposed so a caller building a
    /// rule to hand to [`RuleSet::add`] can name it first.
    pub fn next_id(&self) -> RuleId {
        RuleId(self.next_id)
    }

    /// Adds a rule, giving it the next free id whatever id it arrived with.
    ///
    /// Ids are assigned here rather than by the caller so two clients editing at
    /// once cannot mint the same one.
    pub fn add(&mut self, mut rule: Rule) -> Result<RuleId, RuleError> {
        if self.rules.len() >= MAX_RULES {
            return Err(RuleError::TooManyRules);
        }
        rule.validate()?;
        let id = RuleId(self.next_id);
        self.next_id += 1;
        rule.id = id;
        self.rules.push(rule);
        Ok(id)
    }

    /// Replaces everything about a rule except its identity and its position.
    pub fn replace(&mut self, id: RuleId, mut rule: Rule) -> Result<(), RuleError> {
        rule.validate()?;
        let index = self.index_of(id)?;
        rule.id = id;
        self.rules[index] = rule;
        Ok(())
    }

    pub fn remove(&mut self, id: RuleId) -> Result<Rule, RuleError> {
        let index = self.index_of(id)?;
        Ok(self.rules.remove(index))
    }

    pub fn set_enabled(&mut self, id: RuleId, enabled: bool) -> Result<(), RuleError> {
        let index = self.index_of(id)?;
        self.rules[index].enabled = enabled;
        Ok(())
    }

    /// Copies a rule, disabled, directly after the original.
    ///
    /// The copy starts disabled because a duplicate is made in order to be
    /// edited, and an identical second rule that is immediately live would keep
    /// the machine awake twice for one reason.
    pub fn duplicate(&mut self, id: RuleId) -> Result<RuleId, RuleError> {
        if self.rules.len() >= MAX_RULES {
            return Err(RuleError::TooManyRules);
        }
        let index = self.index_of(id)?;
        let mut copy = self.rules[index].clone();
        let new_id = RuleId(self.next_id);
        self.next_id += 1;
        copy.id = new_id;
        copy.enabled = false;
        self.rules.insert(index + 1, copy);
        Ok(new_id)
    }

    /// Moves a rule to an absolute position in the list.
    pub fn reorder(&mut self, id: RuleId, to_index: usize) -> Result<(), RuleError> {
        let from = self.index_of(id)?;
        if to_index >= self.rules.len() {
            return Err(RuleError::InvalidPosition);
        }
        let rule = self.rules.remove(from);
        self.rules.insert(to_index, rule);
        Ok(())
    }

    pub fn set_priority(&mut self, id: RuleId, priority: u8) -> Result<(), RuleError> {
        let index = self.index_of(id)?;
        self.rules[index].priority = priority;
        Ok(())
    }

    /// Pauses every rule for a bounded time.
    ///
    /// Only the two lengths Issue #13 names are accepted, so a client cannot
    /// quietly turn "pause" into "off forever" by passing a large number; that
    /// is what [`RuleSet::pause_until_resumed`] is for, and it says so.
    pub fn pause_for(&mut self, seconds: u64, now_unix_seconds: u64) -> Result<(), RuleError> {
        if seconds != PAUSE_SHORT_SECONDS && seconds != PAUSE_LONG_SECONDS {
            return Err(RuleError::InvalidPauseDuration);
        }
        self.pause = PauseState::Until {
            unix_seconds: now_unix_seconds.saturating_add(seconds),
        };
        Ok(())
    }

    pub fn pause_until_resumed(&mut self) {
        self.pause = PauseState::UntilResumed;
    }

    /// Ends a pause and any override. One control resumes automatic rules,
    /// whichever way they were stopped, because a user who presses Resume means
    /// "start behaving normally again".
    pub fn resume(&mut self) {
        self.pause = PauseState::Running;
        self.overridden = false;
    }

    /// Suspends every rule until resumed.
    ///
    /// Refused without `confirmed`, because overriding every rule at once is the
    /// one control that can silently undo protection the user set up on purpose.
    pub fn override_all(&mut self, confirmed: bool) -> Result<(), RuleError> {
        if !confirmed {
            return Err(RuleError::OverrideConfirmationRequired);
        }
        self.overridden = true;
        Ok(())
    }

    pub fn is_overridden(&self) -> bool {
        self.overridden
    }

    /// The raw pause record, which may name a moment that has already passed.
    pub fn pause_state(&self) -> PauseState {
        self.pause
    }

    /// Why rules are suspended right now, or `None` when they are running.
    ///
    /// An override outranks a pause in the explanation because it is the more
    /// deliberate of the two.
    pub fn suppression(&self, now_unix_seconds: u64) -> Option<Suppression> {
        if self.overridden {
            return Some(Suppression::Overridden);
        }
        match self.pause {
            PauseState::Running => None,
            PauseState::UntilResumed => Some(Suppression::PausedUntilResumed),
            PauseState::Until { unix_seconds } if unix_seconds > now_unix_seconds => {
                Some(Suppression::PausedUntil { unix_seconds })
            }
            PauseState::Until { .. } => None,
        }
    }

    /// Clears a pause whose moment has passed, so the stored state does not keep
    /// a stale timestamp forever. Returns whether anything changed.
    pub fn expire_pause(&mut self, now_unix_seconds: u64) -> bool {
        if let PauseState::Until { unix_seconds } = self.pause
            && unix_seconds <= now_unix_seconds
        {
            self.pause = PauseState::Running;
            return true;
        }
        false
    }

    /// Every provider any enabled rule needs. Nothing else has to be sampled.
    pub fn required_providers(&self) -> Vec<ProviderKind> {
        let mut providers: Vec<ProviderKind> = self
            .enabled_rules()
            .flat_map(|rule| rule.providers())
            .collect();
        providers.sort();
        providers.dedup();
        providers
    }

    fn index_of(&self, id: RuleId) -> Result<usize, RuleError> {
        self.rules
            .iter()
            .position(|rule| rule.id == id)
            .ok_or(RuleError::UnknownRule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Reason {
        Reason::new(name).unwrap()
    }

    fn on_ac() -> Condition {
        Condition::AcPower { connected: true }
    }

    pub(crate) fn rule(name: &str) -> Rule {
        Rule::new(
            RuleId(0),
            named(name),
            Combine::All,
            [ConditionGroup::one(on_ac()).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn a_process_matcher_is_a_name_and_never_a_path() {
        assert!(
            ProcessMatcher::new(ProcessMatchKind::ExecutableName, " java ")
                .unwrap()
                .matches("JAVA")
        );
        for rejected in ["", "   ", "/usr/bin/java", "..\\evil", "a\u{0}b"] {
            assert!(
                ProcessMatcher::new(ProcessMatchKind::ExecutableName, rejected).is_err(),
                "{rejected:?} must be refused"
            );
        }
        assert_eq!(
            ProcessMatcher::new(
                ProcessMatchKind::DesktopId,
                "a".repeat(MAX_MATCHER_CHARS + 1)
            ),
            Err(RuleError::MatcherTooLong)
        );
    }

    #[test]
    fn a_watched_path_must_be_absolute_and_free_of_parent_segments() {
        assert!(WatchedPath::new("/home/user/Downloads").is_ok());
        for rejected in ["", "relative/path", "/home/../etc/shadow"] {
            assert_eq!(
                WatchedPath::new(rejected),
                Err(RuleError::InvalidWatchedPath),
                "{rejected:?}"
            );
        }
    }

    #[test]
    fn an_interface_name_cannot_exceed_what_the_kernel_can_hold() {
        assert!(InterfaceName::new("wlp0s20f3").is_ok());
        assert!(InterfaceName::new("a".repeat(MAX_INTERFACE_CHARS)).is_ok());
        assert_eq!(
            InterfaceName::new("a".repeat(MAX_INTERFACE_CHARS + 1)),
            Err(RuleError::InvalidInterface)
        );
        assert_eq!(
            InterfaceName::new("eth 0"),
            Err(RuleError::InvalidInterface),
            "a space is not a legal interface character"
        );
    }

    #[test]
    fn a_schedule_inside_one_day_covers_a_half_open_window() {
        let schedule = Schedule::new([Weekday::Monday], 9 * 60, 17 * 60).unwrap();
        assert!(schedule.contains(LocalTime {
            weekday: Weekday::Monday,
            minute_of_day: 9 * 60,
        }));
        assert!(schedule.contains(LocalTime {
            weekday: Weekday::Monday,
            minute_of_day: 17 * 60 - 1,
        }));
        assert!(
            !schedule.contains(LocalTime {
                weekday: Weekday::Monday,
                minute_of_day: 17 * 60,
            }),
            "the end minute belongs to the next window, not this one"
        );
        assert!(!schedule.contains(LocalTime {
            weekday: Weekday::Tuesday,
            minute_of_day: 10 * 60,
        }));
    }

    #[test]
    fn a_schedule_that_crosses_midnight_belongs_to_the_day_it_started_on() {
        // Friday 22:00 to 02:00. Saturday at 01:00 is inside it; Friday at 01:00
        // is not, because that window started on Thursday, which is not selected.
        let schedule = Schedule::new([Weekday::Friday], 22 * 60, 2 * 60).unwrap();
        assert!(schedule.contains(LocalTime {
            weekday: Weekday::Friday,
            minute_of_day: 23 * 60,
        }));
        assert!(schedule.contains(LocalTime {
            weekday: Weekday::Saturday,
            minute_of_day: 60,
        }));
        assert!(!schedule.contains(LocalTime {
            weekday: Weekday::Friday,
            minute_of_day: 60,
        }));
        assert!(!schedule.contains(LocalTime {
            weekday: Weekday::Saturday,
            minute_of_day: 3 * 60,
        }));
    }

    #[test]
    fn a_schedule_with_no_days_or_no_length_is_refused() {
        assert_eq!(
            Schedule::new([], 0, 60),
            Err(RuleError::EmptySchedule),
            "a schedule that can never start is a mistake, not an off switch"
        );
        assert_eq!(
            Schedule::new([Weekday::Monday], 600, 600),
            Err(RuleError::InvalidSchedule)
        );
        assert_eq!(
            Schedule::new([Weekday::Monday], 0, MINUTES_PER_DAY),
            Err(RuleError::InvalidSchedule)
        );
    }

    #[test]
    fn an_empty_group_or_rule_is_refused_rather_than_vacuously_true() {
        assert_eq!(
            ConditionGroup::new(Combine::All, []),
            Err(RuleError::EmptyGroup)
        );
        assert_eq!(
            Rule::new(RuleId(1), named("Nothing"), Combine::All, []),
            Err(RuleError::EmptyRule)
        );
    }

    #[test]
    fn every_condition_names_the_provider_that_can_answer_it() {
        let conditions = [
            Condition::ProcessRunning {
                matcher: ProcessMatcher::new(ProcessMatchKind::ExecutableName, "java").unwrap(),
            },
            Condition::AcPower { connected: true },
            Condition::BatteryPercent {
                at_least: 0,
                at_most: 20,
            },
            Condition::ExternalDisplay { connected: true },
            Condition::AudioPlayback { playing: true },
            Condition::CpuUtilizationAtLeast { percent: 50 },
            Condition::NetworkThroughputAtLeast {
                kibibytes_per_second: 500,
            },
            Condition::NetworkInterfaceUp {
                interface: InterfaceName::new("wg0").unwrap(),
            },
            Condition::TimeSchedule {
                schedule: Schedule::new([Weekday::Monday], 0, 60).unwrap(),
            },
            Condition::WatchedPathActive {
                path: WatchedPath::new("/tmp/x").unwrap(),
                within_seconds: 60,
            },
            Condition::Fullscreen { active: true },
        ];
        let mut seen: Vec<ProviderKind> = conditions.iter().map(Condition::provider).collect();
        seen.sort();
        seen.dedup();
        let mut all = ProviderKind::ALL.to_vec();
        all.sort();
        assert_eq!(
            seen, all,
            "every provider must be reachable from a condition"
        );
    }

    #[test]
    fn out_of_range_operands_are_refused() {
        assert_eq!(
            Condition::BatteryPercent {
                at_least: 50,
                at_most: 20
            }
            .validate(),
            Err(RuleError::InvalidBatteryRange)
        );
        assert_eq!(
            Condition::BatteryPercent {
                at_least: 0,
                at_most: 101
            }
            .validate(),
            Err(RuleError::InvalidBatteryRange)
        );
        assert_eq!(
            Condition::CpuUtilizationAtLeast { percent: 101 }.validate(),
            Err(RuleError::InvalidCpuThreshold)
        );
        assert_eq!(
            Condition::WatchedPathActive {
                path: WatchedPath::new("/tmp/x").unwrap(),
                within_seconds: 0
            }
            .validate(),
            Err(RuleError::InvalidWatchWindow)
        );
    }

    #[test]
    fn adding_a_rule_assigns_the_next_identity_whatever_the_caller_asked_for() {
        let mut set = RuleSet::new();
        let mut supplied = rule("Build");
        supplied.id = RuleId(999);
        let id = set.add(supplied).unwrap();
        assert_eq!(id, RuleId(1));
        assert_eq!(set.rule(RuleId(999)), None);
        assert_eq!(set.add(rule("Download")).unwrap(), RuleId(2));
    }

    #[test]
    fn a_duplicate_lands_next_to_its_original_and_starts_disabled() {
        let mut set = RuleSet::new();
        let first = set.add(rule("Build")).unwrap();
        set.add(rule("Download")).unwrap();

        let copy = set.duplicate(first).unwrap();
        assert_eq!(
            set.rules().iter().map(|rule| rule.id).collect::<Vec<_>>(),
            vec![first, copy, RuleId(2)]
        );
        assert!(!set.rule(copy).unwrap().enabled);
        assert_eq!(set.rule(copy).unwrap().name.as_str(), "Build");
    }

    #[test]
    fn reordering_moves_a_rule_to_an_absolute_position() {
        let mut set = RuleSet::new();
        let first = set.add(rule("A")).unwrap();
        let second = set.add(rule("B")).unwrap();
        let third = set.add(rule("C")).unwrap();

        set.reorder(third, 0).unwrap();
        assert_eq!(
            set.rules().iter().map(|rule| rule.id).collect::<Vec<_>>(),
            vec![third, first, second]
        );
        assert_eq!(set.reorder(third, 3), Err(RuleError::InvalidPosition));
    }

    #[test]
    fn editing_a_rule_that_is_not_there_is_refused_and_changes_nothing() {
        let mut set = RuleSet::new();
        set.add(rule("A")).unwrap();
        assert_eq!(
            set.set_enabled(RuleId(7), false),
            Err(RuleError::UnknownRule)
        );
        assert_eq!(
            set.duplicate(RuleId(7)).unwrap_err(),
            RuleError::UnknownRule
        );
        assert_eq!(set.remove(RuleId(7)).unwrap_err(), RuleError::UnknownRule);
        assert_eq!(set.rules().len(), 1);
    }

    #[test]
    fn a_replacement_keeps_the_identity_and_the_position_it_replaced() {
        let mut set = RuleSet::new();
        let first = set.add(rule("A")).unwrap();
        set.add(rule("B")).unwrap();

        let mut edited = rule("A renamed");
        edited.id = RuleId(12345);
        edited.priority = 90;
        set.replace(first, edited).unwrap();

        assert_eq!(set.rules()[0].id, first);
        assert_eq!(set.rules()[0].name.as_str(), "A renamed");
        assert_eq!(set.rules()[0].priority, 90);
    }

    #[test]
    fn a_rule_set_refuses_to_grow_without_bound() {
        let mut set = RuleSet::new();
        for index in 0..MAX_RULES {
            set.add(rule(&format!("Rule {index}"))).unwrap();
        }
        assert_eq!(set.add(rule("One too many")), Err(RuleError::TooManyRules));
        assert_eq!(
            set.duplicate(RuleId(1)).unwrap_err(),
            RuleError::TooManyRules
        );
    }

    #[test]
    fn only_the_two_documented_pause_lengths_are_accepted() {
        let mut set = RuleSet::new();
        assert!(set.pause_for(PAUSE_SHORT_SECONDS, 1_000).is_ok());
        assert!(set.pause_for(PAUSE_LONG_SECONDS, 1_000).is_ok());
        assert_eq!(
            set.pause_for(999_999, 1_000),
            Err(RuleError::InvalidPauseDuration),
            "an arbitrary length would turn Pause into an undocumented off switch"
        );
    }

    #[test]
    fn a_pause_expires_by_itself_and_an_until_resumed_pause_does_not() {
        let mut set = RuleSet::new();
        set.pause_for(PAUSE_SHORT_SECONDS, 1_000).unwrap();
        assert_eq!(
            set.suppression(1_000),
            Some(Suppression::PausedUntil {
                unix_seconds: 1_000 + PAUSE_SHORT_SECONDS
            })
        );
        assert_eq!(set.suppression(1_000 + PAUSE_SHORT_SECONDS), None);
        assert!(set.expire_pause(1_000 + PAUSE_SHORT_SECONDS));
        assert_eq!(set.pause_state(), PauseState::Running);

        set.pause_until_resumed();
        assert_eq!(
            set.suppression(u64::MAX),
            Some(Suppression::PausedUntilResumed)
        );
        assert!(!set.expire_pause(u64::MAX));
        set.resume();
        assert_eq!(set.suppression(u64::MAX), None);
    }

    #[test]
    fn overriding_every_rule_is_refused_without_an_explicit_confirmation() {
        let mut set = RuleSet::new();
        assert_eq!(
            set.override_all(false),
            Err(RuleError::OverrideConfirmationRequired)
        );
        assert!(!set.is_overridden());
        assert_eq!(set.suppression(1_000), None);

        set.override_all(true).unwrap();
        assert!(set.is_overridden());
        assert_eq!(set.suppression(1_000), Some(Suppression::Overridden));

        set.resume();
        assert!(!set.is_overridden());
    }

    #[test]
    fn an_override_outranks_a_pause_in_the_explanation() {
        let mut set = RuleSet::new();
        set.pause_for(PAUSE_SHORT_SECONDS, 1_000).unwrap();
        set.override_all(true).unwrap();
        assert_eq!(set.suppression(1_000), Some(Suppression::Overridden));
    }

    #[test]
    fn only_the_providers_enabled_rules_need_are_required() {
        let mut set = RuleSet::new();
        let id = set.add(rule("On AC")).unwrap();
        assert_eq!(set.required_providers(), vec![ProviderKind::AcPower]);

        set.set_enabled(id, false).unwrap();
        assert!(
            set.required_providers().is_empty(),
            "a disabled rule must not make the service poll anything"
        );
    }

    #[test]
    fn a_rule_set_survives_a_json_round_trip() {
        let mut set = RuleSet::new();
        set.add(rule("Build")).unwrap();
        set.pause_for(PAUSE_LONG_SECONDS, 1_000).unwrap();
        let document = serde_json::to_string(&set).unwrap();
        assert_eq!(serde_json::from_str::<RuleSet>(&document).unwrap(), set);
    }
}

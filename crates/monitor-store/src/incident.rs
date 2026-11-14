//! "The system was just slow."
//!
//! An incident is a slowdown with evidence attached. Since ticket 115 the
//! service recognizes slowdowns by itself, as they happen, with
//! [`SlowdownDetector`]: programs kept waiting for the processor, memory, or
//! the disk, memory nearly gone while the computer swaps, a program ended
//! because memory ran out, a processor throttling itself because it is too
//! hot, or the machine freezing so badly that the service itself is late.
//! The record says which, for how long, how bad, and which processes were
//! using the most of it. Incidents a person marked by hand with an
//! earlier version carry no [`Detection`] and stay readable as they were.
//!
//! The evidence is the sample taken when the slowdown was recognized, the
//! collector states behind it, and how far the recorded numbers had moved
//! from the preceding baseline. The window before and after is stored on the
//! record rather than applied at query time, so an incident still means the
//! same stretch of history a week later, after the default has changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use monitor_core::{CollectorReport, EntityKind, MetricId, MetricSet, Observation};

use crate::sample::{ProcessSample, Sample};

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

/// One slowdown, recorded by the service or, before ticket 115, marked by
/// hand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Incident {
    pub id: u64,
    /// When the slowdown began, or the moment a person marked.
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
    /// What the service saw, on an incident it recorded by itself. `None` on
    /// one a person marked by hand. Optional, so an older Better Monitor still
    /// reads a record that carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection: Option<Detection>,
}

impl Incident {
    /// Whether a person marked this incident by hand, as Better Monitor let
    /// them before it recorded slowdowns by itself.
    pub fn marked_by_hand(&self) -> bool {
        self.detection.is_none()
    }

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

// ---------------------------------------------------------------------------
// Slowdowns the service recognizes by itself (ticket 115).

/// What a recorded slowdown was.
///
/// Stored by name, so a record a newer Better Monitor wrote with a kind this
/// one does not know still opens, as [`SlowdownKind::Other`], instead of
/// making the whole incident unreadable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlowdownKind {
    /// Programs kept waiting for memory: the kernel was reclaiming or swapping
    /// pages in their way (memory pressure stall information).
    MemoryPressure,
    /// Every running program kept waiting for the disk at once (I/O `full`
    /// pressure stall information).
    DiskPressure,
    /// Programs kept waiting for a turn on the processor while far more were
    /// ready to run than there are processors.
    CpuPressure,
    /// Memory nearly ran out and the computer was swapping.
    MemoryExhausted,
    /// The kernel ended a program because memory ran out.
    OutOfMemoryKill,
    /// The processor slowed itself down because it was too hot.
    ThermalThrottling,
    /// The computer froze: Better Monitor's own once-a-second reading came
    /// far too late while the computer was awake.
    Stalled,
    /// A kind a newer Better Monitor recorded.
    #[serde(other)]
    Other,
}

impl SlowdownKind {
    /// Every kind this build detects, in the order a tie is broken by.
    pub const DETECTED: [SlowdownKind; 7] = [
        SlowdownKind::OutOfMemoryKill,
        SlowdownKind::MemoryExhausted,
        SlowdownKind::MemoryPressure,
        SlowdownKind::DiskPressure,
        SlowdownKind::CpuPressure,
        SlowdownKind::ThermalThrottling,
        SlowdownKind::Stalled,
    ];

    /// A stable key for tests, logs, and locale lookup.
    pub fn key(self) -> &'static str {
        match self {
            SlowdownKind::MemoryPressure => "memory-pressure",
            SlowdownKind::DiskPressure => "disk-pressure",
            SlowdownKind::CpuPressure => "cpu-pressure",
            SlowdownKind::MemoryExhausted => "memory-exhausted",
            SlowdownKind::OutOfMemoryKill => "out-of-memory-kill",
            SlowdownKind::ThermalThrottling => "thermal-throttling",
            SlowdownKind::Stalled => "stalled",
            SlowdownKind::Other => "other",
        }
    }

    /// The resource programs were waiting for.
    pub fn resource(self) -> SlowdownResource {
        match self {
            SlowdownKind::CpuPressure | SlowdownKind::ThermalThrottling => {
                SlowdownResource::Processor
            }
            SlowdownKind::MemoryPressure
            | SlowdownKind::MemoryExhausted
            | SlowdownKind::OutOfMemoryKill => SlowdownResource::Memory,
            SlowdownKind::DiskPressure => SlowdownResource::Disk,
            SlowdownKind::Stalled | SlowdownKind::Other => SlowdownResource::Other,
        }
    }

    /// Whether one reading is the whole evidence. The kernel ending a
    /// program, or the computer freezing for half a minute, is never a brief
    /// spike, so neither needs an onset period.
    fn is_immediate(self) -> bool {
        matches!(self, SlowdownKind::OutOfMemoryKill | SlowdownKind::Stalled)
    }

    fn index(self) -> usize {
        SlowdownKind::DETECTED
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(usize::MAX)
    }
}

const KINDS: usize = SlowdownKind::DETECTED.len();

/// What the busiest-process list of a slowdown is ranked by.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlowdownResource {
    Processor,
    Memory,
    Disk,
    /// Not one resource (a freeze); ranked by processor time.
    #[serde(other)]
    Other,
}

/// One condition that held during a slowdown.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlowdownCause {
    pub kind: SlowdownKind,
    /// How long the condition held, in whole seconds.
    pub seconds: u64,
    /// The worst reading while it held. For memory and processor pressure it
    /// is the share of time, in percent, that at least one program was
    /// waiting (the kernel's ten-second `some` average); for the disk it is
    /// the share of time every running program was waiting at once (`full`).
    /// For [`SlowdownKind::MemoryExhausted`] it is the lowest share of memory,
    /// in percent, that was still available. For [`SlowdownKind::Stalled`] it
    /// is the longest freeze, in seconds. The other event kinds leave it at
    /// zero and count instead.
    #[serde(default)]
    pub worst: f64,
    /// For [`SlowdownKind::OutOfMemoryKill`], how many programs the kernel
    /// ended; for [`SlowdownKind::ThermalThrottling`], how many times a core
    /// or a package was throttled; for [`SlowdownKind::Stalled`], how many
    /// freezes. Zero for the other kinds.
    #[serde(default)]
    pub count: u64,
}

/// What the service saw, on an incident it recorded by itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    /// When the first condition began to hold.
    pub started_at_unix_ms: u64,
    /// When the last condition stopped holding, or the latest moment one was
    /// seen while the slowdown is still going on.
    pub ended_at_unix_ms: u64,
    /// True while the slowdown is still going on.
    #[serde(default)]
    pub ongoing: bool,
    /// What held, the one that explains the slowdown best first.
    pub causes: Vec<SlowdownCause>,
    /// What [`Detection::processes`] is ranked by: the resource of the
    /// leading cause, at the worst moment of it.
    pub processes_by: SlowdownResource,
    /// The processes using the most of that resource at that moment, busiest
    /// first. Names and numbers only: no command line and no user.
    #[serde(default)]
    pub processes: Vec<ProcessSample>,
}

impl Detection {
    /// The cause that explains the slowdown best.
    pub fn leading(&self) -> Option<&SlowdownCause> {
        self.causes.first()
    }

    /// How long the slowdown lasted, in whole seconds.
    pub fn duration_seconds(&self) -> u64 {
        self.ended_at_unix_ms
            .saturating_sub(self.started_at_unix_ms)
            / 1_000
    }
}

/// What counts as a slowdown, and how often one may be recorded.
///
/// The defaults are the product's decision (ticket 115) and are explained on
/// the Events page; a test or a recording session changes only the timing.
/// Every threshold is entered at its value and left only below half of it,
/// so a reading that hovers at the line is one slowdown and not a series.
///
/// Pressure stall information has two lines per resource. `some` is the share
/// of time at least one program was waiting; `full` is the share of time
/// every program that had work to do was waiting at once, which is what a
/// person at the desktop feels, because their own programs are among them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlowdownRules {
    /// How long a condition has to hold before it is a slowdown. A brief
    /// spike is shorter than this and is never recorded.
    pub onset_seconds: u64,
    /// The same for waiting for the processor, which a long build does for
    /// minutes without anyone noticing, so it has to last longer.
    pub cpu_onset_seconds: u64,
    /// How long every condition has to stay clear before the slowdown is
    /// over. Anything that comes back sooner extends the same record.
    pub quiet_seconds: u64,
    /// How often the record of a slowdown still going on is brought up to
    /// date on disk.
    pub checkpoint_seconds: u64,
    /// The most slowdowns recorded in any 24 hours.
    pub max_per_day: usize,
    /// CPU `some` pressure, percent of the last ten seconds.
    pub cpu_some_percent: f64,
    /// The one-minute load average per logical processor that has to come
    /// with it: a build that keeps every core busy has about one runnable
    /// task per core, a machine that is overwhelmed has several.
    pub cpu_load_per_processor: f64,
    /// Memory `some` and `full` pressure.
    pub memory_some_percent: f64,
    pub memory_full_percent: f64,
    /// I/O `full` pressure. `some` alone is what one large copy to a slow
    /// stick holds at 80-90% while the rest of the desktop is fine, so it is
    /// not used.
    pub disk_full_percent: f64,
    /// Memory still available, percent of the total, at or below which the
    /// computer is nearly out of memory.
    pub memory_available_percent: f64,
    /// Pages swapped in and out per second that make "nearly out" a
    /// slowdown rather than a full cache.
    pub swap_pages_per_second: f64,
    /// How late, in seconds, Better Monitor's own reading has to be, with
    /// the computer awake, to count as the computer freezing.
    pub stall_seconds: u64,
}

/// The share of an entry threshold a condition has to fall below to stop
/// holding.
pub const SLOWDOWN_EXIT_FRACTION: f64 = 0.5;

/// How much further the wall clock may move than the monotonic clock between
/// two samples before the difference is a suspend. The monotonic clock stops
/// while the machine sleeps; it does not stop while the machine is stuck, so
/// a gap on both clocks is a freeze and a gap on the wall clock alone is
/// sleep (or the clock being set).
pub const SUSPEND_SLACK_MS: u64 = 10_000;

/// How long one throttle-counter increase keeps thermal throttling holding.
/// The counters go up each time a core or package is held back, not once a
/// second, so a run of throttling reads as a run of separate events.
pub const THROTTLE_HOLD_MS: u64 = 10_000;

/// How many processes a detected slowdown keeps.
pub const SLOWDOWN_PROCESSES: usize = 5;

/// One day, for the daily cap.
const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

impl Default for SlowdownRules {
    fn default() -> Self {
        Self {
            onset_seconds: 10,
            cpu_onset_seconds: 30,
            quiet_seconds: 60,
            checkpoint_seconds: 300,
            max_per_day: 24,
            cpu_some_percent: 80.0,
            cpu_load_per_processor: 2.0,
            // Memory stalls are felt at once (the Overview's own line).
            memory_some_percent: 20.0,
            memory_full_percent: 10.0,
            disk_full_percent: 20.0,
            memory_available_percent: 5.0,
            swap_pages_per_second: 100.0,
            stall_seconds: 30,
        }
    }
}

impl SlowdownRules {
    fn onset_ms(&self, kind: SlowdownKind) -> u64 {
        if kind.is_immediate() {
            0
        } else if kind == SlowdownKind::CpuPressure {
            self.cpu_onset_seconds * 1_000
        } else {
            self.onset_seconds * 1_000
        }
    }
}

/// What happened to the slowdown the detector is following.
#[derive(Clone, Debug, PartialEq)]
pub enum SlowdownEvent {
    /// A slowdown began: record it.
    Began(Detection),
    /// The slowdown is still going on and its record is due to be brought up
    /// to date.
    Continued(Detection),
    /// The slowdown is over: write its final record.
    Ended(Detection),
    /// A slowdown began and the daily cap kept it from being recorded.
    Skipped { at_unix_ms: u64 },
}

/// Which round [`SlowdownDetector::observe`] asks the processes of.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Round {
    /// The round the sample was made from.
    Current,
    /// The round before it: a program the kernel ended for memory is gone
    /// from the round that reports the kill, so the list comes from before.
    Previous,
}

/// How far one reading is past a kind's entry threshold: one or more is
/// entering, [`SLOWDOWN_EXIT_FRACTION`] or more is holding.
#[derive(Clone, Copy, Debug, Default)]
struct Reading {
    severity: f64,
    worst: f64,
    count: u64,
}

/// A moment on both clocks: `clock` is the detector's own monotonic
/// milliseconds, which every duration is measured on; `wall` is what a
/// person reads and what a record says.
#[derive(Clone, Copy, Debug, Default)]
struct Moment {
    clock: u64,
    wall: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct KindState {
    /// When the kind most recently began entering without a break.
    entering_since: Option<Moment>,
    /// For thermal throttling, until when (clock) the last increase holds.
    throttled_until: u64,
    /// The worst reading and the events counted since `entering_since`, so
    /// the onset period is part of the record once it qualifies.
    pending_worst: Option<f64>,
    pending_count: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    joined: bool,
    held_ms: u64,
    worst: Option<f64>,
    count: u64,
    /// Whether the kind ever held for the onset period in this slowdown. A
    /// kind that only spiked inside someone else's slowdown is not a cause.
    qualified: bool,
}

#[derive(Clone, Debug)]
struct OpenSlowdown {
    started_at_unix_ms: u64,
    last_held: Moment,
    last_checkpoint_clock: u64,
    tallies: [Tally; KINDS],
    processes_by: SlowdownResource,
    processes: Vec<ProcessSample>,
    /// The severity the process list was taken at.
    processes_severity: f64,
    /// False when the daily cap was reached: the slowdown is followed, so it
    /// is not counted again every second, but nothing is written.
    recorded: bool,
}

impl OpenSlowdown {
    fn cause(&self, index: usize) -> Option<SlowdownCause> {
        let tally = &self.tallies[index];
        tally.qualified.then(|| SlowdownCause {
            kind: SlowdownKind::DETECTED[index],
            seconds: tally.held_ms / 1_000,
            worst: tally.worst.unwrap_or(0.0),
            count: tally.count,
        })
    }

    fn detection(&self, ongoing: bool) -> Detection {
        let mut causes: Vec<SlowdownCause> =
            (0..KINDS).filter_map(|index| self.cause(index)).collect();
        causes.sort_by(cause_order);
        Detection {
            started_at_unix_ms: self.started_at_unix_ms,
            ended_at_unix_ms: self.last_held.wall,
            ongoing,
            causes,
            processes_by: self.processes_by,
            processes: self.processes.clone(),
        }
    }

    /// The leading cause's kind, worked out without building a record.
    fn leading(&self) -> Option<SlowdownKind> {
        (0..KINDS)
            .filter_map(|index| self.cause(index))
            .min_by(cause_order)
            .map(|cause| cause.kind)
    }
}

/// The program being ended explains everything else, so it leads; then
/// whatever held longest; then the fixed order of [`SlowdownKind::DETECTED`].
fn cause_order(left: &SlowdownCause, right: &SlowdownCause) -> std::cmp::Ordering {
    let oom = |cause: &SlowdownCause| cause.kind == SlowdownKind::OutOfMemoryKill;
    oom(right)
        .cmp(&oom(left))
        .then(right.seconds.cmp(&left.seconds))
        .then(left.kind.index().cmp(&right.kind.index()))
}

/// Recognizes slowdowns in a stream of samples.
///
/// It reads only what each sample already carries, so it costs a few
/// comparisons per round and allocates nothing until something happens. It
/// holds one state per kind and at most one open slowdown, and its memory of
/// the daily cap is the start times of the last day's records.
#[derive(Clone, Debug)]
pub struct SlowdownDetector {
    rules: SlowdownRules,
    kinds: [KindState; KINDS],
    open: Option<OpenSlowdown>,
    /// The previous sample's wall time and monotonic nanoseconds.
    previous: Option<(u64, u64)>,
    clock_ms: u64,
    /// Start times of the slowdowns recorded in the last day.
    recent_starts: Vec<u64>,
    /// Slowdowns the daily cap kept from being recorded since this detector
    /// started.
    skipped: u64,
}

impl SlowdownDetector {
    pub fn new(rules: SlowdownRules) -> Self {
        Self {
            rules,
            kinds: [KindState::default(); KINDS],
            open: None,
            previous: None,
            clock_ms: 0,
            recent_starts: Vec::new(),
            skipped: 0,
        }
    }

    /// Count the slowdowns already recorded, so a restart does not reset the
    /// daily cap.
    pub fn remember(&mut self, incidents: &[Incident]) {
        self.recent_starts = incidents
            .iter()
            .filter_map(|incident| incident.detection.as_ref())
            .map(|detection| detection.started_at_unix_ms)
            .collect();
    }

    pub fn rules(&self) -> SlowdownRules {
        self.rules
    }

    /// Slowdowns the daily cap kept from being recorded since this detector
    /// started.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Whether a slowdown is being followed right now.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The recorded slowdown going on right now, as it stands this second.
    pub fn current(&self) -> Option<Detection> {
        self.open
            .as_ref()
            .filter(|open| open.recorded)
            .map(|open| open.detection(true))
    }

    /// Take one sample.
    ///
    /// `busiest` ranks the processes of a round by one resource; it is asked
    /// only when a slowdown begins or gets worse.
    pub fn observe(
        &mut self,
        sample: &Sample,
        busiest: &mut dyn FnMut(SlowdownResource, Round) -> Vec<ProcessSample>,
    ) -> Vec<SlowdownEvent> {
        let wall = sample.wall_unix_ms;
        let mut events = Vec::new();
        let before = Moment {
            clock: self.clock_ms,
            wall: self.previous.map(|(wall, _)| wall).unwrap_or(wall),
        };
        // How long passed on each clock. A monotonic clock that went
        // backwards is another process's; nothing is known about the gap.
        let (elapsed_ms, slept) = match self.previous {
            None => (0, false),
            Some((previous_wall, previous_monotonic)) => {
                match sample.monotonic_ns.checked_sub(previous_monotonic) {
                    None => (0, true),
                    Some(nanoseconds) => {
                        let monotonic_ms = nanoseconds / 1_000_000;
                        let wall_ms = wall.saturating_sub(previous_wall);
                        (
                            monotonic_ms,
                            wall_ms > monotonic_ms + SUSPEND_SLACK_MS || wall < previous_wall,
                        )
                    }
                }
            }
        };
        self.previous = Some((wall, sample.monotonic_ns));
        if slept {
            // Asleep, or a clock set by hand: nothing held across it, and a
            // slowdown ends where watching stopped.
            self.kinds = [KindState::default(); KINDS];
            if let Some(open) = self.open.take()
                && open.recorded
            {
                events.push(SlowdownEvent::Ended(open.detection(false)));
            }
        }
        let held_ms = if slept { 0 } else { elapsed_ms };
        self.clock_ms += held_ms;
        let now = Moment {
            clock: self.clock_ms,
            wall,
        };
        let stalled_ms = (!slept && held_ms >= self.rules.stall_seconds * 1_000).then_some(held_ms);

        let mut readings = [Reading::default(); KINDS];
        for (index, kind) in SlowdownKind::DETECTED.into_iter().enumerate() {
            let state = &mut self.kinds[index];
            let reading = match kind {
                // The freeze began when the last reading before it was taken.
                SlowdownKind::Stalled => match stalled_ms {
                    Some(stalled) => {
                        state.entering_since = Some(before);
                        Reading {
                            severity: 1.0,
                            worst: stalled as f64 / 1_000.0,
                            count: 1,
                        }
                    }
                    None => Reading::default(),
                },
                _ => read(kind, sample, &self.rules, held_ms, now.clock, state),
            };
            if reading.severity >= 1.0 {
                state.entering_since.get_or_insert(now);
                state.pending_worst = Some(worse(kind, state.pending_worst, reading.worst));
                state.pending_count += reading.count;
            } else {
                state.entering_since = None;
                state.pending_worst = None;
                state.pending_count = 0;
            }
            readings[index] = reading;
        }

        let rules = self.rules;
        let qualified = |state: &KindState, kind: SlowdownKind| {
            state
                .entering_since
                .is_some_and(|since| now.clock.saturating_sub(since.clock) >= rules.onset_ms(kind))
        };

        if let Some(open) = self.open.as_mut() {
            let mut held_now = false;
            for (index, reading) in readings.iter().enumerate() {
                let kind = SlowdownKind::DETECTED[index];
                let entering = reading.severity >= 1.0;
                let holding = reading.severity >= SLOWDOWN_EXIT_FRACTION;
                let tally = &mut open.tallies[index];
                if !(holding && tally.joined || entering) {
                    continue;
                }
                tally.joined = true;
                // A freeze in the middle of a slowdown is the slowdown going
                // on: the time held includes it.
                tally.held_ms += if kind == SlowdownKind::Stalled {
                    stalled_ms.unwrap_or(0)
                } else {
                    held_ms
                };
                tally.count += reading.count;
                tally.worst = Some(worse(kind, tally.worst, reading.worst));
                if qualified(&self.kinds[index], kind) {
                    tally.qualified = true;
                }
                if tally.qualified {
                    open.last_held = now;
                    held_now = true;
                }
            }
            if let Some(leading) = open.leading() {
                if leading == SlowdownKind::OutOfMemoryKill {
                    // The list from before the kill names the program that
                    // was using the memory; a later one cannot.
                    let killed_now = readings[leading.index()].count > 0;
                    let have_memory_list =
                        open.processes_by == SlowdownResource::Memory && !open.processes.is_empty();
                    if killed_now && !have_memory_list {
                        open.processes = busiest(SlowdownResource::Memory, Round::Previous);
                        open.processes_by = SlowdownResource::Memory;
                    }
                    open.processes_severity = f64::INFINITY;
                } else {
                    let severity = readings[leading.index()].severity;
                    if leading.resource() != open.processes_by || severity > open.processes_severity
                    {
                        let ranked = busiest(leading.resource(), Round::Current);
                        if !ranked.is_empty() || leading.resource() != open.processes_by {
                            open.processes = ranked;
                            open.processes_by = leading.resource();
                            open.processes_severity = severity;
                        }
                    }
                }
            }
            if !held_now
                && now.clock.saturating_sub(open.last_held.clock) >= rules.quiet_seconds * 1_000
            {
                let open = self.open.take().expect("an open slowdown");
                if open.recorded {
                    events.push(SlowdownEvent::Ended(open.detection(false)));
                }
            } else if open.recorded
                && now.clock.saturating_sub(open.last_checkpoint_clock)
                    >= rules.checkpoint_seconds * 1_000
            {
                open.last_checkpoint_clock = now.clock;
                events.push(SlowdownEvent::Continued(open.detection(true)));
            }
            return events;
        }

        let mut tallies = [Tally::default(); KINDS];
        let mut started_at: Option<u64> = None;
        for (index, kind) in SlowdownKind::DETECTED.into_iter().enumerate() {
            let state = &self.kinds[index];
            if !qualified(state, kind) {
                continue;
            }
            let since = state.entering_since.unwrap_or(now);
            started_at = Some(started_at.map_or(since.wall, |at| at.min(since.wall)));
            tallies[index] = Tally {
                joined: true,
                held_ms: now.clock.saturating_sub(since.clock),
                worst: state.pending_worst,
                count: state.pending_count,
                qualified: true,
            };
        }
        let Some(started_at) = started_at else {
            return events;
        };
        self.recent_starts
            .retain(|start| wall.saturating_sub(*start) < DAY_MS);
        let recorded = self.recent_starts.len() < rules.max_per_day;
        let mut open = OpenSlowdown {
            started_at_unix_ms: started_at,
            last_held: now,
            last_checkpoint_clock: now.clock,
            tallies,
            processes_by: SlowdownResource::Other,
            processes: Vec::new(),
            processes_severity: 0.0,
            recorded,
        };
        if recorded {
            let leading = open.leading().expect("a slowdown has a cause");
            open.processes_by = leading.resource();
            if leading == SlowdownKind::OutOfMemoryKill {
                open.processes = busiest(SlowdownResource::Memory, Round::Previous);
                open.processes_severity = f64::INFINITY;
            } else {
                open.processes = busiest(leading.resource(), Round::Current);
                open.processes_severity = readings[leading.index()].severity;
            }
            self.recent_starts.push(started_at);
            events.push(SlowdownEvent::Began(open.detection(true)));
        } else {
            self.skipped += 1;
            events.push(SlowdownEvent::Skipped {
                at_unix_ms: started_at,
            });
        }
        self.open = Some(open);
        events
    }

    /// The service is stopping: the slowdown it was following, if any, ends
    /// where it was last seen.
    pub fn interrupt(&mut self) -> Option<Detection> {
        let open = self.open.take()?;
        self.kinds = [KindState::default(); KINDS];
        open.recorded.then(|| open.detection(false))
    }
}

/// Keep the worse of two readings: the higher pressure or the longer freeze,
/// the lower available memory.
fn worse(kind: SlowdownKind, held: Option<f64>, reading: f64) -> f64 {
    match (kind, held) {
        (_, None) => reading,
        (SlowdownKind::MemoryExhausted, Some(held)) => held.min(reading),
        (_, Some(held)) => held.max(reading),
    }
}

/// The metric identifiers the detector reads, made once rather than every
/// second.
struct Ids {
    some: MetricId,
    full: MetricId,
    load: MetricId,
    processors: MetricId,
    available: MetricId,
    total: MetricId,
    swap_in: MetricId,
    swap_out: MetricId,
    oom_kills: MetricId,
    throttle: MetricId,
    process_cpu: MetricId,
    process_memory: MetricId,
    process_read: MetricId,
    process_write: MetricId,
}

fn ids() -> &'static Ids {
    static IDS: std::sync::OnceLock<Ids> = std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        let id = |raw: &str| MetricId::new(raw).expect("a slowdown metric id is well formed");
        Ids {
            some: id("pressure.some.avg10"),
            full: id("pressure.full.avg10"),
            load: id("cpu.load.average.1m"),
            processors: id("cpu.logical.count"),
            available: id("memory.available"),
            total: id("memory.total"),
            swap_in: id("memory.swap_in.rate"),
            swap_out: id("memory.swap_out.rate"),
            oom_kills: id("memory.oom.kill.rate"),
            throttle: id("cpu.thermal_throttle.rate"),
            process_cpu: id("process.cpu.utilization"),
            process_memory: id("process.memory.resident"),
            process_read: id("process.io.read.bytes.rate"),
            process_write: id("process.io.write.bytes.rate"),
        }
    })
}

fn pressure(sample: &Sample, resource: &str, id: &MetricId) -> Option<f64> {
    sample
        .entities
        .iter()
        .find(|entity| entity.kind == EntityKind::PressureResource && entity.key == resource)
        .and_then(|entity| entity.metrics.get(id))
        .and_then(Observation::as_f64)
}

/// One kind's reading in one sample. A metric this machine cannot report
/// reads as nothing, never as a slowdown and never as proof of none.
fn read(
    kind: SlowdownKind,
    sample: &Sample,
    rules: &SlowdownRules,
    elapsed_ms: u64,
    now_clock: u64,
    state: &mut KindState,
) -> Reading {
    let ids = ids();
    let seconds = elapsed_ms as f64 / 1_000.0;
    let over = |value: Option<f64>, threshold: f64| match value {
        Some(value) if threshold > 0.0 => value / threshold,
        _ => 0.0,
    };
    match kind {
        SlowdownKind::MemoryPressure => {
            let some = pressure(sample, "memory", &ids.some);
            let full = pressure(sample, "memory", &ids.full);
            Reading {
                severity: over(some, rules.memory_some_percent)
                    .max(over(full, rules.memory_full_percent)),
                worst: some.unwrap_or(0.0),
                count: 0,
            }
        }
        SlowdownKind::DiskPressure => {
            let full = pressure(sample, "io", &ids.full);
            Reading {
                severity: over(full, rules.disk_full_percent),
                worst: full.unwrap_or(0.0),
                count: 0,
            }
        }
        SlowdownKind::CpuPressure => {
            // `full` is undefined for the processor at system level, so the
            // second sign is the run queue: far more tasks ready than there
            // are processors to run them.
            let some = pressure(sample, "cpu", &ids.some);
            let load_per_processor =
                match (sample.value_of(&ids.load), sample.value_of(&ids.processors)) {
                    (Some(load), Some(processors)) if processors > 0.0 => Some(load / processors),
                    _ => None,
                };
            Reading {
                severity: over(some, rules.cpu_some_percent)
                    .min(over(load_per_processor, rules.cpu_load_per_processor)),
                worst: some.unwrap_or(0.0),
                count: 0,
            }
        }
        SlowdownKind::MemoryExhausted => {
            let available = sample.value_of(&ids.available);
            let total = sample.value_of(&ids.total);
            let swap_in = sample.value_of(&ids.swap_in);
            let swap_out = sample.value_of(&ids.swap_out);
            let (Some(available), Some(total)) = (available, total) else {
                return Reading::default();
            };
            if total <= 0.0 || (swap_in.is_none() && swap_out.is_none()) {
                return Reading::default();
            }
            let percent = available / total * 100.0;
            let swapping = swap_in.unwrap_or(0.0) + swap_out.unwrap_or(0.0);
            let scarce = rules.memory_available_percent / percent.max(0.01);
            Reading {
                severity: scarce.min(over(Some(swapping), rules.swap_pages_per_second)),
                worst: percent,
                count: 0,
            }
        }
        SlowdownKind::OutOfMemoryKill => {
            let rate = sample.value_of(&ids.oom_kills).unwrap_or(0.0);
            if rate > 0.0 {
                Reading {
                    severity: f64::INFINITY,
                    worst: 0.0,
                    count: ((rate * seconds).round() as u64).max(1),
                }
            } else {
                Reading::default()
            }
        }
        SlowdownKind::ThermalThrottling => {
            let rate = sample.value_of(&ids.throttle).unwrap_or(0.0);
            let count = if rate > 0.0 {
                state.throttled_until = now_clock + THROTTLE_HOLD_MS;
                ((rate * seconds).round() as u64).max(1)
            } else {
                0
            };
            Reading {
                severity: if now_clock < state.throttled_until || count > 0 {
                    1.0
                } else {
                    0.0
                },
                worst: 0.0,
                count,
            }
        }
        SlowdownKind::Stalled | SlowdownKind::Other => Reading::default(),
    }
}

/// The processes of one round using the most of `resource`, busiest first,
/// with names and numbers only. Only the few that are kept are turned into
/// records.
pub fn busiest_processes(
    reports: &[CollectorReport],
    resource: SlowdownResource,
    limit: usize,
) -> Vec<ProcessSample> {
    let ids = ids();
    let number = |metrics: &MetricSet, id: &MetricId| metrics.get(id).and_then(Observation::as_f64);
    let disk = |metrics: &MetricSet| match (
        number(metrics, &ids.process_read),
        number(metrics, &ids.process_write),
    ) {
        (None, None) => None,
        (read, write) => Some(read.unwrap_or(0.0) + write.unwrap_or(0.0)),
    };
    let mut ranked: Vec<(f64, &monitor_core::Entity)> = reports
        .iter()
        .flat_map(|report| report.entities.iter())
        .filter(|entity| entity.id.kind == EntityKind::Process)
        .filter_map(|entity| {
            let score = match resource {
                SlowdownResource::Memory => number(&entity.metrics, &ids.process_memory),
                SlowdownResource::Disk => disk(&entity.metrics),
                SlowdownResource::Processor | SlowdownResource::Other => {
                    number(&entity.metrics, &ids.process_cpu)
                }
            }?;
            (score > 0.0).then_some((score, entity))
        })
        .collect();
    let by_score = |left: &(f64, &monitor_core::Entity), right: &(f64, &monitor_core::Entity)| {
        right
            .0
            .total_cmp(&left.0)
            .then(left.1.id.key.cmp(&right.1.id.key))
    };
    if ranked.len() > limit && limit > 0 {
        ranked.select_nth_unstable_by(limit - 1, by_score);
        ranked.truncate(limit);
    }
    ranked.sort_by(by_score);
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, entity)| {
            let mut process = crate::sample::process_sample(&entity.id.key, &entity.metrics);
            process.command_line = None;
            process.user = None;
            process.disk_bytes_rate = disk(&entity.metrics);
            process
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{CollectorId, Entity, EntityId, Timestamp, UnsupportedReason};

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
            detection: None,
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

    // Detection, on synthetic sample streams.

    /// One second of a machine: pressure per resource as `(some, full)`,
    /// and the system-level readings given.
    #[derive(Clone, Default)]
    struct Moment {
        cpu: f64,
        memory: (f64, f64),
        io: (f64, f64),
        system: Vec<(&'static str, f64)>,
        /// Processes left out of this round, by name: a program the kernel
        /// just ended is not in the round that reports the kill.
        gone: Vec<&'static str>,
    }

    /// One round of reports for `moment`: pressure, system readings, and
    /// three processes that each use the most of one resource.
    fn moment_reports(at_ms: u64, monotonic_ms: u64, moment: &Moment) -> Vec<CollectorReport> {
        let at = Timestamp {
            unix_ms: at_ms,
            monotonic_ns: monotonic_ms * 1_000_000,
        };
        let mut pressure = CollectorReport::new(CollectorId::new("linux.pressure").unwrap(), at);
        for (resource, some, full) in [
            ("cpu", moment.cpu, 0.0),
            ("memory", moment.memory.0, moment.memory.1),
            ("io", moment.io.0, moment.io.1),
        ] {
            let mut metrics = MetricSet::new();
            metrics.insert(id("pressure.some.avg10"), Observation::float(some));
            metrics.insert(id("pressure.full.avg10"), Observation::float(full));
            pressure.entities.push(Entity::new(
                EntityId::new(EntityKind::PressureResource, resource),
                metrics,
            ));
        }
        let mut system = CollectorReport::new(CollectorId::new("linux.memory").unwrap(), at);
        for (name, value) in &moment.system {
            system.metrics.insert(id(name), Observation::float(*value));
        }
        let mut processes = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at);
        for (pid, name, cpu, memory, disk) in [
            (10, "compiler", 3.5, 200.0e6, 0.0),
            (20, "browser", 0.4, 3.0e9, 0.0),
            (30, "indexer", 0.1, 50.0e6, 9.0e7),
        ] {
            if moment.gone.contains(&name) {
                continue;
            }
            let mut metrics = MetricSet::new();
            metrics.insert(id("process.name"), Observation::text(name));
            metrics.insert(id("process.cpu.utilization"), Observation::float(cpu));
            metrics.insert(
                id("process.memory.resident"),
                Observation::unsigned(memory as u64),
            );
            metrics.insert(id("process.io.read.bytes.rate"), Observation::float(disk));
            metrics.insert(
                id("process.command_line"),
                Observation::text(format!("{name} --token=secret")),
            );
            processes.entities.push(Entity::new(
                EntityId::new(EntityKind::Process, pid.to_string()),
                metrics,
            ));
        }
        vec![pressure, system, processes]
    }

    fn calm() -> Moment {
        Moment::default()
    }

    fn memory_stall() -> Moment {
        Moment {
            memory: (45.0, 12.0),
            ..Moment::default()
        }
    }

    /// A detector and the clocks it is fed from. The monotonic clock runs
    /// with the wall clock except across [`Machine::sleep`], as on a
    /// computer that was suspended.
    struct Machine {
        d: SlowdownDetector,
        slept_ms: u64,
        previous: Vec<CollectorReport>,
    }

    impl Machine {
        fn new(rules: SlowdownRules) -> Self {
            Self {
                d: SlowdownDetector::new(rules),
                slept_ms: 0,
                previous: Vec::new(),
            }
        }

        /// The next samples' wall clock will be `seconds` ahead of their
        /// monotonic clock.
        fn sleep(&mut self, seconds: u64) {
            self.slept_ms += seconds * 1_000;
        }

        fn take(&mut self, at_ms: u64, moment: &Moment) -> Vec<SlowdownEvent> {
            let reports = moment_reports(at_ms, at_ms - self.slept_ms, moment);
            let sample = Sample::from_reports(&reports, 10);
            let previous = std::mem::take(&mut self.previous);
            let events = self.d.observe(&sample, &mut |resource, round| {
                let from = match round {
                    Round::Current => &reports,
                    Round::Previous => &previous,
                };
                busiest_processes(from, resource, SLOWDOWN_PROCESSES)
            });
            self.previous = reports;
            events
        }
    }

    /// Feed `seconds` one-second samples of `moment` from `start`, and
    /// return every event with the second it came at.
    fn feed(
        machine: &mut Machine,
        start: u64,
        seconds: u64,
        moment: &Moment,
    ) -> Vec<(u64, SlowdownEvent)> {
        feed_every(machine, start, seconds, 1, moment)
    }

    /// The same, one sample every `step` seconds.
    fn feed_every(
        machine: &mut Machine,
        start: u64,
        seconds: u64,
        step: u64,
        moment: &Moment,
    ) -> Vec<(u64, SlowdownEvent)> {
        let mut events = Vec::new();
        for second in (start..start + seconds).step_by(step as usize) {
            for event in machine.take(second * 1_000, moment) {
                events.push((second, event));
            }
        }
        events
    }

    fn began(events: &[(u64, SlowdownEvent)]) -> Vec<(u64, Detection)> {
        events
            .iter()
            .filter_map(|(second, event)| match event {
                SlowdownEvent::Began(detection) => Some((*second, detection.clone())),
                _ => None,
            })
            .collect()
    }

    fn ended(events: &[(u64, SlowdownEvent)]) -> Vec<(u64, Detection)> {
        events
            .iter()
            .filter_map(|(second, event)| match event {
                SlowdownEvent::Ended(detection) => Some((*second, detection.clone())),
                _ => None,
            })
            .collect()
    }

    fn detector() -> Machine {
        Machine::new(SlowdownRules::default())
    }

    /// A build that keeps every core busy: processor `some` pressure high,
    /// one runnable task per processor.
    fn all_core_build() -> Moment {
        Moment {
            cpu: 92.0,
            system: vec![("cpu.load.average.1m", 25.0), ("cpu.logical.count", 24.0)],
            ..Moment::default()
        }
    }

    /// The processor overwhelmed: the same pressure with five times as many
    /// tasks ready as there are processors.
    fn overwhelmed_processor() -> Moment {
        Moment {
            cpu: 92.0,
            system: vec![("cpu.load.average.1m", 120.0), ("cpu.logical.count", 24.0)],
            ..Moment::default()
        }
    }

    #[test]
    fn sustained_pressure_makes_one_incident() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 30, &calm());
        events.extend(feed(&mut detector, 1_030, 20, &memory_stall()));
        events.extend(feed(&mut detector, 1_050, 120, &calm()));

        let began = began(&events);
        assert_eq!(began.len(), 1, "{events:?}");
        let (second, detection) = &began[0];
        // Recognized once it had held for the onset period, and dated from
        // when it began rather than from when it was recognized.
        assert_eq!(*second, 1_040);
        assert_eq!(detection.started_at_unix_ms, 1_030_000);
        assert!(detection.ongoing);
        assert_eq!(
            detection.leading().unwrap().kind,
            SlowdownKind::MemoryPressure
        );

        let ended = ended(&events);
        assert_eq!(ended.len(), 1);
        let (second, detection) = &ended[0];
        assert!(!detection.ongoing);
        // It ends after the quiet period, at the last moment it held.
        assert_eq!(detection.ended_at_unix_ms, 1_049_000);
        assert_eq!(*second, 1_049 + SlowdownRules::default().quiet_seconds);
        assert_eq!(detection.duration_seconds(), 19);
        let cause = detection.leading().unwrap();
        assert_eq!(cause.seconds, 19);
        assert!((cause.worst - 45.0).abs() < 1e-9);
        assert!(!detector.d.is_open());
    }

    #[test]
    fn a_spike_makes_no_incident() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 30, &calm());
        events.extend(feed(&mut detector, 1_030, 9, &memory_stall()));
        events.extend(feed(&mut detector, 1_039, 120, &calm()));
        // A second spike, under the onset period too, is still no slowdown.
        events.extend(feed(&mut detector, 1_159, 5, &memory_stall()));
        events.extend(feed(&mut detector, 1_164, 5, &calm()));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_long_slowdown_is_one_incident_extended_while_it_lasts() {
        let mut detector = detector();
        let mut events = Vec::new();
        // Twenty minutes, dipping under the entry line but never under the
        // exit line, and with clear stretches shorter than the quiet period.
        let wavering = Moment {
            memory: (12.0, 0.0),
            ..Moment::default()
        };
        let mut second = 1_000;
        for _ in 0..20 {
            events.extend(feed(&mut detector, second, 30, &memory_stall()));
            events.extend(feed(&mut detector, second + 30, 15, &wavering));
            events.extend(feed(&mut detector, second + 45, 15, &calm()));
            second += 60;
        }
        events.extend(feed(&mut detector, second, 120, &calm()));

        assert_eq!(began(&events).len(), 1, "{events:?}");
        let ended = ended(&events);
        assert_eq!(ended.len(), 1);
        let detection = &ended[0].1;
        assert_eq!(detection.started_at_unix_ms, 1_000_000);
        assert!(detection.duration_seconds() >= 19 * 60);
        // The record of a slowdown still going on is rewritten now and then,
        // not every second.
        let continued = events
            .iter()
            .filter(|(_, event)| matches!(event, SlowdownEvent::Continued(_)))
            .count();
        // Recognized at 1 010 s and over at about 2 245 s: rewritten at
        // 1 310, 1 610, 1 910, and 2 210.
        assert_eq!(continued, 4, "every five minutes while it lasted");
    }

    #[test]
    fn two_slowdowns_separated_by_quiet_make_two() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 20, &memory_stall());
        events.extend(feed(&mut detector, 1_020, 90, &calm()));
        events.extend(feed(&mut detector, 1_110, 20, &memory_stall()));
        events.extend(feed(&mut detector, 1_130, 90, &calm()));
        let began = began(&events);
        assert_eq!(began.len(), 2, "{events:?}");
        assert_eq!(ended(&events).len(), 2);
        assert_eq!(began[1].1.started_at_unix_ms, 1_110_000);
    }

    #[test]
    fn a_slowdown_that_returns_inside_the_quiet_period_extends_the_first() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 20, &memory_stall());
        events.extend(feed(&mut detector, 1_020, 40, &calm()));
        events.extend(feed(&mut detector, 1_060, 20, &memory_stall()));
        events.extend(feed(&mut detector, 1_080, 90, &calm()));
        assert_eq!(began(&events).len(), 1, "{events:?}");
        let ended = ended(&events);
        assert_eq!(ended[0].1.ended_at_unix_ms, 1_079_000);
    }

    #[test]
    fn the_daily_cap_holds_and_survives_a_restart() {
        let rules = SlowdownRules {
            max_per_day: 3,
            ..SlowdownRules::default()
        };
        let mut detector = Machine::new(rules);
        let mut events = Vec::new();
        let mut second = 1_000;
        for _ in 0..5 {
            events.extend(feed(&mut detector, second, 20, &memory_stall()));
            events.extend(feed(&mut detector, second + 20, 90, &calm()));
            second += 110;
        }
        assert_eq!(began(&events).len(), 3);
        assert_eq!(ended(&events).len(), 3, "an unrecorded one writes nothing");
        assert_eq!(detector.d.skipped(), 2);
        let skipped: Vec<u64> = events
            .iter()
            .filter_map(|(_, event)| match event {
                SlowdownEvent::Skipped { at_unix_ms } => Some(*at_unix_ms),
                _ => None,
            })
            .collect();
        assert_eq!(skipped, vec![1_330_000, 1_440_000], "each one is reported");

        // A restarted service counts what the store already holds.
        let recorded: Vec<Incident> = began(&events)
            .into_iter()
            .enumerate()
            .map(|(index, (_, detection))| {
                let mut recorded = incident();
                recorded.id = index as u64 + 1;
                recorded.detection = Some(detection);
                recorded
            })
            .collect();
        let mut restarted = Machine::new(rules);
        restarted.d.remember(&recorded);
        let more = feed(&mut restarted, second, 20, &memory_stall());
        assert!(began(&more).is_empty(), "{more:?}");

        // A day later (the computer asleep in between) there is room again.
        let next_day = 1_000 + 24 * 60 * 60;
        restarted.sleep(next_day - second - 20);
        let mut events = feed(&mut restarted, next_day, 200, &calm());
        events.extend(feed(&mut restarted, next_day + 200, 20, &memory_stall()));
        assert_eq!(began(&events).len(), 1, "{events:?}");
    }

    fn kind_of(moment: Moment) -> (SlowdownKind, Detection) {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 5, &calm());
        events.extend(feed(&mut detector, 1_005, 45, &moment));
        events.extend(feed(&mut detector, 1_050, 90, &calm()));
        let ended = ended(&events);
        assert_eq!(ended.len(), 1, "{events:?}");
        let detection = ended[0].1.clone();
        (detection.leading().unwrap().kind, detection)
    }

    #[test]
    fn each_trigger_kind_is_named_and_ranked_by_its_own_resource() {
        let (kind, detection) = kind_of(overwhelmed_processor());
        assert_eq!(kind, SlowdownKind::CpuPressure);
        assert_eq!(detection.processes_by, SlowdownResource::Processor);
        assert_eq!(detection.processes[0].name, "compiler");

        let (kind, detection) = kind_of(memory_stall());
        assert_eq!(kind, SlowdownKind::MemoryPressure);
        assert_eq!(detection.processes_by, SlowdownResource::Memory);
        assert_eq!(detection.processes[0].name, "browser");

        let (kind, detection) = kind_of(Moment {
            io: (30.0, 25.0),
            ..Moment::default()
        });
        assert_eq!(kind, SlowdownKind::DiskPressure);
        assert_eq!(detection.processes_by, SlowdownResource::Disk);
        assert!(
            (detection.leading().unwrap().worst - 25.0).abs() < 1e-9,
            "the disk's worst is everything waiting at once"
        );
        assert_eq!(detection.processes[0].name, "indexer");
        assert_eq!(detection.processes[0].disk_bytes_rate, Some(9.0e7));
        assert_eq!(detection.processes.len(), 1, "nobody else read anything");

        let (kind, detection) = kind_of(Moment {
            system: vec![
                ("memory.total", 8.0e9),
                ("memory.available", 0.2e9),
                ("memory.swap_in.rate", 400.0),
                ("memory.swap_out.rate", 900.0),
            ],
            ..Moment::default()
        });
        assert_eq!(kind, SlowdownKind::MemoryExhausted);
        assert!((detection.leading().unwrap().worst - 2.5).abs() < 1e-9);

        let (kind, detection) = kind_of(Moment {
            system: vec![("cpu.thermal_throttle.rate", 3.0)],
            ..Moment::default()
        });
        assert_eq!(kind, SlowdownKind::ThermalThrottling);
        assert_eq!(detection.leading().unwrap().count, 135);
        assert_eq!(detection.processes_by, SlowdownResource::Processor);
    }

    #[test]
    fn memory_nearly_gone_without_swapping_is_a_full_cache_and_not_a_slowdown() {
        let mut detector = detector();
        let events = feed(
            &mut detector,
            1_000,
            60,
            &Moment {
                system: vec![
                    ("memory.total", 8.0e9),
                    ("memory.available", 0.2e9),
                    ("memory.swap_in.rate", 0.0),
                    ("memory.swap_out.rate", 0.0),
                ],
                ..Moment::default()
            },
        );
        assert!(events.is_empty(), "{events:?}");
    }

    fn killed() -> Moment {
        Moment {
            memory: (45.0, 12.0),
            system: vec![("memory.oom.kill.rate", 1.0)],
            gone: vec!["browser"],
            ..Moment::default()
        }
    }

    #[test]
    fn a_program_ended_for_memory_is_a_slowdown_at_once_and_leads() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 5, &calm());
        events.extend(feed(&mut detector, 1_005, 20, &memory_stall()));
        events.extend(feed(&mut detector, 1_025, 1, &killed()));
        events.extend(feed(
            &mut detector,
            1_026,
            90,
            &Moment {
                gone: vec!["browser"],
                ..Moment::default()
            },
        ));
        let ended = ended(&events);
        assert_eq!(ended.len(), 1, "{events:?}");
        let detection = &ended[0].1;
        assert_eq!(
            detection.leading().unwrap().kind,
            SlowdownKind::OutOfMemoryKill
        );
        assert_eq!(detection.leading().unwrap().count, 1);
        assert_eq!(detection.causes[1].kind, SlowdownKind::MemoryPressure);
        // The list taken while memory was short is kept: it names the
        // program that was using it, which the round of the kill no longer
        // has.
        assert_eq!(detection.processes_by, SlowdownResource::Memory);
        assert_eq!(detection.processes[0].name, "browser");
    }

    #[test]
    fn a_kill_on_its_own_is_recorded_at_once_with_the_processes_from_before_it() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 2_000, 3, &calm());
        events.extend(feed(
            &mut detector,
            2_003,
            1,
            &Moment {
                system: vec![("memory.oom.kill.rate", 2.0)],
                gone: vec!["browser"],
                ..Moment::default()
            },
        ));
        let began = began(&events);
        assert_eq!(began.len(), 1, "{events:?}");
        assert_eq!(began[0].1.processes[0].name, "browser");
    }

    #[test]
    fn one_throttle_event_is_a_spike_and_not_a_slowdown() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 5, &calm());
        events.extend(feed(
            &mut detector,
            1_005,
            1,
            &Moment {
                system: vec![("cpu.thermal_throttle.rate", 4.0)],
                ..Moment::default()
            },
        ));
        events.extend(feed(&mut detector, 1_006, 60, &calm()));
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_short_spike_of_another_resource_is_not_listed_as_a_cause() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 30, &memory_stall());
        events.extend(feed(
            &mut detector,
            1_030,
            3,
            &Moment {
                memory: (45.0, 12.0),
                cpu: 90.0,
                ..Moment::default()
            },
        ));
        events.extend(feed(&mut detector, 1_033, 90, &calm()));
        let detection = &ended(&events)[0].1;
        assert_eq!(detection.causes.len(), 1, "{:?}", detection.causes);
    }

    #[test]
    fn a_suspend_ends_the_slowdown_where_watching_stopped_and_adds_no_time() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 20, &memory_stall());
        // The machine slept for an hour and woke up still stalled.
        detector.sleep(3_580);
        events.extend(feed(&mut detector, 4_600, 5, &memory_stall()));
        let finished = ended(&events);
        assert_eq!(finished.len(), 1, "{events:?}");
        assert_eq!(finished[0].1.ended_at_unix_ms, 1_019_000);
        assert_eq!(
            finished[0].1.leading().unwrap().seconds,
            19,
            "no sleep in it"
        );
        // Five seconds after waking is not yet a new slowdown.
        assert_eq!(began(&events).len(), 1);

        // A short suspend inside a slowdown adds nothing either.
        let mut detector = self::detector();
        let mut events = feed(&mut detector, 1_000, 20, &memory_stall());
        detector.sleep(15);
        events.extend(feed(&mut detector, 1_035, 10, &memory_stall()));
        events.extend(feed(&mut detector, 1_045, 90, &calm()));
        for (_, detection) in ended(&events) {
            assert!(
                detection.leading().unwrap().seconds < 30,
                "{:?}",
                detection.causes
            );
        }
    }

    #[test]
    fn samples_forty_seconds_apart_under_memory_pressure_are_one_slowdown() {
        // Swapping so hard that the service itself gets a turn only every
        // forty seconds: the wall and monotonic clocks both moved, so the
        // computer was awake and stuck, which is the slowdown going on.
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 5, &calm());
        events.extend(feed_every(&mut detector, 1_005, 400, 40, &memory_stall()));
        events.extend(feed(&mut detector, 1_405, 90, &calm()));
        assert_eq!(began(&events).len(), 1, "{events:?}");
        let ended = ended(&events);
        assert_eq!(ended.len(), 1);
        let detection = &ended[0].1;
        assert_eq!(detection.started_at_unix_ms, 1_005_000);
        assert!(detection.duration_seconds() >= 360);
        let kinds: Vec<SlowdownKind> = detection.causes.iter().map(|cause| cause.kind).collect();
        assert!(kinds.contains(&SlowdownKind::MemoryPressure), "{kinds:?}");
        assert!(kinds.contains(&SlowdownKind::Stalled), "{kinds:?}");
    }

    #[test]
    fn a_long_freeze_alone_is_a_slowdown() {
        let mut detector = detector();
        let mut events = feed(&mut detector, 1_000, 5, &calm());
        events.extend(feed(&mut detector, 1_050, 1, &calm()));
        let began = began(&events);
        assert_eq!(began.len(), 1, "{events:?}");
        let cause = began[0].1.leading().copied().unwrap();
        assert_eq!(cause.kind, SlowdownKind::Stalled);
        assert!((cause.worst - 46.0).abs() < 1e-9, "{cause:?}");
        assert_eq!(began[0].1.started_at_unix_ms, 1_004_000);
    }

    #[test]
    fn a_big_copy_to_a_slow_stick_is_not_a_slowdown() {
        // One program waiting for the disk nearly all the time, the rest of
        // the desktop running.
        let mut detector = detector();
        let events = feed(
            &mut detector,
            1_000,
            600,
            &Moment {
                io: (88.0, 8.0),
                ..Moment::default()
            },
        );
        assert!(events.is_empty(), "{events:?}");
        // Everything waiting for the disk is.
        let events = feed(
            &mut detector,
            1_600,
            20,
            &Moment {
                io: (90.0, 40.0),
                ..Moment::default()
            },
        );
        assert_eq!(began(&events).len(), 1);
    }

    #[test]
    fn a_build_on_every_core_is_not_a_slowdown_and_an_overwhelmed_processor_is() {
        let mut detector = detector();
        let events = feed(&mut detector, 1_000, 900, &all_core_build());
        assert!(events.is_empty(), "{events:?}");
        // Overwhelmed for less than the processor's onset: still nothing.
        let events = feed(&mut detector, 1_900, 25, &overwhelmed_processor());
        assert!(events.is_empty(), "{events:?}");
        let events = feed(&mut detector, 1_925, 40, &overwhelmed_processor());
        assert_eq!(began(&events).len(), 1);
    }

    #[test]
    fn the_slowdown_going_on_can_be_read_as_it_stands() {
        let mut detector = detector();
        assert!(detector.d.current().is_none());
        feed(&mut detector, 1_000, 100, &memory_stall());
        let current = detector.d.current().unwrap();
        assert!(current.ongoing);
        assert_eq!(current.ended_at_unix_ms, 1_099_000);
        assert_eq!(current.duration_seconds(), 99);
    }

    #[test]
    fn a_reading_this_machine_cannot_make_is_never_a_slowdown() {
        let mut detector = detector();
        for second in 1_000..1_100 {
            let sample = Sample::from_reports(&[], 10);
            let mut sample = sample;
            sample.wall_unix_ms = second * 1_000;
            let events = detector.d.observe(&sample, &mut |_, _| Vec::new());
            assert!(events.is_empty());
        }
    }

    #[test]
    fn the_process_list_of_a_slowdown_carries_no_command_line_or_user() {
        let (_, detection) = kind_of(memory_stall());
        for process in &detection.processes {
            assert!(process.command_line.is_none());
            assert!(process.user.is_none());
        }
    }

    #[test]
    fn a_detected_incident_round_trips_and_one_marked_by_hand_reads_as_one() {
        // Against what v0.2.13 itself wrote and read: see the store's tests.
        let (_, detection) = kind_of(memory_stall());
        let mut detected = incident();
        detected.note = None;
        detected.detection = Some(detection);
        let encoded = serde_json::to_string(&detected).unwrap();
        assert_eq!(
            serde_json::from_str::<Incident>(&encoded).unwrap(),
            detected
        );
        assert!(!detected.marked_by_hand());

        let by_hand = incident();
        let encoded = serde_json::to_string(&by_hand).unwrap();
        assert!(!encoded.contains("detection"));
        assert!(
            serde_json::from_str::<Incident>(&encoded)
                .unwrap()
                .marked_by_hand()
        );
    }

    #[test]
    fn a_kind_from_a_newer_version_reads_as_other_rather_than_failing() {
        let cause: SlowdownCause =
            serde_json::from_str(r#"{"kind":"gpu_pressure","seconds":30}"#).unwrap();
        assert_eq!(cause.kind, SlowdownKind::Other);
        assert_eq!(cause.kind.resource(), SlowdownResource::Other);
    }
}

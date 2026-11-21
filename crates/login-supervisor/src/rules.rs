//! The login supervisor's rules (design/login-screen-spec.md section 5.10),
//! with no I/O: what the supervisor saw goes in as [`Seen`], what it does
//! comes out as [`Action`]s, and everything it must remember across its own
//! restarts is the [`State`] written to `/run/better-os/login-path.json`.
//!
//! One appearance of the login screen runs: stages 1 to 3 (a graphics
//! device with a screen, `seat0` graphical, logind answering) within 12 s of
//! T0, `plymouth deactivate` inside them, then up to three attempts at the
//! greeter of at most 15 s each (the third in safe mode) with a stop of at
//! most 6 s after each miss, then the fall back to a text login. Every
//! deadline is counted from times kept in the state, so a supervisor
//! restarted in the middle continues the same appearance with the same
//! clock.

use manager_ipc::boot_watch::{Evidence, LoginStage, RestartOutcome, SessionSeen};
use serde::{Deserialize, Serialize};

/// Microseconds on `CLOCK_MONOTONIC`, the clock systemd's `*Monotonic`
/// timestamps use. It does not run during a suspend, so a suspend at the
/// login screen never looks like a missed deadline.
pub type Micros = u64;

pub const SECOND: Micros = 1_000_000;

/// The deadlines, every one of them from the spec's section 5.10.
pub mod limits {
    use super::{Micros, SECOND};

    /// How often the supervisor looks.
    pub const TICK: Micros = SECOND / 4;
    /// Stages 1 to 3, from T0.
    pub const STAGES: Micros = 12 * SECOND;
    /// `plymouth deactivate` is sent by then at the latest, so that with its
    /// own limit it has returned, or `plymouthd` has been killed, by
    /// [`STAGES`].
    pub const PLYMOUTH_LATEST_SEND: Micros = STAGES - PLYMOUTH;
    /// Every `plymouth` command's limit.
    pub const PLYMOUTH: Micros = 5 * SECOND;
    /// Attempts at the greeter per appearance.
    pub const ATTEMPTS: u8 = 3;
    /// Into an attempt: the greeter's session (stage 5).
    pub const STAGE_5: Micros = 5 * SECOND;
    /// Into an attempt: the first heartbeat (stage 6).
    pub const STAGE_6: Micros = 10 * SECOND;
    /// Into an attempt: `ready` (stage 7).
    pub const STAGE_7: Micros = 15 * SECOND;
    /// A stop of the greeter's unit: `SIGTERM`, 4 s, `SIGKILL`, 2 s.
    pub const STOP: Micros = 6 * SECOND;
    /// The text login accepting input once started.
    pub const TEXT_LOGIN: Micros = 3 * SECOND;
    /// The property.
    pub const BUDGET: Micros = 80 * SECOND;
    /// The backstop's timer.
    pub const BACKSTOP: Micros = 90 * SECOND;
    /// After `ready`, the heartbeat is never older than this while no
    /// person's session runs.
    pub const HEARTBEAT_STALE: Micros = 30 * SECOND;
    /// Every call to logind or systemd.
    pub const CALL: Micros = 2 * SECOND;

    /// The budget written out, with every stop in it (12, then 15 and 6
    /// three times, then 3: 78 s), leaving the rest of the 80 s for the
    /// supervisor noticing each of its seven deadlines up to a tick late.
    pub const WORST_CASE: Micros =
        STAGES + ATTEMPTS as Micros * (STAGE_7 + STOP) + TEXT_LOGIN + 7 * TICK;
    const _: () = assert!(WORST_CASE <= BUDGET);
    const _: () = assert!(PLYMOUTH_LATEST_SEND + PLYMOUTH <= STAGES);
    const _: () = assert!(PLYMOUTH < STOP);
}

/// The record format of `login-path.json`. A file of another version is
/// unreadable, never guessed at.
pub const STATE_VERSION: u32 = 1;

/// The two virtual terminals the fall back may use.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tty {
    Tty1,
    Tty2,
}

impl Tty {
    pub fn number(self) -> u8 {
        match self {
            Tty::Tty1 => 1,
            Tty::Tty2 => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tty::Tty1 => "tty1",
            Tty::Tty2 => "tty2",
        }
    }

    pub fn device(self) -> &'static str {
        match self {
            Tty::Tty1 => "/dev/tty1",
            Tty::Tty2 => "/dev/tty2",
        }
    }
}

/// The units the supervisor may start or stop: a closed set, so no input
/// can name another (the security review's fixed list).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Greeter,
    Fallback(Tty),
}

/// Every unit name [`Unit`] can produce.
pub const UNITS: [&str; 3] = [
    "better-os-greeter.service",
    "better-os-login-fallback@tty1.service",
    "better-os-login-fallback@tty2.service",
];

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Greeter => UNITS[0],
            Unit::Fallback(Tty::Tty1) => UNITS[1],
            Unit::Fallback(Tty::Tty2) => UNITS[2],
        }
    }
}

/// The stages a journal line names: the spec's table, 1 to 9.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// 1.
    Device,
    /// 1a.
    DriverLate,
    /// 2.
    Seat,
    /// 3.
    Logind,
    /// 4.
    Plymouth,
    /// 5.
    GreeterSession,
    /// 6.
    Compositor,
    /// 7.
    Greeter,
    /// 9.
    SessionEnd,
    /// The supervisor itself.
    Supervisor,
}

impl Step {
    pub fn code(self) -> &'static str {
        match self {
            Step::Device => "1",
            Step::DriverLate => "1a",
            Step::Seat => "2",
            Step::Logind => "3",
            Step::Plymouth => "4",
            Step::GreeterSession => "5",
            Step::Compositor => "6",
            Step::Greeter => "7",
            Step::SessionEnd => "9",
            Step::Supervisor => "supervisor",
        }
    }

    /// The stage a record and the text login's message name. Only the
    /// stages that can end an attempt or an appearance have one of their
    /// own; the others are reported as the greeter's.
    pub fn stage(self) -> LoginStage {
        match self {
            Step::Device => LoginStage::GraphicsDevice,
            Step::Logind => LoginStage::LoginManager,
            Step::GreeterSession => LoginStage::SignInService,
            Step::Compositor => LoginStage::DisplayServer,
            Step::Supervisor => LoginStage::Supervisor,
            Step::DriverLate | Step::Seat | Step::Plymouth | Step::Greeter | Step::SessionEnd => {
                LoginStage::LoginScreenProgram
            }
        }
    }
}

/// What a stage that did not simply pass ended in.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// It continued, with something to say (the firmware framebuffer only,
    /// seat0 not graphical, `plymouthd` killed, the supervisor restarted).
    Ok,
    Retried,
    FellBack,
}

impl Outcome {
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Retried => "retried",
            Outcome::FellBack => "fell-back",
        }
    }
}

/// Why: a closed list, so a journal line and a record carry nothing the
/// supervisor did not write itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NoGraphicsDevice,
    FirmwareFramebufferOnly,
    NoScreen,
    ScreenConnected,
    RealDriverBound,
    SeatNotGraphical,
    LogindNoAnswer,
    PlymouthDidNotReturn,
    NoGreeterSession,
    GreeterAccountMissing,
    NoHeartbeat,
    NotReady,
    HeartbeatStale,
    StopDidNotComplete,
    FallbackStartRefused,
    StateUnreadable,
    Restarted,
}

impl Reason {
    pub fn word(self) -> &'static str {
        match self {
            Reason::NoGraphicsDevice => "no-graphics-device",
            Reason::FirmwareFramebufferOnly => "firmware-framebuffer-only",
            Reason::NoScreen => "no-screen",
            Reason::ScreenConnected => "screen-connected",
            Reason::RealDriverBound => "real-driver-bound",
            Reason::SeatNotGraphical => "seat-not-graphical",
            Reason::LogindNoAnswer => "logind-no-answer",
            Reason::PlymouthDidNotReturn => "plymouth-did-not-return",
            Reason::NoGreeterSession => "no-greeter-session",
            Reason::GreeterAccountMissing => "greeter-account-missing",
            Reason::NoHeartbeat => "no-heartbeat",
            Reason::NotReady => "not-ready",
            Reason::HeartbeatStale => "heartbeat-stale",
            Reason::StopDidNotComplete => "stop-did-not-complete",
            Reason::FallbackStartRefused => "fallback-start-refused",
            Reason::StateUnreadable => "state-unreadable",
            Reason::Restarted => "restarted",
        }
    }
}

/// One stage that did not simply pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub step: Step,
    pub outcome: Outcome,
    pub reason: Reason,
}

impl Note {
    /// `BETTER_OS_LOGIN_STAGE=<stage> RESULT=<ok|retried|fell-back> REASON=<…>`.
    pub fn journal_line(&self) -> String {
        format!(
            "BETTER_OS_LOGIN_STAGE={} RESULT={} REASON={}",
            self.step.code(),
            self.outcome.word(),
            self.reason.word()
        )
    }
}

/// A `plymouth` command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlymouthCommand {
    Deactivate,
    QuitRetainSplash,
    Quit,
}

impl PlymouthCommand {
    pub fn arguments(self) -> &'static [&'static str] {
        match self {
            PlymouthCommand::Deactivate => &["deactivate"],
            PlymouthCommand::QuitRetainSplash => &["quit", "--retain-splash"],
            PlymouthCommand::Quit => &["quit"],
        }
    }
}

/// What the greeter's start program reads for the third attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GreeterMode {
    Plain,
    Safe,
}

/// What the supervisor does. Each is carried out by the process edges
/// (`world::World::perform`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    StartUnit(Unit),
    StopUnit(Unit),
    /// Start the command; the supervisor polls it, never waits on it.
    Plymouth(PlymouthCommand),
    /// Kill the command still running and `plymouthd`.
    KillPlymouth,
    SetGreeterMode(GreeterMode),
    /// `KDSETMODE KD_TEXT` and `VT_SETMODE VT_AUTO`.
    ResetTty(Tty),
    /// `VT_ACTIVATE`, which is what `chvt` does.
    ActivateVt(Tty),
    /// `/run/better-os/login-fallback.issue`, the stage's English name and
    /// the time filled in.
    WriteIssue(LoginStage),
    /// `/run/better-os/login-fallback.active`, before the text login starts,
    /// so the backstop never starts a second one.
    MarkFallbackActive(Tty),
    /// `/run/better-os/login-held`: the property held this boot (the
    /// backstop's third condition).
    MarkHeld,
    Journal(String),
    Record(Box<RecordDraft>),
}

/// An event for the records directory; the edges add the boot id and the
/// wall-clock time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordDraft {
    pub waited_seconds: u32,
    pub evidence: Evidence,
    pub restart: RestartOutcome,
}

// What the supervisor saw. Unknown is `None` wherever it matters, and the
// rules never read an unknown as a pass.

/// One card under `/sys/class/drm`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Card {
    pub name: String,
    /// Its driver is a firmware framebuffer, or could not be read.
    pub firmware: bool,
    /// A connector that is not a writeback says `connected` or `unknown`.
    pub screen: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Drm {
    pub cards: Vec<Card>,
    /// Every entry of the directory, for a record.
    pub entries: Vec<String>,
}

/// What stage 1 sees.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cards {
    RealWithScreen,
    RealWithoutScreen,
    FirmwareOnly,
    None,
}

impl Cards {
    pub fn of(drm: Option<&Drm>) -> Cards {
        let Some(drm) = drm else {
            return Cards::None;
        };
        let real = || drm.cards.iter().filter(|card| !card.firmware);
        if real().any(|card| card.screen) {
            Cards::RealWithScreen
        } else if real().next().is_some() {
            Cards::RealWithoutScreen
        } else if drm.cards.is_empty() {
            Cards::None
        } else {
            Cards::FirmwareOnly
        }
    }
}

/// One session from logind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogindSession {
    pub id: String,
    pub uid: u32,
    pub class: String,
    /// The PAM service: `greetd` for a person's session greetd started,
    /// `greetd-greeter` for the greeter's.
    pub service: String,
    pub seat: String,
    pub kind: String,
}

impl LogindSession {
    /// A person's session that greetd runs on the first seat.
    pub fn is_greetd_person(&self) -> bool {
        self.class.starts_with("user") && self.service == "greetd" && self.seat == "seat0"
    }

    pub fn is_greeter(&self) -> bool {
        self.class == "greeter"
    }
}

/// logind, when it answered this time.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Logind {
    pub can_graphical: bool,
    pub sessions: Vec<LogindSession>,
}

/// One process of greetd's tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub uid: u32,
    /// In `better-os-greeter.service`'s own cgroup, as opposed to a
    /// descendant moved into a session's scope.
    pub in_cgroup: bool,
}

/// greetd's tree: the processes of its unit's cgroup and their descendants.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Greetd {
    pub processes: Vec<Process>,
}

impl Greetd {
    /// The unit has no process left: a stop has completed.
    pub fn stopped(&self) -> bool {
        !self.processes.iter().any(|p| p.in_cgroup)
    }

    fn in_cgroup(&self) -> usize {
        self.processes.iter().filter(|p| p.in_cgroup).count()
    }

    /// A process of an account other than root and the greeter's: a
    /// person's session, certainly when the greeter's account is known.
    fn foreign(&self, greeter: Option<u32>) -> bool {
        self.processes
            .iter()
            .any(|p| p.uid != 0 && Some(p.uid) != greeter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Word {
    Starting,
    Ready,
    Other,
}

/// The greeter's heartbeat file as it is now: `identity` changes with every
/// rewrite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeartbeatFile {
    pub identity: u64,
    pub word: Word,
}

/// A `plymouth` command the edges started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Job {
    Running,
    Finished,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Seen {
    /// `None`: `/sys/class/drm` could not be read.
    pub drm: Option<Drm>,
    /// `None`: logind did not answer within its limit.
    pub logind: Option<Logind>,
    /// `None`: the cgroup or `/proc` could not be read.
    pub greetd: Option<Greetd>,
    /// `None`: no heartbeat file, or not one that can be read.
    pub heartbeat: Option<HeartbeatFile>,
    /// `None`: the edges have no command running or finished (after a
    /// restart of the supervisor, too).
    pub plymouth: Option<Job>,
    /// `_better-greeter`'s uid in `/etc/passwd`.
    pub greeter_uid: Option<u32>,
}

impl Seen {
    /// The greeter's account: `/etc/passwd`, or the greeter session logind
    /// lists.
    fn greeter(&self) -> Option<u32> {
        self.greeter_uid.or_else(|| {
            self.logind
                .as_ref()?
                .sessions
                .iter()
                .find(|s| s.is_greeter())
                .map(|s| s.uid)
        })
    }

    fn person_by_logind(&self) -> bool {
        self.logind
            .as_ref()
            .is_some_and(|l| l.sessions.iter().any(LogindSession::is_greetd_person))
    }

    /// A person's session runs, for certain: one that satisfies the
    /// property (an automatic login). greetd's tree decides when it can be
    /// read, because logind may be the thing that failed and still list a
    /// session that is gone (ticket 256); with the greeter's account
    /// unknown, a process of another account counts only when logind
    /// agrees.
    pub fn person_certain(&self) -> bool {
        let greeter = self.greeter();
        match (&self.greetd, greeter) {
            (Some(tree), Some(_)) => tree.foreign(greeter),
            (Some(tree), None) => tree.foreign(None) && self.person_by_logind(),
            (None, _) => self.person_by_logind(),
        }
    }

    /// A person's session may run: then the greeter's unit is never
    /// stopped, because greetd ends the session it runs when it is. With
    /// neither greetd's tree nor logind readable nothing says one runs, and
    /// the property comes first.
    pub fn person_possible(&self) -> bool {
        match &self.greetd {
            Some(tree) => tree.foreign(self.greeter()),
            None => self.person_by_logind(),
        }
    }

    /// Whether a person's session greetd ran has ended: greetd's tree holds
    /// no process of a person, whatever logind says; logind decides only
    /// when the tree cannot be read; with neither, it has not.
    fn session_ended(&self) -> bool {
        match &self.greetd {
            Some(tree) => !tree.foreign(self.greeter()),
            None => self.logind.is_some() && !self.person_by_logind(),
        }
    }

    /// Stage 5: the greeter's session is open. With logind answering, a
    /// session of class `greeter` (of `_better-greeter` when its uid is
    /// known); without, a second process in the unit's cgroup.
    fn greeter_session_open(&self) -> bool {
        match &self.logind {
            Some(logind) => {
                let greeter = self.greeter_uid;
                logind
                    .sessions
                    .iter()
                    .any(|s| s.is_greeter() && greeter.is_none_or(|uid| uid == s.uid))
            }
            None => self
                .greetd
                .as_ref()
                .is_some_and(|tree| tree.in_cgroup() >= 2),
        }
    }

    /// The unit's cgroup is empty. Unknown is not stopped.
    fn greeter_stopped(&self) -> bool {
        self.greetd.as_ref().is_some_and(Greetd::stopped)
    }

    fn evidence(&self, notes: &[Note]) -> Evidence {
        Evidence {
            display_manager_journal: notes.iter().map(|n| n.journal_line() + "\n").collect(),
            seat0_can_graphical: self.logind.as_ref().map(|l| l.can_graphical),
            drm_devices: self
                .drm
                .as_ref()
                .map(|d| d.entries.clone())
                .unwrap_or_default(),
            sessions: self
                .logind
                .as_ref()
                .map(|l| {
                    l.sessions
                        .iter()
                        .map(|s| SessionSeen {
                            id: s.id.clone(),
                            class: s.class.clone(),
                            kind: s.kind.clone(),
                            seat: s.seat.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            problems: Vec::new(),
            ..Evidence::default()
        }
    }
}

// What the supervisor remembers.

/// The last heartbeat change it saw, on its own clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    pub identity: u64,
    pub word: Word,
    pub changed_at: Micros,
}

/// A `plymouth` command in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlymouthJob {
    pub command: PlymouthCommand,
    pub deadline: Micros,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlymouthState {
    pub deactivated: bool,
    pub quit: bool,
    pub job: Option<PlymouthJob>,
}

/// What follows a stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "then")]
pub enum Then {
    Attempt {
        number: u8,
    },
    /// A new appearance from stage 1, its T0 the moment it was decided.
    NewAppearance {
        t0: Micros,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum Phase {
    /// Stages 1 to 3. `baseline` is the heartbeat's identity when the
    /// appearance began: a greeter greetd brought back after a session,
    /// ready since then, needs nothing more.
    Checking { baseline: Option<u64> },
    /// An attempt at the greeter.
    Attempt {
        number: u8,
        started: Micros,
        /// The heartbeat's identity when the attempt started; a different
        /// one was written during it.
        baseline: Option<u64>,
        session_seen: bool,
        heartbeat_seen: bool,
    },
    /// The greeter's unit is being stopped.
    Stopping {
        then: Then,
        deadline: Micros,
        step: Step,
    },
    /// The greeter accepts input.
    Ready { since: Micros },
    /// The greeter is started on a card with no screen: the property is
    /// suspended until one is connected.
    NoScreen { since: Micros },
    /// A person's session runs.
    Session { since: Micros },
    /// The fall back: the stop and `plymouth quit`, at most 6 s.
    FallingBack { step: Step, deadline: Micros },
    /// The text login runs; nothing more this boot.
    FellBack { step: Step, tty: Tty, at: Micros },
    /// The state file could not be read: decided at the first look.
    Unreadable,
}

/// `/run/better-os/login-path.json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u32,
    /// This boot's id; a file naming another boot is not this boot's.
    pub boot_id: Option<String>,
    /// 1 at boot, one more for each later appearance.
    pub appearance: u32,
    pub t0: Micros,
    pub phase: Phase,
    /// This appearance's stages that did not simply pass.
    pub notes: Vec<Note>,
    /// The greeter was started on the firmware framebuffer this appearance.
    pub on_firmware: bool,
    pub plymouth: PlymouthState,
    pub heartbeat: Option<Beat>,
}

/// What reading the state file gave.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Loaded {
    Absent,
    Unreadable,
    Found(Box<State>),
}

impl State {
    /// A new boot's state: T0 is when the unit began to start this boot
    /// (`InactiveExitTimestampMonotonic`), or now when that is unknown or
    /// later than now.
    pub fn fresh(boot_id: Option<String>, first_start: Option<Micros>, now: Micros) -> State {
        State {
            version: STATE_VERSION,
            boot_id,
            appearance: 1,
            t0: first_start.filter(|t| *t > 0 && *t <= now).unwrap_or(now),
            phase: Phase::Checking { baseline: None },
            notes: Vec::new(),
            on_firmware: false,
            plymouth: PlymouthState::default(),
            heartbeat: None,
        }
    }

    /// The stage the appearance is at, for the test hook and the status.
    pub fn current_step(&self) -> Option<Step> {
        match self.phase {
            Phase::Checking { .. } => Some(Step::Device),
            Phase::Attempt {
                session_seen,
                heartbeat_seen,
                ..
            } => Some(if !session_seen {
                Step::GreeterSession
            } else if !heartbeat_seen {
                Step::Compositor
            } else {
                Step::Greeter
            }),
            Phase::Stopping { step, .. } | Phase::FallingBack { step, .. } => Some(step),
            Phase::Ready { .. } | Phase::NoScreen { .. } | Phase::Session { .. } => None,
            Phase::FellBack { step, .. } => Some(step),
            Phase::Unreadable => Some(Step::Supervisor),
        }
    }
}

/// The machine: the state and the rules over it.
#[derive(Clone, Debug)]
pub struct Machine {
    pub state: State,
}

/// How a start continued.
pub fn start(
    loaded: Loaded,
    boot_id: Option<String>,
    first_start: Option<Micros>,
    now: Micros,
) -> (Machine, Vec<Action>) {
    match loaded {
        Loaded::Found(state)
            if state.version == STATE_VERSION
                && (state.boot_id.is_none() || boot_id.is_none() || state.boot_id == boot_id) =>
        {
            let mut machine = Machine { state: *state };
            let actions = machine.resumed();
            (machine, actions)
        }
        Loaded::Found(_) | Loaded::Absent => (
            Machine {
                state: State::fresh(boot_id, first_start, now),
            },
            Vec::new(),
        ),
        Loaded::Unreadable => {
            let mut state = State::fresh(boot_id, first_start, now);
            state.phase = Phase::Unreadable;
            (Machine { state }, Vec::new())
        }
    }
}

impl Machine {
    /// A supervisor restarted in the middle: it says so and asks again for
    /// what it may have been cut off asking. Starting and stopping a unit
    /// are both idempotent.
    fn resumed(&mut self) -> Vec<Action> {
        let note = Note {
            step: Step::Supervisor,
            outcome: Outcome::Ok,
            reason: Reason::Restarted,
        };
        let mut actions = vec![Action::Journal(note.journal_line())];
        match self.state.phase {
            Phase::Stopping { .. } | Phase::FallingBack { .. } => {
                actions.push(Action::StopUnit(Unit::Greeter));
            }
            Phase::Attempt { number, .. } => {
                actions.push(Action::SetGreeterMode(mode(number)));
                actions.push(Action::StartUnit(Unit::Greeter));
            }
            _ => {}
        }
        actions
    }

    /// One look: what to do now.
    pub fn step(&mut self, now: Micros, seen: &Seen) -> Vec<Action> {
        let mut out = Vec::new();
        self.track_heartbeat(now, seen);
        self.track_plymouth(now, seen, &mut out);
        match self.state.phase {
            Phase::Unreadable => self.unreadable(now, seen, &mut out),
            Phase::Checking { .. } => self.checking(now, seen, &mut out),
            Phase::Attempt { .. } => self.attempt(now, seen, &mut out),
            Phase::Stopping { .. } => self.stopping(now, seen, &mut out),
            Phase::Ready { .. } => self.ready(now, seen, &mut out),
            Phase::NoScreen { .. } => self.no_screen(now, seen, &mut out),
            Phase::Session { .. } => self.session(now, seen),
            Phase::FallingBack { .. } => self.falling_back(now, seen, &mut out),
            Phase::FellBack { .. } => {}
        }
        out
    }

    /// An action the edges could not carry out. Only a refused start of
    /// the text login changes what happens: it moves to tty2. Returns the
    /// actions that replace the rest of the failed list.
    pub fn failed(&mut self, now: Micros, seen: &Seen, action: &Action) -> Option<Vec<Action>> {
        if let (Action::StartUnit(Unit::Fallback(Tty::Tty1)), Phase::FellBack { step, .. }) =
            (action, self.state.phase)
        {
            let mut out = Vec::new();
            self.note(
                step,
                Outcome::FellBack,
                Reason::FallbackStartRefused,
                &mut out,
            );
            self.complete(now, seen, step, Tty::Tty2, &mut out);
            return Some(out);
        }
        None
    }

    fn note(&mut self, step: Step, outcome: Outcome, reason: Reason, out: &mut Vec<Action>) {
        let note = Note {
            step,
            outcome,
            reason,
        };
        out.push(Action::Journal(note.journal_line()));
        self.state.notes.push(note);
    }

    fn track_heartbeat(&mut self, now: Micros, seen: &Seen) {
        let Some(file) = seen.heartbeat else {
            return;
        };
        if self.state.heartbeat.map(|b| b.identity) != Some(file.identity) {
            self.state.heartbeat = Some(Beat {
                identity: file.identity,
                word: file.word,
                changed_at: now,
            });
        }
    }

    /// A heartbeat written since `baseline` was taken.
    fn beat_since(&self, baseline: Option<u64>) -> Option<Beat> {
        self.state
            .heartbeat
            .filter(|beat| Some(beat.identity) != baseline)
    }

    fn baseline(&self) -> Option<u64> {
        self.state.heartbeat.map(|b| b.identity)
    }

    fn track_plymouth(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Some(job) = self.state.plymouth.job else {
            return;
        };
        match seen.plymouth {
            Some(Job::Finished) => self.plymouth_done(job.command),
            Some(Job::Running) if now < job.deadline => {}
            // Running past its limit, or cut off by a restart of the
            // supervisor with its time used up.
            Some(Job::Running) | None if now >= job.deadline => {
                out.push(Action::KillPlymouth);
                self.note(
                    Step::Plymouth,
                    Outcome::Ok,
                    Reason::PlymouthDidNotReturn,
                    out,
                );
                // A killed plymouthd holds nothing and draws nothing.
                self.state.plymouth.deactivated = true;
                self.state.plymouth.quit = true;
                self.state.plymouth.job = None;
            }
            // Cut off by a restart: sent again, with what was left of its
            // limit.
            _ => out.push(Action::Plymouth(job.command)),
        }
    }

    fn plymouth_done(&mut self, command: PlymouthCommand) {
        match command {
            PlymouthCommand::Deactivate => self.state.plymouth.deactivated = true,
            PlymouthCommand::Quit | PlymouthCommand::QuitRetainSplash => {
                self.state.plymouth.deactivated = true;
                self.state.plymouth.quit = true;
            }
        }
        self.state.plymouth.job = None;
    }

    fn plymouth(&mut self, now: Micros, command: PlymouthCommand, out: &mut Vec<Action>) {
        if self.state.plymouth.quit || self.state.plymouth.job.is_some() {
            return;
        }
        if command == PlymouthCommand::Deactivate && self.state.plymouth.deactivated {
            return;
        }
        self.state.plymouth.job = Some(PlymouthJob {
            command,
            deadline: now + limits::PLYMOUTH,
        });
        out.push(Action::Plymouth(command));
    }

    fn plymouth_settled(&self) -> bool {
        self.state.plymouth.job.is_none()
    }

    fn unreadable(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        if seen.person_possible() {
            self.enter_session(now, None, seen, out);
        } else if seen.heartbeat.is_some_and(|h| h.word == Word::Ready) {
            // The greeter says it is ready: taken as such, and held to its
            // heartbeat from now on like any ready greeter.
            out.push(Action::MarkHeld);
            self.state.phase = Phase::Ready { since: now };
        } else {
            self.fall_back(now, Step::Supervisor, Reason::StateUnreadable, out);
        }
    }

    fn checking(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        if seen.person_certain() {
            return self.enter_session(now, None, seen, out);
        }
        let Phase::Checking { baseline } = self.state.phase else {
            return;
        };
        if self
            .beat_since(baseline)
            .is_some_and(|b| b.word == Word::Ready)
        {
            return self.enter_ready(now, 1, seen, out);
        }
        let t0 = self.state.t0;
        let cards = Cards::of(seen.drm.as_ref());
        let passed =
            cards == Cards::RealWithScreen && seen.logind.as_ref().is_some_and(|l| l.can_graphical);
        if passed || now >= t0 + limits::PLYMOUTH_LATEST_SEND {
            self.plymouth(now, PlymouthCommand::Deactivate, out);
        }
        if !passed && now < t0 + limits::STAGES {
            return;
        }
        if !self.plymouth_settled() {
            return;
        }
        if !passed {
            if cards == Cards::None {
                return self.fall_back(now, Step::Device, Reason::NoGraphicsDevice, out);
            }
            let Some(logind) = &seen.logind else {
                return self.fall_back(now, Step::Logind, Reason::LogindNoAnswer, out);
            };
            let can_graphical = logind.can_graphical;
            match cards {
                Cards::FirmwareOnly => self.note(
                    Step::Device,
                    Outcome::Ok,
                    Reason::FirmwareFramebufferOnly,
                    out,
                ),
                Cards::RealWithoutScreen => {
                    self.note(Step::Device, Outcome::Ok, Reason::NoScreen, out)
                }
                Cards::RealWithScreen | Cards::None => {}
            }
            if !can_graphical {
                self.note(Step::Seat, Outcome::Ok, Reason::SeatNotGraphical, out);
            }
        }
        self.state.on_firmware = cards == Cards::FirmwareOnly;
        self.begin_attempt(now, 1, out);
        if cards == Cards::RealWithoutScreen {
            out.push(Action::MarkHeld);
            self.state.phase = Phase::NoScreen { since: now };
        }
    }

    fn begin_attempt(&mut self, now: Micros, number: u8, out: &mut Vec<Action>) {
        out.push(Action::SetGreeterMode(mode(number)));
        out.push(Action::StartUnit(Unit::Greeter));
        self.state.phase = Phase::Attempt {
            number,
            started: now,
            baseline: self.baseline(),
            session_seen: false,
            heartbeat_seen: false,
        };
    }

    fn attempt(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::Attempt {
            number,
            started,
            baseline,
            mut session_seen,
            mut heartbeat_seen,
        } = self.state.phase
        else {
            return;
        };
        if seen.person_certain() {
            return self.enter_session(now, Some(number), seen, out);
        }
        let beat = self.beat_since(baseline);
        if beat.is_some_and(|b| b.word == Word::Ready) {
            return self.enter_ready(now, number, seen, out);
        }
        if self.driver_bound_late(seen) {
            return self.restart_appearance(now, Step::DriverLate, out);
        }
        session_seen |= seen.greeter_session_open();
        heartbeat_seen |= beat.is_some_and(|b| matches!(b.word, Word::Starting | Word::Ready));
        self.state.phase = Phase::Attempt {
            number,
            started,
            baseline,
            session_seen,
            heartbeat_seen,
        };
        let elapsed = now.saturating_sub(started);
        if elapsed >= limits::STAGE_7 {
            self.miss(now, number, Step::Greeter, Reason::NotReady, seen, out);
        } else if elapsed >= limits::STAGE_6 && !heartbeat_seen {
            self.miss(
                now,
                number,
                Step::Compositor,
                Reason::NoHeartbeat,
                seen,
                out,
            );
        } else if elapsed >= limits::STAGE_5 && !session_seen {
            let reason = if seen.greeter_uid.is_none() {
                Reason::GreeterAccountMissing
            } else {
                Reason::NoGreeterSession
            };
            self.miss(now, number, Step::GreeterSession, reason, seen, out);
        }
    }

    /// The real driver bound after the greeter was started on the firmware
    /// framebuffer (stage 1a): restart it on the new card, never while a
    /// person's session may run.
    fn driver_bound_late(&self, seen: &Seen) -> bool {
        self.state.on_firmware
            && Cards::of(seen.drm.as_ref()) == Cards::RealWithScreen
            && !seen.person_possible()
    }

    fn restart_appearance(&mut self, now: Micros, step: Step, out: &mut Vec<Action>) {
        let reason = match step {
            Step::DriverLate => Reason::RealDriverBound,
            _ => Reason::HeartbeatStale,
        };
        let outcome = match step {
            Step::DriverLate => Outcome::Ok,
            _ => Outcome::Retried,
        };
        self.note(step, outcome, reason, out);
        out.push(Action::StopUnit(Unit::Greeter));
        self.state.phase = Phase::Stopping {
            then: Then::NewAppearance { t0: now },
            deadline: now + limits::STOP,
            step: Step::Greeter,
        };
    }

    fn miss(
        &mut self,
        now: Micros,
        number: u8,
        step: Step,
        reason: Reason,
        seen: &Seen,
        out: &mut Vec<Action>,
    ) {
        // greetd ends the session it runs when it is stopped: with a
        // person's session possible, the attempt is held rather than ended.
        if seen.person_possible() {
            return;
        }
        if number < limits::ATTEMPTS {
            self.note(step, Outcome::Retried, reason, out);
            out.push(Action::StopUnit(Unit::Greeter));
            self.state.phase = Phase::Stopping {
                then: Then::Attempt { number: number + 1 },
                deadline: now + limits::STOP,
                step,
            };
        } else {
            self.fall_back(now, step, reason, out);
        }
    }

    fn stopping(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::Stopping {
            then,
            deadline,
            step,
        } = self.state.phase
        else {
            return;
        };
        if seen.greeter_stopped() {
            match then {
                Then::Attempt { number } => self.begin_attempt(now, number, out),
                Then::NewAppearance { t0 } => self.new_appearance(t0),
            }
        } else if now >= deadline {
            // A process the kernel cannot kill keeps the unit: no start can
            // follow, so the text login starts on tty2 at once.
            self.note(step, Outcome::FellBack, Reason::StopDidNotComplete, out);
            // Not waited for: its own limit is watched from the next look.
            self.plymouth(now, PlymouthCommand::Quit, out);
            self.complete(now, seen, step, Tty::Tty2, out);
        }
    }

    fn new_appearance(&mut self, t0: Micros) {
        self.state.appearance += 1;
        self.state.t0 = t0;
        self.state.notes.clear();
        self.state.on_firmware = false;
        self.state.phase = Phase::Checking {
            baseline: self.baseline(),
        };
    }

    fn enter_ready(&mut self, now: Micros, number: u8, seen: &Seen, out: &mut Vec<Action>) {
        self.plymouth(now, PlymouthCommand::QuitRetainSplash, out);
        out.push(Action::MarkHeld);
        self.record_restarts(now, number, seen, out);
        self.state.phase = Phase::Ready { since: now };
    }

    fn enter_session(
        &mut self,
        now: Micros,
        number: Option<u8>,
        seen: &Seen,
        out: &mut Vec<Action>,
    ) {
        // An automatic login's own compositor draws now; the splash goes.
        self.plymouth(now, PlymouthCommand::Quit, out);
        out.push(Action::MarkHeld);
        if let Some(number) = number {
            self.record_restarts(now, number, seen, out);
        }
        self.state.phase = Phase::Session { since: now };
    }

    fn record_restarts(&mut self, now: Micros, number: u8, seen: &Seen, out: &mut Vec<Action>) {
        if number <= 1 {
            return;
        }
        let stage = self
            .state
            .notes
            .iter()
            .rev()
            .find(|n| n.outcome == Outcome::Retried)
            .map(|n| n.step.stage())
            .unwrap_or(LoginStage::LoginScreenProgram);
        let waited = seconds(now.saturating_sub(self.state.t0));
        out.push(Action::Record(Box::new(RecordDraft {
            waited_seconds: waited,
            evidence: seen.evidence(&self.state.notes),
            restart: RestartOutcome::LoginScreenRestarted {
                stage,
                restarts: u32::from(number - 1),
                after_seconds: waited,
            },
        })));
    }

    fn ready(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        if seen.person_certain() {
            return self.enter_session(now, None, seen, out);
        }
        if self.driver_bound_late(seen) {
            return self.restart_appearance(now, Step::DriverLate, out);
        }
        let Phase::Ready { since } = self.state.phase else {
            return;
        };
        let last = self
            .state
            .heartbeat
            .map_or(since, |b| b.changed_at.max(since));
        if now.saturating_sub(last) > limits::HEARTBEAT_STALE && !seen.person_possible() {
            self.restart_appearance(now, Step::Greeter, out);
        }
    }

    fn no_screen(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        if seen.person_certain() {
            return self.enter_session(now, None, seen, out);
        }
        // A greeter with no output to draw on may say it is ready; only a
        // screen ends the wait.
        if Cards::of(seen.drm.as_ref()) == Cards::RealWithScreen {
            // A screen was connected: a new T0, and the first attempt's
            // deadlines from it. Starting the unit again is a no-op while
            // it runs.
            self.note(Step::Device, Outcome::Ok, Reason::ScreenConnected, out);
            self.state.t0 = now;
            self.begin_attempt(now, 1, out);
        }
    }

    fn session(&mut self, now: Micros, seen: &Seen) {
        if seen.session_ended() {
            self.new_appearance(now);
        }
    }

    fn fall_back(&mut self, now: Micros, step: Step, reason: Reason, out: &mut Vec<Action>) {
        self.note(step, Outcome::FellBack, reason, out);
        out.push(Action::StopUnit(Unit::Greeter));
        // At the same moment as the stop, so the two share its 6 s.
        self.plymouth(now, PlymouthCommand::Quit, out);
        self.state.phase = Phase::FallingBack {
            step,
            deadline: now + limits::STOP,
        };
    }

    fn falling_back(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::FallingBack { step, deadline } = self.state.phase else {
            return;
        };
        let stopped = seen.greeter_stopped();
        if stopped && self.plymouth_settled() {
            self.complete(now, seen, step, Tty::Tty1, out);
        } else if now >= deadline {
            if self.state.plymouth.job.is_some() {
                out.push(Action::KillPlymouth);
                self.state.plymouth.job = None;
                self.state.plymouth.quit = true;
            }
            let tty = if stopped {
                Tty::Tty1
            } else {
                self.note(step, Outcome::FellBack, Reason::StopDidNotComplete, out);
                Tty::Tty2
            };
            self.complete(now, seen, step, tty, out);
        }
    }

    /// The text login: the message, the terminals back to text, the marker
    /// before the start, the switch, the record.
    fn complete(&mut self, now: Micros, seen: &Seen, step: Step, tty: Tty, out: &mut Vec<Action>) {
        let stage = step.stage();
        out.push(Action::WriteIssue(stage));
        // tty1 always: a compositor or plymouthd killed there may have left
        // it in graphics mode, and in VT_PROCESS mode no switch away from it
        // completes.
        out.push(Action::ResetTty(Tty::Tty1));
        if tty == Tty::Tty2 {
            out.push(Action::ResetTty(Tty::Tty2));
        }
        out.push(Action::MarkFallbackActive(tty));
        out.push(Action::MarkHeld);
        out.push(Action::StartUnit(Unit::Fallback(tty)));
        out.push(Action::ActivateVt(tty));
        let waited = seconds(now.saturating_sub(self.state.t0));
        out.push(Action::Record(Box::new(RecordDraft {
            waited_seconds: waited,
            evidence: seen.evidence(&self.state.notes),
            restart: RestartOutcome::TextLoginUsed {
                stage,
                tty: tty.name().to_string(),
                after_seconds: waited,
            },
        })));
        self.state.phase = Phase::FellBack { step, tty, at: now };
    }
}

fn mode(number: u8) -> GreeterMode {
    if number >= limits::ATTEMPTS {
        GreeterMode::Safe
    } else {
        GreeterMode::Plain
    }
}

fn seconds(micros: Micros) -> u32 {
    u32::try_from(micros / SECOND).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;

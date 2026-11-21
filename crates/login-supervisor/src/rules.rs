//! The login supervisor's rules (design/login-screen-spec.md section 5.10),
//! with no I/O: what the supervisor saw goes in as [`Seen`], what it does
//! comes out as [`Action`]s, what it needs looked at next is [`Needs`], and
//! everything it must remember across its own restarts is the [`State`]
//! written to `/run/better-os/login-path.json`.
//!
//! One appearance of the login screen runs: stages 1 to 3 (a graphics
//! device with a screen, `seat0` graphical, logind answering) within 12 s of
//! T0, `plymouth deactivate` inside them, then up to three attempts at the
//! greeter of at most 15 s each (the third in safe mode) with a stop of at
//! most 6 s after each miss, then the fall back to a text login on a
//! terminal of its own. However the time went, the fall back begins by
//! T0 + 68 s. Every deadline is counted from times kept in the state, so a
//! supervisor restarted in the middle continues the same appearance with
//! the same clock.

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
    /// Into a stop: the greeter's session scope, which systemd's stop of
    /// greetd's unit never reaches, is killed with `SIGKILL`, as systemd
    /// kills greetd at its `TimeoutStopSec=4`.
    pub const KILL_SESSION: Micros = 4 * SECOND;
    /// The text login accepting input once started, and how long a start of
    /// it has to be seen active before the other terminal is tried.
    pub const TEXT_LOGIN: Micros = 3 * SECOND;
    /// A start of the text login not yet active is asked for again then.
    pub const TEXT_LOGIN_AGAIN: Micros = 3 * SECOND / 2;
    /// Every call to logind or systemd.
    pub const CALL: Micros = 2 * SECOND;
    /// However the time went (slow calls, a restart of the supervisor), the
    /// fall back begins by then, counted from T0.
    pub const FALL_BACK_BY: Micros = 68 * SECOND;
    /// The property.
    pub const BUDGET: Micros = 80 * SECOND;
    /// The backstop's timer, from the supervisor's first start; with its
    /// own bounded steps (at most 8 s) it gives a text login by 90 s.
    pub const BACKSTOP: Micros = 82 * SECOND;
    /// After `ready`, the heartbeat is never older than this while no
    /// person's session runs.
    pub const HEARTBEAT_STALE: Micros = 30 * SECOND;
    /// A stale heartbeat while greetd's sessions cannot be told apart: the
    /// text login after this much more, without stopping anything.
    pub const UNSURE_GRACE: Micros = 30 * SECOND;

    /// The worst case written out: the fall back begins a tick after
    /// [`FALL_BACK_BY`], its stop takes [`STOP`] and is noticed a tick late,
    /// the start of the text login takes a whole [`CALL`], and the text
    /// login then takes [`TEXT_LOGIN`].
    pub const WORST_CASE: Micros = FALL_BACK_BY + TICK + STOP + TICK + CALL + TEXT_LOGIN;
    const _: () = assert!(WORST_CASE <= BUDGET);
    /// The ordinary sequence fits before the cut-off.
    const _: () =
        assert!(STAGES + ATTEMPTS as Micros * STAGE_7 + 2 * STOP <= FALL_BACK_BY + STAGE_7);
    const _: () = assert!(PLYMOUTH_LATEST_SEND + PLYMOUTH <= STAGES);
    const _: () = assert!(PLYMOUTH < STOP);
    const _: () = assert!(KILL_SESSION < STOP);
}

/// The record format of `login-path.json`. A file of another version is
/// unreadable, never guessed at.
pub const STATE_VERSION: u32 = 2;

/// The terminals the text login runs on. Outside logind's automatic VTs
/// (`NAutoVTs=6`): logind starts `autovt@ttyN` (`getty@ttyN`) when the
/// screen switches to a VT of 1 to 6 with nobody on it, which would stop a
/// text login there through its `Conflicts=` and lose the message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTty {
    Tty8,
    Tty9,
}

impl TextTty {
    pub fn other(self) -> TextTty {
        match self {
            TextTty::Tty8 => TextTty::Tty9,
            TextTty::Tty9 => TextTty::Tty8,
        }
    }

    pub fn name(self) -> &'static str {
        self.vt().name()
    }

    pub fn vt(self) -> Vt {
        match self {
            TextTty::Tty8 => Vt::Tty8,
            TextTty::Tty9 => Vt::Tty9,
        }
    }
}

/// Every terminal the supervisor touches: tty1, where the greeter and
/// Plymouth were, given back to text; the two text login terminals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Vt {
    Tty1,
    Tty8,
    Tty9,
}

impl Vt {
    pub fn number(self) -> u8 {
        match self {
            Vt::Tty1 => 1,
            Vt::Tty8 => 8,
            Vt::Tty9 => 9,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Vt::Tty1 => "tty1",
            Vt::Tty8 => "tty8",
            Vt::Tty9 => "tty9",
        }
    }

    pub fn device(self) -> &'static str {
        match self {
            Vt::Tty1 => "/dev/tty1",
            Vt::Tty8 => "/dev/tty8",
            Vt::Tty9 => "/dev/tty9",
        }
    }
}

/// The units the supervisor may start or stop: a closed set, so no input
/// can name another (the security review's fixed list).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Greeter,
    Fallback(TextTty),
}

/// Every unit name [`Unit`] can produce.
pub const UNITS: [&str; 3] = [
    "better-os-greeter.service",
    "better-os-login-fallback@tty8.service",
    "better-os-login-fallback@tty9.service",
];

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Greeter => UNITS[0],
            Unit::Fallback(TextTty::Tty8) => UNITS[1],
            Unit::Fallback(TextTty::Tty9) => UNITS[2],
        }
    }
}

/// The one shape of a session scope's cgroup the supervisor may kill:
/// `/user.slice/user-<uid>.slice/session-<id>.scope`, the one logind gives a
/// session. Anything else is never acted on.
pub fn is_session_scope(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/user.slice/user-") else {
        return false;
    };
    let Some((uid, rest)) = rest.split_once(".slice/session-") else {
        return false;
    };
    let Some(id) = rest.strip_suffix(".scope") else {
        return false;
    };
    !uid.is_empty()
        && uid.bytes().all(|b| b.is_ascii_digit())
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_alphanumeric())
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
    /// F, the text login itself.
    TextLogin,
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
            Step::TextLogin => "F",
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
            Step::DriverLate
            | Step::Seat
            | Step::Plymouth
            | Step::Greeter
            | Step::SessionEnd
            | Step::TextLogin => LoginStage::LoginScreenProgram,
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
    SessionKilled,
    StopDidNotComplete,
    OutOfTime,
    SessionsUnknown,
    TextLoginNotActive,
    StateUnreadable,
    Restarted,
    StartedAgain,
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
            Reason::SessionKilled => "session-killed",
            Reason::StopDidNotComplete => "stop-did-not-complete",
            Reason::OutOfTime => "out-of-time",
            Reason::SessionsUnknown => "sessions-unknown",
            Reason::TextLoginNotActive => "text-login-not-active",
            Reason::StateUnreadable => "state-unreadable",
            Reason::Restarted => "restarted",
            Reason::StartedAgain => "started-again",
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

/// The most notes one appearance keeps; the journal has every line.
const MAX_NOTES: usize = 64;
/// The most session scopes one appearance remembers.
const MAX_SCOPES: usize = 8;

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
    /// `SIGKILL` to every process of a greeter session's scope (a path
    /// [`is_session_scope`] accepts) that runs as root or as the greeter's
    /// account `greeter`: never a person's.
    KillScope {
        path: String,
        greeter: u32,
    },
    SetGreeterMode(GreeterMode),
    /// `KDSETMODE KD_TEXT` and `VT_SETMODE VT_AUTO`.
    ResetVt(Vt),
    /// `VT_ACTIVATE`, which is what `chvt` does.
    ActivateVt(TextTty),
    /// `/run/better-os/login-fallback.issue`, the stage's English name and
    /// the time filled in.
    WriteIssue(LoginStage),
    /// `/run/better-os/login-fallback.active`, once the text login is seen
    /// active: the backstop never starts a second one.
    MarkFallbackActive(TextTty),
    /// The text login is given up for the greeter again (a screen was
    /// connected after a fall back on a machine that had none).
    ClearFallbackActive,
    /// `/run/better-os/login-held`: the property held this boot (the
    /// backstop timer's third condition).
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

/// What the supervisor needs looked at beyond the usual look.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Needs {
    /// The greeter session scopes it remembers.
    pub scopes: Vec<String>,
    /// Who holds a `/dev/dri/card*` open: only while a stop runs.
    pub drm_holders: bool,
    /// Whether this text login unit is active.
    pub text_login: Option<TextTty>,
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
    /// A card of a GPU's own driver with a screen.
    RealWithScreen,
    /// Only the firmware's framebuffer shows a screen (a real card without
    /// one may sit beside it): it is used.
    FirmwareWithScreen,
    /// Cards, and none of them with a screen.
    NoScreen,
    None,
}

impl Cards {
    pub fn of(drm: Option<&Drm>) -> Cards {
        let Some(drm) = drm else {
            return Cards::None;
        };
        if drm.cards.iter().any(|c| !c.firmware && c.screen) {
            Cards::RealWithScreen
        } else if drm.cards.iter().any(|c| c.screen) {
            Cards::FirmwareWithScreen
        } else if drm.cards.is_empty() {
            Cards::None
        } else {
            Cards::NoScreen
        }
    }

    fn screen(self) -> bool {
        matches!(self, Cards::RealWithScreen | Cards::FirmwareWithScreen)
    }
}

/// One session from logind.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogindSession {
    pub id: String,
    pub uid: u32,
    pub class: String,
    /// The PAM service: `greetd` for a person's session greetd started,
    /// `greetd-greeter` for the greeter's.
    pub service: String,
    pub seat: String,
    pub kind: String,
    /// Its scope unit, `session-<id>.scope`.
    pub scope: String,
}

impl LogindSession {
    /// A person's session that greetd runs on the first seat.
    pub fn is_greetd_person(&self) -> bool {
        self.is_person() && self.service == "greetd" && self.seat == "seat0"
    }

    /// A person's session of any kind (a text login included).
    pub fn is_person(&self) -> bool {
        self.class.starts_with("user")
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

/// One process of greetd's tree or of a session scope.
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

/// A process of an account other than root and the greeter's.
fn foreign(processes: &[Process], greeter: Option<u32>) -> bool {
    processes
        .iter()
        .any(|p| p.uid != 0 && Some(p.uid) != greeter)
}

impl Greetd {
    /// The unit's own cgroup is empty.
    pub fn unit_empty(&self) -> bool {
        !self.processes.iter().any(|p| p.in_cgroup)
    }

    /// A person's session, certainly when the greeter's account is known.
    fn foreign(&self, greeter: Option<u32>) -> bool {
        foreign(&self.processes, greeter)
    }
}

/// A session scope the edges found: through a logind session of class
/// `greeter`, through a process of greetd's tree, or because the
/// supervisor remembered it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeSeen {
    pub path: String,
    /// logind lists it as a greeter session's scope.
    pub greeter_session: bool,
    /// `None`: there and unreadable. A scope that is gone is empty.
    pub processes: Option<Vec<Process>>,
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
    /// Every session scope found or asked about.
    pub scopes: Vec<ScopeSeen>,
    /// The processes holding a `/dev/dri/card*` open, `plymouthd` left out
    /// (its own path quits or kills it); `None` when not asked for or
    /// unreadable.
    pub drm_holders: Option<Vec<u32>>,
    /// Whether the text login asked about is active; `None` when not asked
    /// for or unknown.
    pub text_login_active: Option<bool>,
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
    /// stopped, because greetd ends the session it runs when it is.
    pub fn person_possible(&self) -> bool {
        match &self.greetd {
            Some(tree) => tree.foreign(self.greeter()),
            None => self.person_by_logind() || self.logind.is_none(),
        }
    }

    /// Neither greetd's tree nor logind can be read: whether a person's
    /// session runs cannot be known, so nothing that would end one is done.
    pub fn sessions_unknown(&self) -> bool {
        self.greetd.is_none() && self.logind.is_none()
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

    /// Nobody is signed in on the seat at all, text logins included, as
    /// far as can be known.
    fn nobody_signed_in(&self) -> bool {
        self.logind
            .as_ref()
            .is_some_and(|l| !l.sessions.iter().any(LogindSession::is_person))
            && self
                .greetd
                .as_ref()
                .is_some_and(|tree| !tree.foreign(self.greeter()))
    }

    /// Stage 5: the greeter's session is open. With logind answering, a
    /// session of class `greeter` (of `_better-greeter` when its uid is
    /// known); without, a session scope of the greeter's with a process
    /// in it, or a second process under greetd.
    fn greeter_session_open(&self) -> bool {
        match &self.logind {
            Some(logind) => {
                let greeter = self.greeter_uid;
                logind
                    .sessions
                    .iter()
                    .any(|s| s.is_greeter() && greeter.is_none_or(|uid| uid == s.uid))
            }
            None => {
                self.greetd
                    .as_ref()
                    .is_some_and(|tree| tree.processes.len() >= 2)
                    || self.scopes.iter().any(|s| {
                        s.processes.as_ref().is_some_and(|p| !p.is_empty())
                            && self.is_greeter_scope(s)
                    })
            }
        }
    }

    /// A scope that is the greeter's session: logind says so, or every
    /// process in it is root or the greeter's and one is the greeter's.
    fn is_greeter_scope(&self, scope: &ScopeSeen) -> bool {
        if !is_session_scope(&scope.path) {
            return false;
        }
        let greeter = self.greeter();
        match &scope.processes {
            Some(processes) if !processes.is_empty() => {
                !foreign(processes, greeter)
                    && (scope.greeter_session
                        || greeter.is_some_and(|g| processes.iter().any(|p| p.uid == g)))
            }
            _ => scope.greeter_session,
        }
    }

    fn scope(&self, path: &str) -> Option<&ScopeSeen> {
        self.scopes.iter().find(|s| s.path == path)
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
    /// The greeter's unit, and its session's scope, are being stopped.
    Stopping {
        then: Then,
        issued: Micros,
        step: Step,
    },
    /// The greeter accepts input.
    Ready { since: Micros },
    /// A person's session runs.
    Session { since: Micros },
    /// The fall back: the stop and `plymouth quit`, at most 6 s.
    FallingBack { step: Step, issued: Micros },
    /// The text login: started on `tty`, `confirmed` once seen active.
    TextLogin {
        step: Step,
        tty: TextTty,
        started: Micros,
        starts: u8,
        confirmed: bool,
    },
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
    /// No card showed a screen at stage 1 this appearance.
    pub no_screen: bool,
    pub plymouth: PlymouthState,
    pub heartbeat: Option<Beat>,
    /// The greeter session scopes seen this appearance, so that a stop
    /// reaches them after greetd is gone.
    pub scopes: Vec<String>,
    /// systemd's `NRestarts` for the unit when this state was last taken
    /// up: a start with more is the supervisor's own restart, which
    /// continues; any other start of the display manager is a new
    /// appearance.
    pub unit_restarts: Option<u32>,
    /// Since when a stale heartbeat has been seen while greetd's sessions
    /// could not be told apart.
    pub unsure_since: Option<Micros>,
}

/// What reading the state file gave.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Loaded {
    Absent,
    Unreadable,
    Found(Box<State>),
}

/// This start of the unit, as systemd reports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnitStart {
    /// `InactiveExitTimestampMonotonic`: when this start began.
    pub began: Option<Micros>,
    /// `NRestarts`.
    pub restarts: Option<u32>,
}

impl State {
    /// A new boot's state: T0 is when the unit began to start this boot
    /// (`InactiveExitTimestampMonotonic`), or now when that is unknown or
    /// later than now.
    pub fn fresh(boot_id: Option<String>, unit: UnitStart, now: Micros) -> State {
        State {
            version: STATE_VERSION,
            boot_id,
            appearance: 1,
            t0: t0_of(unit, now),
            phase: Phase::Checking { baseline: None },
            notes: Vec::new(),
            on_firmware: false,
            no_screen: false,
            plymouth: PlymouthState::default(),
            heartbeat: None,
            scopes: Vec::new(),
            unit_restarts: unit.restarts,
            unsure_since: None,
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
            Phase::Stopping { step, .. }
            | Phase::FallingBack { step, .. }
            | Phase::TextLogin { step, .. } => Some(step),
            Phase::Ready { .. } | Phase::Session { .. } => None,
            Phase::Unreadable => Some(Step::Supervisor),
        }
    }
}

fn t0_of(unit: UnitStart, now: Micros) -> Micros {
    unit.began.filter(|t| *t > 0 && *t <= now).unwrap_or(now)
}

/// The machine: the state and the rules over it.
#[derive(Clone, Debug)]
pub struct Machine {
    pub state: State,
}

/// How a start continued: the boot's first, the supervisor's own restart in
/// the middle of an appearance, a display manager stopped and started
/// again (a new appearance), or a state that could not be read.
pub fn start(
    loaded: Loaded,
    boot_id: Option<String>,
    unit: UnitStart,
    now: Micros,
) -> (Machine, Vec<Action>) {
    match loaded {
        Loaded::Found(state)
            if state.version == STATE_VERSION
                && (state.boot_id.is_none() || boot_id.is_none() || state.boot_id == boot_id) =>
        {
            let mut machine = Machine { state: *state };
            // systemd counts its automatic restarts; a start it did not
            // count is someone starting the display manager again.
            let own_restart = match (unit.restarts, machine.state.unit_restarts) {
                (Some(now), Some(before)) => now > before,
                // Unknown: continue, which keeps the earlier deadlines.
                _ => true,
            };
            machine.state.unit_restarts = unit.restarts.or(machine.state.unit_restarts);
            let actions = if own_restart {
                machine.resumed()
            } else {
                machine.started_again(t0_of(unit, now))
            };
            (machine, actions)
        }
        Loaded::Found(_) | Loaded::Absent => (
            Machine {
                state: State::fresh(boot_id, unit, now),
            },
            Vec::new(),
        ),
        Loaded::Unreadable => {
            let mut state = State::fresh(boot_id, unit, now);
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
        let mut actions = Vec::new();
        self.note(
            Step::Supervisor,
            Outcome::Ok,
            Reason::Restarted,
            &mut actions,
        );
        match self.state.phase {
            Phase::Stopping { .. } | Phase::FallingBack { .. } => {
                actions.push(Action::StopUnit(Unit::Greeter));
            }
            Phase::Attempt { number, .. } => {
                actions.push(Action::SetGreeterMode(mode(number)));
                actions.push(Action::StartUnit(Unit::Greeter));
            }
            Phase::TextLogin {
                tty,
                confirmed: false,
                ..
            } => actions.push(Action::StartUnit(Unit::Fallback(tty))),
            _ => {}
        }
        actions
    }

    /// The display manager was stopped and started again this boot: a new
    /// appearance from stage 1. What Plymouth did stays done.
    fn started_again(&mut self, t0: Micros) -> Vec<Action> {
        let mut actions = Vec::new();
        self.new_appearance(t0);
        self.note(
            Step::Supervisor,
            Outcome::Ok,
            Reason::StartedAgain,
            &mut actions,
        );
        actions
    }

    /// What to look at beyond the usual look.
    pub fn needs(&self) -> Needs {
        Needs {
            scopes: self.state.scopes.clone(),
            drm_holders: matches!(
                self.state.phase,
                Phase::Stopping { .. } | Phase::FallingBack { .. }
            ),
            text_login: match self.state.phase {
                Phase::TextLogin {
                    tty,
                    confirmed: false,
                    ..
                } => Some(tty),
                _ => None,
            },
        }
    }

    /// One look: what to do now.
    pub fn step(&mut self, now: Micros, seen: &Seen) -> Vec<Action> {
        let mut out = Vec::new();
        self.track_heartbeat(now, seen);
        self.track_scopes(seen);
        self.track_plymouth(now, seen, &mut out);
        if self.out_of_time(now, seen, &mut out) {
            return out;
        }
        match self.state.phase {
            Phase::Unreadable => self.unreadable(now, seen, &mut out),
            Phase::Checking { .. } => self.checking(now, seen, &mut out),
            Phase::Attempt { .. } => self.attempt(now, seen, &mut out),
            Phase::Stopping { .. } => self.stopping(now, seen, &mut out),
            Phase::Ready { .. } => self.ready(now, seen, &mut out),
            Phase::Session { .. } => self.session(now, seen),
            Phase::FallingBack { .. } => self.falling_back(now, seen, &mut out),
            Phase::TextLogin { .. } => self.text_login(now, seen, &mut out),
        }
        out
    }

    fn note(&mut self, step: Step, outcome: Outcome, reason: Reason, out: &mut Vec<Action>) {
        let note = Note {
            step,
            outcome,
            reason,
        };
        out.push(Action::Journal(note.journal_line()));
        if self.state.notes.len() < MAX_NOTES {
            self.state.notes.push(note);
        }
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

    /// Every greeter session scope seen is remembered for the appearance:
    /// once greetd is gone, the scope is no longer found through its tree.
    fn track_scopes(&mut self, seen: &Seen) {
        for scope in &seen.scopes {
            if self.state.scopes.len() < MAX_SCOPES
                && !self.state.scopes.contains(&scope.path)
                && scope.processes.as_ref().is_some_and(|p| !p.is_empty())
                && seen.is_greeter_scope(scope)
            {
                self.state.scopes.push(scope.path.clone());
            }
        }
    }

    /// The greeter's unit and every remembered session scope have no
    /// process, and no process holds a card. Unknown is not gone.
    fn greeter_gone(&self, seen: &Seen) -> bool {
        seen.greetd.as_ref().is_some_and(Greetd::unit_empty)
            && self.state.scopes.iter().all(|path| {
                seen.scope(path)
                    .and_then(|s| s.processes.as_ref())
                    .is_some_and(|p| p.is_empty())
            })
            && seen.drm_holders.as_ref().is_some_and(|h| h.is_empty())
    }

    /// `SIGKILL` to the remembered greeter session scopes that still have
    /// a process, never one with a process of a person's.
    fn kill_scopes(&mut self, seen: &Seen, step: Step, out: &mut Vec<Action>) {
        let Some(greeter) = seen.greeter() else {
            return;
        };
        let mut killed = false;
        for path in &self.state.scopes {
            let Some(processes) = seen.scope(path).and_then(|s| s.processes.as_ref()) else {
                continue;
            };
            if !processes.is_empty() && !foreign(processes, Some(greeter)) {
                out.push(Action::KillScope {
                    path: path.clone(),
                    greeter,
                });
                killed = true;
            }
        }
        let said = self
            .state
            .notes
            .last()
            .is_some_and(|n| n.reason == Reason::SessionKilled);
        if killed && !said {
            self.note(step, Outcome::Ok, Reason::SessionKilled, out);
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

    /// The cut-off: by T0 + 68 s the fall back has begun, however the time
    /// went before (slow calls, the supervisor's own restarts).
    fn out_of_time(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) -> bool {
        if now < self.state.t0 + limits::FALL_BACK_BY {
            return false;
        }
        let step = self.state.current_step().unwrap_or(Step::Greeter);
        match self.state.phase {
            Phase::Checking { .. } | Phase::Attempt { .. } => {}
            Phase::Stopping {
                then: Then::Attempt { .. },
                issued,
                step,
            } => {
                // The stop goes on, and the text login follows it.
                self.note(step, Outcome::FellBack, Reason::OutOfTime, out);
                self.plymouth(now, PlymouthCommand::Quit, out);
                self.state.phase = Phase::FallingBack { step, issued };
                return false;
            }
            _ => return false,
        }
        if seen.person_certain() {
            self.enter_session(now, None, seen, out);
            return true;
        }
        self.fall_back(now, step, Reason::OutOfTime, seen, out);
        true
    }

    fn unreadable(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        if seen.person_possible() && !seen.sessions_unknown() {
            self.enter_session(now, None, seen, out);
        } else if seen.heartbeat.is_some_and(|h| h.word == Word::Ready) {
            // The greeter says it is ready: taken as such, and held to its
            // heartbeat from now on like any ready greeter.
            out.push(Action::MarkHeld);
            self.state.phase = Phase::Ready { since: now };
        } else {
            self.fall_back(now, Step::Supervisor, Reason::StateUnreadable, seen, out);
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
                return self.fall_back(now, Step::Device, Reason::NoGraphicsDevice, seen, out);
            }
            let Some(logind) = &seen.logind else {
                return self.fall_back(now, Step::Logind, Reason::LogindNoAnswer, seen, out);
            };
            let can_graphical = logind.can_graphical;
            match cards {
                Cards::FirmwareWithScreen => self.note(
                    Step::Device,
                    Outcome::Ok,
                    Reason::FirmwareFramebufferOnly,
                    out,
                ),
                // The attempts run as on any machine: a greeter with no
                // screen may still say it is ready, and otherwise the text
                // login waits for the screen to be connected.
                Cards::NoScreen => self.note(Step::Device, Outcome::Ok, Reason::NoScreen, out),
                Cards::RealWithScreen | Cards::None => {}
            }
            if !can_graphical {
                self.note(Step::Seat, Outcome::Ok, Reason::SeatNotGraphical, out);
            }
        }
        self.state.on_firmware = cards == Cards::FirmwareWithScreen;
        self.state.no_screen = cards == Cards::NoScreen;
        self.begin_attempt(now, 1, out);
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
            return self.restart_appearance(now, Step::DriverLate, seen, out);
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

    fn restart_appearance(&mut self, now: Micros, step: Step, seen: &Seen, out: &mut Vec<Action>) {
        let (outcome, reason) = match step {
            Step::DriverLate => (Outcome::Ok, Reason::RealDriverBound),
            _ => (Outcome::Retried, Reason::HeartbeatStale),
        };
        if seen.sessions_unknown() {
            return self.complete(
                now,
                Step::Greeter,
                Reason::SessionsUnknown,
                TextTty::Tty8,
                out,
            );
        }
        self.note(step, outcome, reason, out);
        out.push(Action::StopUnit(Unit::Greeter));
        self.state.phase = Phase::Stopping {
            then: Then::NewAppearance { t0: now },
            issued: now,
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
        // Neither greetd's tree nor logind readable: the unit may hold a
        // session that cannot be seen, so it is never stopped; the text
        // login comes on its own terminal.
        if seen.sessions_unknown() {
            return self.complete(now, step, Reason::SessionsUnknown, TextTty::Tty8, out);
        }
        // greetd ends the session it runs when it is stopped: with a
        // person's session possible, the attempt is held rather than ended
        // (until the cut-off, which starts the text login without a stop).
        if seen.person_possible() {
            return;
        }
        if number < limits::ATTEMPTS {
            self.note(step, Outcome::Retried, reason, out);
            out.push(Action::StopUnit(Unit::Greeter));
            self.state.phase = Phase::Stopping {
                then: Then::Attempt { number: number + 1 },
                issued: now,
                step,
            };
        } else {
            self.fall_back(now, step, reason, seen, out);
        }
    }

    fn stopping(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::Stopping { then, issued, step } = self.state.phase else {
            return;
        };
        let elapsed = now.saturating_sub(issued);
        if elapsed >= limits::KILL_SESSION {
            self.kill_scopes(seen, step, out);
        }
        if self.greeter_gone(seen) {
            self.state.scopes.clear();
            match then {
                Then::Attempt { number } => self.begin_attempt(now, number, out),
                Then::NewAppearance { t0 } => self.new_appearance(t0),
            }
        } else if elapsed >= limits::STOP {
            // A process the kernel cannot kill keeps the device: no start
            // of the greeter can follow, so the text login starts at once.
            self.note(step, Outcome::FellBack, Reason::StopDidNotComplete, out);
            self.complete(now, step, Reason::StopDidNotComplete, TextTty::Tty8, out);
        }
    }

    fn new_appearance(&mut self, t0: Micros) {
        self.state.appearance += 1;
        self.state.t0 = t0;
        self.state.notes.clear();
        self.state.on_firmware = false;
        self.state.no_screen = false;
        self.state.unsure_since = None;
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
            self.state.unsure_since = None;
            return self.enter_session(now, None, seen, out);
        }
        if self.driver_bound_late(seen) {
            return self.restart_appearance(now, Step::DriverLate, seen, out);
        }
        let Phase::Ready { since } = self.state.phase else {
            return;
        };
        let last = self
            .state
            .heartbeat
            .map_or(since, |b| b.changed_at.max(since));
        if now.saturating_sub(last) <= limits::HEARTBEAT_STALE {
            self.state.unsure_since = None;
            return;
        }
        if !seen.person_possible() {
            return self.restart_appearance(now, Step::Greeter, seen, out);
        }
        // Stale, and greetd's sessions cannot be told apart: after a grace,
        // the text login on its own terminal, nothing stopped.
        let since = *self.state.unsure_since.get_or_insert(now);
        if now.saturating_sub(since) >= limits::UNSURE_GRACE {
            self.complete(
                now,
                Step::Greeter,
                Reason::SessionsUnknown,
                TextTty::Tty8,
                out,
            );
        }
    }

    fn session(&mut self, now: Micros, seen: &Seen) {
        if seen.session_ended() {
            self.new_appearance(now);
        }
    }

    fn fall_back(
        &mut self,
        now: Micros,
        step: Step,
        reason: Reason,
        seen: &Seen,
        out: &mut Vec<Action>,
    ) {
        // A unit that may hold a session nothing can see, or one of a
        // person's that cannot be told from the greeter's, is never
        // stopped: the text login comes on its own terminal beside it.
        if seen.sessions_unknown() || seen.person_possible() {
            let why = if seen.sessions_unknown() {
                Reason::SessionsUnknown
            } else {
                reason
            };
            return self.complete(now, step, why, TextTty::Tty8, out);
        }
        self.note(step, Outcome::FellBack, reason, out);
        out.push(Action::StopUnit(Unit::Greeter));
        // At the same moment as the stop, so the two share its 6 s.
        self.plymouth(now, PlymouthCommand::Quit, out);
        self.state.phase = Phase::FallingBack { step, issued: now };
    }

    fn falling_back(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::FallingBack { step, issued } = self.state.phase else {
            return;
        };
        let elapsed = now.saturating_sub(issued);
        if elapsed >= limits::KILL_SESSION {
            self.kill_scopes(seen, step, out);
        }
        let gone = self.greeter_gone(seen);
        if gone && self.plymouth_settled() {
            self.state.scopes.clear();
            self.start_text_login(now, step, TextTty::Tty8, out);
        } else if elapsed >= limits::STOP {
            if self.state.plymouth.job.is_some() {
                out.push(Action::KillPlymouth);
                self.state.plymouth.job = None;
                self.state.plymouth.quit = true;
            }
            if !gone {
                self.note(step, Outcome::FellBack, Reason::StopDidNotComplete, out);
            }
            self.start_text_login(now, step, TextTty::Tty8, out);
        }
    }

    /// The text login now, without a stop: noted with `reason`.
    fn complete(
        &mut self,
        now: Micros,
        step: Step,
        reason: Reason,
        tty: TextTty,
        out: &mut Vec<Action>,
    ) {
        let said = self
            .state
            .notes
            .last()
            .is_some_and(|n| n.outcome == Outcome::FellBack && n.reason == reason);
        if !said {
            self.note(step, Outcome::FellBack, reason, out);
        }
        self.start_text_login(now, step, tty, out);
    }

    /// The message, the terminals back to text, the start and the switch.
    /// The markers and the record wait until the unit is seen active.
    fn start_text_login(&mut self, now: Micros, step: Step, tty: TextTty, out: &mut Vec<Action>) {
        self.plymouth(now, PlymouthCommand::Quit, out);
        out.push(Action::WriteIssue(step.stage()));
        // tty1 always: a compositor or plymouthd killed there may have left
        // it in graphics mode, and in VT_PROCESS mode no switch away from it
        // completes.
        out.push(Action::ResetVt(Vt::Tty1));
        out.push(Action::ResetVt(tty.vt()));
        out.push(Action::StartUnit(Unit::Fallback(tty)));
        out.push(Action::ActivateVt(tty));
        self.state.phase = Phase::TextLogin {
            step,
            tty,
            started: now,
            starts: 1,
            confirmed: false,
        };
    }

    fn text_login(&mut self, now: Micros, seen: &Seen, out: &mut Vec<Action>) {
        let Phase::TextLogin {
            step,
            tty,
            started,
            starts,
            confirmed,
        } = self.state.phase
        else {
            return;
        };
        if confirmed {
            // A machine that had no screen at stage 1, with nobody signed
            // in: a screen connected now brings the greeter back.
            if self.state.no_screen
                && Cards::of(seen.drm.as_ref()).screen()
                && seen.nobody_signed_in()
            {
                self.note(Step::Device, Outcome::Ok, Reason::ScreenConnected, out);
                out.push(Action::StopUnit(Unit::Fallback(tty)));
                out.push(Action::ClearFallbackActive);
                self.new_appearance(now);
            }
            return;
        }
        if seen.text_login_active == Some(true) {
            out.push(Action::MarkFallbackActive(tty));
            out.push(Action::MarkHeld);
            let waited = seconds(now.saturating_sub(self.state.t0));
            out.push(Action::Record(Box::new(RecordDraft {
                waited_seconds: waited,
                evidence: seen.evidence(&self.state.notes),
                restart: RestartOutcome::TextLoginUsed {
                    stage: step.stage(),
                    tty: tty.name().to_string(),
                    after_seconds: waited,
                },
            })));
            self.state.phase = Phase::TextLogin {
                step,
                tty,
                started,
                starts,
                confirmed: true,
            };
            return;
        }
        let elapsed = now.saturating_sub(started);
        let tries = self
            .state
            .notes
            .iter()
            .filter(|n| n.reason == Reason::TextLoginNotActive)
            .count();
        // After two rounds on both terminals, every 30 s: the journal is not
        // flooded while systemd refuses them.
        let wait = if tries >= 4 {
            limits::HEARTBEAT_STALE
        } else {
            limits::TEXT_LOGIN
        };
        if elapsed >= wait {
            // Not active on this terminal: the other one, and so on,
            // without a marker, so the backstop stays armed.
            self.note(
                Step::TextLogin,
                Outcome::Retried,
                Reason::TextLoginNotActive,
                out,
            );
            self.start_text_login(now, step, tty.other(), out);
        } else if elapsed >= limits::TEXT_LOGIN_AGAIN && starts == 1 {
            out.push(Action::StartUnit(Unit::Fallback(tty)));
            self.state.phase = Phase::TextLogin {
                step,
                tty,
                started,
                starts: 2,
                confirmed,
            };
        }
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

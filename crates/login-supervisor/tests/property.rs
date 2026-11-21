//! The property (design/login-screen-spec.md section 5.10) on a fake
//! clock, for every injected outcome: within 80 seconds of T0 the login
//! screen accepts input, or a text login runs on the terminal that is on
//! screen with Plymouth gone and a journal line for every stage that did
//! not pass. The fake world stands in for logind, systemd, `/sys/class/drm`,
//! Plymouth, the greeter's cgroup and the clock; the supervisor's own loop
//! (`world::Supervisor`) runs unchanged over it, restarts included.

use login_supervisor::rules::{
    Action, Card, Drm, Greetd, GreeterMode, HeartbeatFile, Job, Loaded, Logind, LogindSession,
    Micros, Phase, PlymouthCommand, Process, SECOND, Seen, State, Tty, UNITS, Unit, Word, limits,
};
use login_supervisor::world::{Supervisor, World};
use manager_ipc::boot_watch::{LoginStage, RestartOutcome};

const GREETER_UID: u32 = 118;
const PERSON_UID: u32 = 1000;
/// The supervisor's unit begins to start 5 s after boot: T0.
const T0: Micros = 5 * SECOND;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cards {
    None,
    Firmware,
    RealNoScreen,
    Real,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    /// Gone half a second after the stop.
    Clean,
    /// Ignores `SIGTERM`: systemd's `SIGKILL` after `TimeoutStopSec=4`.
    IgnoresTerm,
    /// Gone just inside the 6 s a stop may take.
    AtTheLimit,
    /// In uninterruptible sleep: never gone.
    Never,
}

/// What one start of the greeter's unit does, from its start.
#[derive(Clone, Copy, Debug)]
struct Greeter {
    session: Option<Micros>,
    heartbeat: Option<Micros>,
    ready: Option<Micros>,
    /// The heartbeat stops changing this long after `ready`.
    hangs_after_ready: Option<Micros>,
    stop: Stop,
}

const GOOD: Greeter = Greeter {
    session: Some(SECOND),
    heartbeat: Some(2 * SECOND),
    ready: Some(3 * SECOND),
    hangs_after_ready: None,
    stop: Stop::Clean,
};

#[derive(Clone, Debug)]
struct Scenario {
    cards: Cards,
    /// The cards from this long after T0.
    cards_later: Option<(Micros, Cards)>,
    /// logind answers from this long after T0; `None`: never.
    logind_from: Option<Micros>,
    logind_until: Option<Micros>,
    can_graphical: bool,
    plymouth_hangs: bool,
    /// Only `plymouth quit` hangs.
    quit_hangs: bool,
    account: bool,
    /// One per start of the greeter's unit; the last repeats.
    starts: Vec<Greeter>,
    /// greetd starts a person's session instead of the greeter.
    automatic_login: bool,
    /// A person logs in this long after T0 (once the greeter is ready), and
    /// logs out at the second time.
    person: Option<(Micros, Option<Micros>)>,
    fallback_tty1_refused: bool,
    /// After the person's logout greetd's first start shows nothing.
    nothing_after_logout: bool,
}

impl Scenario {
    fn good() -> Scenario {
        Scenario {
            cards: Cards::Real,
            cards_later: None,
            logind_from: Some(0),
            logind_until: None,
            can_graphical: true,
            plymouth_hangs: false,
            quit_hangs: false,
            account: true,
            starts: vec![GOOD],
            automatic_login: false,
            person: None,
            fallback_tty1_refused: false,
            nothing_after_logout: false,
        }
    }

    fn starts(mut self, starts: &[Greeter]) -> Scenario {
        self.starts = starts.to_vec();
        self
    }
}

#[derive(Default)]
struct GreeterUnit {
    running: bool,
    starts: u32,
    started_at: Micros,
    stop_at: Option<Micros>,
    mode: Option<GreeterMode>,
}

/// The fake machine.
struct Fake {
    scenario: Scenario,
    now: Micros,
    greeter: GreeterUnit,
    modes: Vec<GreeterMode>,
    /// The plymouth command running and when it was started.
    plymouth: Option<(PlymouthCommand, Micros)>,
    plymouth_finished: bool,
    plymouthd_alive: bool,
    plymouthd_killed: bool,
    fallbacks: Vec<(Tty, Micros)>,
    active_vt: Option<Tty>,
    resets: Vec<Tty>,
    issue: Option<LoginStage>,
    fallback_marker: Option<(Tty, Micros)>,
    held: bool,
    records: Vec<RestartOutcome>,
    journal: Vec<String>,
    saved: Option<String>,
    units_named: Vec<&'static str>,
    stops_while_person: u32,
    /// When the greeter first accepted input on screen.
    first_ready: Option<Micros>,
    /// Every time it began to.
    readies: Vec<Micros>,
    accepting: bool,
    person_logged_in: bool,
}

impl Fake {
    fn new(scenario: Scenario) -> Fake {
        Fake {
            scenario,
            now: T0,
            greeter: GreeterUnit::default(),
            modes: Vec::new(),
            plymouth: None,
            plymouth_finished: false,
            plymouthd_alive: true,
            plymouthd_killed: false,
            fallbacks: Vec::new(),
            active_vt: None,
            resets: Vec::new(),
            issue: None,
            fallback_marker: None,
            held: false,
            records: Vec::new(),
            journal: Vec::new(),
            saved: None,
            units_named: Vec::new(),
            stops_while_person: 0,
            first_ready: None,
            readies: Vec::new(),
            accepting: false,
            person_logged_in: false,
        }
    }

    fn since_t0(&self) -> Micros {
        self.now - T0
    }

    fn behaviour(&self) -> Greeter {
        let index = (self.greeter.starts.max(1) - 1) as usize;
        self.scenario.starts[index.min(self.scenario.starts.len() - 1)]
    }

    fn cards(&self) -> Cards {
        match self.scenario.cards_later {
            Some((at, cards)) if self.since_t0() >= at => cards,
            _ => self.scenario.cards,
        }
    }

    fn logind_answers(&self) -> bool {
        let t = self.since_t0();
        self.scenario.logind_from.is_some_and(|from| t >= from)
            && self.scenario.logind_until.is_none_or(|until| t < until)
    }

    /// Settles the unit's stop.
    fn settle(&mut self) {
        if let Some(at) = self.greeter.stop_at {
            let gone = match self.behaviour().stop {
                Stop::Clean => Some(at + SECOND / 2),
                Stop::IgnoresTerm => Some(at + 4 * SECOND + SECOND / 5),
                Stop::AtTheLimit => Some(at + 6 * SECOND - SECOND / 10),
                Stop::Never => None,
            };
            if gone.is_some_and(|gone| self.now >= gone) {
                self.greeter.running = false;
                self.greeter.stop_at = None;
            }
        }
        if self.person_session() && !self.person_logged_in {
            self.person_logged_in = true;
        }
    }

    fn up(&self, after: Option<Micros>) -> bool {
        self.greeter.running && after.is_some_and(|a| self.now >= self.greeter.started_at + a)
    }

    fn person_session(&self) -> bool {
        if !self.greeter.running {
            return false;
        }
        if self.scenario.automatic_login && self.greeter.starts == 1 {
            return self.now >= self.greeter.started_at + SECOND;
        }
        match self.scenario.person {
            Some((login, logout)) => {
                self.first_ready.is_some()
                    && self.since_t0() >= login
                    && logout.is_none_or(|out| self.since_t0() < out)
            }
            None => false,
        }
    }

    fn logged_out(&self) -> bool {
        self.scenario
            .person
            .is_some_and(|(_, out)| out.is_some_and(|out| self.since_t0() >= out))
    }

    fn heartbeat(&self) -> Option<HeartbeatFile> {
        // Without its account greetd cannot open the greeter's session.
        if !self.scenario.account
            || (self.scenario.nothing_after_logout && self.logged_out() && self.greeter.starts == 1)
        {
            return None;
        }
        let b = self.behaviour();
        let first = b.heartbeat?;
        if !self.up(Some(first)) || self.person_session() {
            return None;
        }
        let since_first = self.now - (self.greeter.started_at + first);
        let ready_at = b.ready.map(|r| self.greeter.started_at + r);
        let ready = ready_at.is_some_and(|r| self.now >= r);
        // A hung greeter's file stays as it was last written.
        let frozen = match (ready_at, b.hangs_after_ready) {
            (Some(r), Some(h)) if self.now >= r + h => {
                Some(r + h - (self.greeter.started_at + first))
            }
            _ => None,
        };
        let generation = frozen.unwrap_or(since_first) / (5 * SECOND);
        let word = if ready { Word::Ready } else { Word::Starting };
        let word_bit = u64::from(ready);
        Some(HeartbeatFile {
            identity: (u64::from(self.greeter.starts) << 40) | (generation << 1) | word_bit,
            word,
        })
    }

    fn greeter_accepting_input(&self) -> bool {
        self.heartbeat().is_some_and(|h| h.word == Word::Ready)
            && !self.person_session()
            && self.greeter.stop_at.is_none()
            && self.cards() != Cards::RealNoScreen
    }

    fn observe_now(&mut self) -> Seen {
        self.settle();
        let accepting = self.greeter_accepting_input();
        if accepting && !self.accepting {
            self.readies.push(self.now);
            self.first_ready.get_or_insert(self.now);
        }
        self.accepting = accepting;
        let drm = match self.cards() {
            Cards::None => None,
            cards => Some(Drm {
                cards: vec![Card {
                    name: "card0".into(),
                    firmware: cards == Cards::Firmware,
                    screen: cards != Cards::RealNoScreen,
                }],
                entries: vec!["card0".into()],
            }),
        };
        let b = self.behaviour();
        let session_open = self.up(b.session)
            && self.scenario.account
            && !(self.scenario.nothing_after_logout
                && self.logged_out()
                && self.greeter.starts == 1);
        let mut sessions = Vec::new();
        if session_open && !self.person_session() && !self.scenario.automatic_login {
            sessions.push(LogindSession {
                id: "c1".into(),
                uid: GREETER_UID,
                class: "greeter".into(),
                service: "greetd-greeter".into(),
                seat: "seat0".into(),
                kind: "wayland".into(),
            });
        }
        if self.person_session() {
            sessions.push(LogindSession {
                id: "2".into(),
                uid: PERSON_UID,
                class: "user".into(),
                service: "greetd".into(),
                seat: "seat0".into(),
                kind: "wayland".into(),
            });
        }
        let logind = self.logind_answers().then_some(Logind {
            can_graphical: self.scenario.can_graphical,
            sessions,
        });
        let mut processes = Vec::new();
        if self.greeter.running {
            processes.push(Process {
                pid: 100,
                uid: 0,
                in_cgroup: true,
            });
            if self.person_session() {
                processes.push(Process {
                    pid: 300,
                    uid: PERSON_UID,
                    in_cgroup: false,
                });
            } else if session_open && !self.scenario.automatic_login {
                processes.push(Process {
                    pid: 200,
                    uid: GREETER_UID,
                    in_cgroup: true,
                });
            }
        }
        if let Some((command, at)) = self.plymouth
            && !self.scenario.plymouth_hangs
            && !(self.scenario.quit_hangs && command != PlymouthCommand::Deactivate)
            && self.now >= at + SECOND / 10
        {
            self.plymouth = None;
            self.plymouth_finished = true;
            if command != PlymouthCommand::Deactivate {
                self.plymouthd_alive = false;
            }
        }
        Seen {
            drm,
            logind,
            greetd: Some(Greetd { processes }),
            heartbeat: self.heartbeat(),
            plymouth: if self.plymouth.is_some() {
                Some(Job::Running)
            } else if self.plymouth_finished {
                Some(Job::Finished)
            } else {
                None
            },
            greeter_uid: self.scenario.account.then_some(GREETER_UID),
        }
    }

    /// What a restart of the supervisor loses: its children.
    fn forget_children(&mut self) {
        self.plymouth = None;
        self.plymouth_finished = false;
    }
}

impl World for Fake {
    fn now(&mut self) -> Micros {
        self.now
    }

    fn observe(&mut self) -> Seen {
        self.observe_now()
    }

    fn perform(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::StartUnit(unit) => {
                self.units_named.push(unit.name());
                match unit {
                    Unit::Greeter => {
                        if !self.greeter.running {
                            self.greeter.running = true;
                            self.greeter.starts += 1;
                            self.greeter.started_at = self.now;
                            self.greeter.stop_at = None;
                            self.modes
                                .push(self.greeter.mode.unwrap_or(GreeterMode::Plain));
                        }
                    }
                    Unit::Fallback(tty) => {
                        if *tty == Tty::Tty1 && self.scenario.fallback_tty1_refused {
                            return Err("Unit is masked".into());
                        }
                        self.fallbacks.push((*tty, self.now));
                    }
                }
            }
            Action::StopUnit(unit) => {
                self.units_named.push(unit.name());
                if self.person_session() {
                    self.stops_while_person += 1;
                }
                if self.greeter.running && self.greeter.stop_at.is_none() {
                    self.greeter.stop_at = Some(self.now);
                }
            }
            Action::Plymouth(command) => {
                self.plymouth = Some((*command, self.now));
                self.plymouth_finished = false;
            }
            Action::KillPlymouth => {
                self.plymouth = None;
                self.plymouth_finished = true;
                self.plymouthd_alive = false;
                self.plymouthd_killed = true;
            }
            Action::SetGreeterMode(mode) => self.greeter.mode = Some(*mode),
            Action::ResetTty(tty) => self.resets.push(*tty),
            Action::ActivateVt(tty) => self.active_vt = Some(*tty),
            Action::WriteIssue(stage) => self.issue = Some(*stage),
            Action::MarkFallbackActive(tty) => self.fallback_marker = Some((*tty, self.now)),
            Action::MarkHeld => self.held = true,
            Action::Journal(line) => self.journal.push(line.clone()),
            Action::Record(draft) => self.records.push(draft.restart.clone()),
        }
        Ok(())
    }

    fn save(&mut self, state: &State) -> Result<(), String> {
        self.saved = Some(serde_json::to_string(state).unwrap());
        Ok(())
    }

    fn notify(&mut self, _message: &str) {}

    fn log(&mut self, line: &str) {
        self.journal.push(line.to_string());
    }
}

/// How a run ended, for the assertions.
struct Run {
    fake: Fake,
    state: State,
}

impl Run {
    /// When a person could first type: the greeter accepting input, a text
    /// login on the terminal on screen three seconds after its start (with
    /// Plymouth gone), or a person's session.
    fn prompt_at(&self) -> Option<Micros> {
        let text = self
            .fake
            .fallbacks
            .last()
            .filter(|(tty, _)| self.fake.active_vt == Some(*tty) && !self.fake.plymouthd_alive)
            .map(|(_, at)| at + limits::TEXT_LOGIN);
        [self.fake.first_ready, text].into_iter().flatten().min()
    }

    /// The first prompt at or after `from`.
    fn prompt_after(&self, from: Micros) -> Option<Micros> {
        let text = self
            .fake
            .fallbacks
            .iter()
            .filter(|(tty, at)| *at >= from && self.fake.active_vt == Some(*tty))
            .map(|(_, at)| at + limits::TEXT_LOGIN)
            .next();
        let ready = self.fake.readies.iter().copied().find(|at| *at >= from);
        [ready, text].into_iter().flatten().min()
    }

    fn fell_back(&self) -> Option<Tty> {
        self.fake.fallbacks.last().map(|(tty, _)| *tty)
    }

    fn has_line(&self, stage: &str, result: &str, reason: &str) -> bool {
        let line = format!("BETTER_OS_LOGIN_STAGE={stage} RESULT={result} REASON={reason}");
        self.fake.journal.contains(&line)
    }

    /// Every note the supervisor kept has its journal line, and every unit
    /// it named is on the fixed list.
    fn check_invariants(&self) {
        for note in &self.state.notes {
            assert!(
                self.fake.journal.contains(&note.journal_line()),
                "{:?} has no journal line in {:?}",
                note,
                self.fake.journal
            );
        }
        for name in &self.fake.units_named {
            assert!(UNITS.contains(name), "{name} is not on the fixed list");
        }
        assert_eq!(
            self.fake.stops_while_person, 0,
            "the greeter's unit was stopped while a person's session ran"
        );
        if let Some((tty, marked)) = self.fake.fallback_marker {
            let (started_tty, started) = *self.fake.fallbacks.last().unwrap();
            assert_eq!(tty, started_tty);
            assert!(marked <= started, "the marker is written before the start");
            assert!(
                self.fake.resets.contains(&Tty::Tty1),
                "tty1 is given back to text"
            );
            assert!(self.fake.issue.is_some(), "the message is written");
            assert!(self.fake.held);
        }
    }

    /// The property for the boot's first appearance.
    fn assert_property(&self) {
        self.check_invariants();
        let at = self.prompt_at().unwrap_or_else(|| {
            panic!(
                "no prompt at all; journal {:?}, state {:?}",
                self.fake.journal, self.state.phase
            )
        });
        assert!(
            at <= T0 + limits::BUDGET,
            "the prompt came {} s after T0; journal {:?}",
            (at - T0) as f64 / SECOND as f64,
            self.fake.journal
        );
    }
}

/// Runs the supervisor for `seconds` of fake time from T0, restarting it at
/// each of `restarts` (seconds after T0) from what it saved.
fn run_with_restarts(scenario: Scenario, seconds: u64, restarts: &[u64]) -> Run {
    let mut fake = Fake::new(scenario);
    let mut supervisor =
        Supervisor::start(&mut fake, Loaded::Absent, Some("b".repeat(32)), Some(T0));
    let end = T0 + seconds * SECOND;
    while fake.now < end {
        let since = fake.now - T0;
        if restarts.iter().any(|r| r * SECOND == since) {
            fake.forget_children();
            let saved: State = serde_json::from_str(fake.saved.as_ref().unwrap()).unwrap();
            let now = fake.now;
            supervisor = Supervisor::start(
                &mut fake,
                Loaded::Found(Box::new(saved)),
                Some("b".repeat(32)),
                // systemd's InactiveExit moved with the restart: must not be used.
                Some(now),
            );
        }
        supervisor.tick(&mut fake);
        fake.now += limits::TICK;
    }
    Run {
        state: supervisor.state().clone(),
        fake,
    }
}

fn run(scenario: Scenario, seconds: u64) -> Run {
    run_with_restarts(scenario, seconds, &[])
}

fn no_heartbeat() -> Greeter {
    Greeter {
        heartbeat: None,
        ready: None,
        ..GOOD
    }
}

#[test]
fn a_normal_boot_shows_the_greeter_and_records_nothing() {
    let run = run(Scenario::good(), 60);
    run.assert_property();
    assert!(run.fake.first_ready.unwrap() < T0 + 5 * SECOND);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
    assert!(run.fake.records.is_empty());
    assert!(run.fake.fallbacks.is_empty());
    assert!(run.state.notes.is_empty(), "{:?}", run.state.notes);
    assert_eq!(run.fake.greeter.starts, 1);
    assert!(run.fake.held);
    // deactivate before the greeter, quit with the splash kept at ready.
    assert!(!run.fake.plymouthd_alive);
    assert_eq!(run.fake.modes, [GreeterMode::Plain]);
}

#[test]
fn a_compositor_that_crashes_at_start_is_retried_twice_then_the_text_login() {
    let run = run(Scenario::good().starts(&[no_heartbeat()]), 100);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(Tty::Tty1));
    assert_eq!(run.fake.greeter.starts, 3);
    assert_eq!(
        run.fake.modes,
        [GreeterMode::Plain, GreeterMode::Plain, GreeterMode::Safe],
        "the third attempt is in safe mode"
    );
    assert!(run.has_line("6", "retried", "no-heartbeat"));
    assert!(run.has_line("6", "fell-back", "no-heartbeat"));
    assert_eq!(run.fake.issue, Some(LoginStage::DisplayServer));
    assert!(matches!(
        run.fake.records.as_slice(),
        [RestartOutcome::TextLoginUsed {
            stage: LoginStage::DisplayServer,
            ..
        }]
    ));
    assert_eq!(run.fake.active_vt, Some(Tty::Tty1));
}

#[test]
fn a_compositor_that_ignores_sigterm_costs_each_stop_its_six_seconds_and_no_more() {
    let stubborn = Greeter {
        stop: Stop::IgnoresTerm,
        ..no_heartbeat()
    };
    let run = run(Scenario::good().starts(&[stubborn]), 100);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(Tty::Tty1));
    assert!(run.has_line("6", "fell-back", "no-heartbeat"));
}

#[test]
fn a_greeter_that_never_becomes_ready_falls_back_at_stage_7() {
    let crashing = Greeter {
        ready: None,
        ..GOOD
    };
    let run = run(Scenario::good().starts(&[crashing]), 100);
    run.assert_property();
    assert!(run.has_line("7", "retried", "not-ready"));
    assert!(run.has_line("7", "fell-back", "not-ready"));
    assert_eq!(run.fake.issue, Some(LoginStage::LoginScreenProgram));
}

#[test]
fn greetd_hanging_before_the_greeter_misses_stage_5_and_falls_back() {
    let hung = Greeter {
        session: None,
        heartbeat: None,
        ready: None,
        hangs_after_ready: None,
        stop: Stop::IgnoresTerm,
    };
    let run = run(Scenario::good().starts(&[hung]), 100);
    run.assert_property();
    assert!(run.has_line("5", "retried", "no-greeter-session"));
    assert!(run.has_line("5", "fell-back", "no-greeter-session"));
    assert_eq!(run.fake.issue, Some(LoginStage::SignInService));
}

#[test]
fn a_missing_greeter_account_is_stage_5_and_named_as_such() {
    let mut scenario = Scenario::good();
    scenario.account = false;
    let run = run(scenario, 100);
    run.assert_property();
    assert!(run.has_line("5", "fell-back", "greeter-account-missing"));
}

#[test]
fn a_retry_that_works_is_recorded_as_one_restart() {
    let run = run(Scenario::good().starts(&[no_heartbeat(), GOOD]), 60);
    run.assert_property();
    assert!(run.fake.fallbacks.is_empty());
    assert_eq!(
        run.fake.records,
        [RestartOutcome::LoginScreenRestarted {
            stage: LoginStage::DisplayServer,
            restarts: 1,
            after_seconds: ((run.fake.first_ready.unwrap() - T0) / SECOND) as u32,
        }]
    );
}

#[test]
fn logind_hanging_from_before_t0_falls_back_at_stage_3_after_12_seconds() {
    let mut scenario = Scenario::good();
    scenario.logind_from = None;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.has_line("3", "fell-back", "logind-no-answer"));
    let (_, started) = run.fake.fallbacks[0];
    assert!(started <= T0 + limits::STAGES + SECOND, "{}", started - T0);
    assert_eq!(run.fake.greeter.starts, 0, "no greeter without logind");
    assert_eq!(run.fake.issue, Some(LoginStage::LoginManager));
}

#[test]
fn logind_restarting_during_the_stages_delays_nothing_once_it_answers() {
    let mut scenario = Scenario::good();
    scenario.logind_from = Some(6 * SECOND);
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.fake.fallbacks.is_empty());
}

#[test]
fn logind_stopping_after_the_greeter_started_uses_the_cgroup_for_stage_5() {
    let mut scenario = Scenario::good();
    scenario.logind_until = Some(SECOND);
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.fake.fallbacks.is_empty(), "{:?}", run.fake.journal);
}

#[test]
fn no_graphics_device_falls_back_at_stage_1() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::None;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.has_line("1", "fell-back", "no-graphics-device"));
    assert_eq!(run.fake.issue, Some(LoginStage::GraphicsDevice));
}

#[test]
fn the_firmware_framebuffer_alone_at_12_seconds_is_used_and_noted() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::Firmware;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.has_line("1", "ok", "firmware-framebuffer-only"));
    let ready = run.fake.first_ready.unwrap() - T0;
    assert!((12 * SECOND..20 * SECOND).contains(&ready), "{ready}");
}

#[test]
fn a_late_real_driver_restarts_the_greeter_on_it_while_nobody_is_logged_in() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::Firmware;
    scenario.cards_later = Some((25 * SECOND, Cards::Real));
    let run = run(scenario, 80);
    run.assert_property();
    assert!(run.has_line("1a", "ok", "real-driver-bound"));
    assert_eq!(run.fake.greeter.starts, 2);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
    assert!(!run.state.on_firmware);
}

#[test]
fn a_late_real_driver_after_a_login_leaves_the_session_alone() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::Firmware;
    scenario.cards_later = Some((25 * SECOND, Cards::Real));
    scenario.person = Some((20 * SECOND, None));
    let run = run(scenario, 80);
    run.assert_property();
    assert_eq!(run.fake.greeter.starts, 1, "{:?}", run.fake.journal);
    assert!(matches!(run.state.phase, Phase::Session { .. }));
}

#[test]
fn a_card_with_no_screen_starts_the_greeter_and_waits_with_a_new_t0() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::RealNoScreen;
    scenario.cards_later = Some((40 * SECOND, Cards::Real));
    let run = run(scenario, 120);
    run.check_invariants();
    assert!(run.has_line("1", "ok", "no-screen"));
    assert!(run.has_line("1", "ok", "screen-connected"));
    assert!(run.fake.fallbacks.is_empty(), "{:?}", run.fake.journal);
    let ready = run.fake.first_ready.unwrap() - T0;
    // Within 80 s of the screen's connection, the new T0.
    assert!((40 * SECOND..=40 * SECOND + limits::BUDGET).contains(&ready));
    assert!(run.fake.held, "no screen still keeps the backstop away");
}

#[test]
fn a_seat_that_is_not_graphical_continues_and_is_noted() {
    let mut scenario = Scenario::good();
    scenario.can_graphical = false;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.has_line("2", "ok", "seat-not-graphical"));
    assert!(run.fake.fallbacks.is_empty());
}

#[test]
fn a_plymouth_that_hangs_is_killed_inside_the_first_12_seconds() {
    let mut scenario = Scenario::good();
    scenario.plymouth_hangs = true;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.fake.plymouthd_killed);
    assert!(run.has_line("4", "ok", "plymouth-did-not-return"));
    assert!(run.fake.greeter.started_at <= T0 + limits::STAGES + limits::TICK);
}

#[test]
fn a_plymouth_quit_that_hangs_during_a_fall_back_is_killed_within_the_stop() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::None;
    scenario.quit_hangs = true;
    let run = run(scenario, 60);
    run.assert_property();
    assert!(run.fake.plymouthd_killed);
    let (_, at) = run.fake.fallbacks[0];
    assert!(at <= T0 + limits::STAGES + limits::STOP + limits::TICK);
}

#[test]
fn a_stop_that_never_completes_moves_the_fall_back_to_tty2_at_once() {
    let stuck = Greeter {
        stop: Stop::Never,
        ..no_heartbeat()
    };
    let run = run(Scenario::good().starts(&[stuck]), 60);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(Tty::Tty2));
    assert_eq!(
        run.fake.greeter.starts, 1,
        "no second start behind a stuck stop"
    );
    assert!(run.has_line("6", "fell-back", "stop-did-not-complete"));
    assert!(run.fake.resets.contains(&Tty::Tty1) && run.fake.resets.contains(&Tty::Tty2));
    assert_eq!(run.fake.active_vt, Some(Tty::Tty2));
}

#[test]
fn a_refused_text_login_on_tty1_moves_to_tty2() {
    let mut scenario = Scenario::good().starts(&[no_heartbeat()]);
    scenario.fallback_tty1_refused = true;
    let run = run(scenario, 100);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(Tty::Tty2));
    assert!(run.has_line("6", "fell-back", "fallback-start-refused"));
    assert_eq!(run.fake.active_vt, Some(Tty::Tty2));
}

#[test]
fn the_worst_case_with_every_stop_at_its_limit_is_inside_80_seconds() {
    // Each attempt runs its whole 15 s and each stop ends by SIGKILL, with
    // the stages taking their whole 12 s on the firmware framebuffer.
    let worst = Greeter {
        session: Some(SECOND),
        heartbeat: Some(2 * SECOND),
        ready: None,
        hangs_after_ready: None,
        stop: Stop::AtTheLimit,
    };
    let mut scenario = Scenario::good().starts(&[worst]);
    scenario.cards = Cards::Firmware;
    scenario.plymouth_hangs = true;
    let run = run(scenario, 100);
    run.assert_property();
    let (_, started) = *run.fake.fallbacks.last().unwrap();
    assert!(
        started + limits::TEXT_LOGIN <= T0 + limits::BUDGET,
        "{} s",
        (started - T0) / SECOND
    );
}

#[test]
fn a_supervisor_restarted_in_the_middle_of_an_attempt_continues_its_clock() {
    let scenario = Scenario::good().starts(&[no_heartbeat()]);
    let plain = run(scenario.clone(), 100);
    // Restarted during the second attempt, during a stop, and during the
    // fall back's stop: the same appearance, the same deadlines.
    for restarts in [&[25][..], &[18, 32], &[17, 33, 52, 60]] {
        let restarted = run_with_restarts(scenario.clone(), 100, restarts);
        restarted.assert_property();
        let (_, at) = *restarted.fake.fallbacks.last().unwrap();
        let (_, plain_at) = *plain.fake.fallbacks.last().unwrap();
        assert!(
            at.abs_diff(plain_at) <= 2 * limits::TICK,
            "restarted at {restarts:?}: fell back at {} s, not {} s",
            (at - T0) / SECOND,
            (plain_at - T0) / SECOND
        );
        assert_eq!(restarted.state.appearance, 1);
        assert_eq!(restarted.fake.greeter.starts, 3, "{restarts:?}");
        assert!(restarted.has_line("supervisor", "ok", "restarted"));
    }
}

#[test]
fn a_supervisor_restarted_while_plymouth_hangs_kills_it_by_the_first_deadline() {
    let mut scenario = Scenario::good();
    scenario.plymouth_hangs = true;
    scenario.cards = Cards::Firmware;
    let run = run_with_restarts(scenario, 60, &[9]);
    run.assert_property();
    assert!(run.fake.plymouthd_killed);
    assert!(run.fake.greeter.started_at <= T0 + limits::STAGES + limits::TICK);
}

#[test]
fn an_unreadable_state_file_falls_back_unless_a_person_or_a_ready_greeter_is_there() {
    let mut fake = Fake::new(Scenario::good());
    let mut supervisor = Supervisor::start(&mut fake, Loaded::Unreadable, None, Some(T0));
    for _ in 0..80 {
        supervisor.tick(&mut fake);
        fake.now += limits::TICK;
    }
    assert!(matches!(supervisor.state().phase, Phase::FellBack { .. }));
    assert_eq!(fake.issue, Some(LoginStage::Supervisor));
    assert!(
        fake.journal
            .iter()
            .any(|l| l.ends_with("REASON=state-unreadable"))
    );
}

#[test]
fn an_automatic_login_satisfies_the_property_and_is_never_stopped() {
    let mut scenario = Scenario::good();
    scenario.automatic_login = true;
    let run = run(scenario, 100);
    run.check_invariants();
    assert!(matches!(run.state.phase, Phase::Session { .. }));
    assert!(run.fake.fallbacks.is_empty());
    assert!(
        run.fake.held,
        "the backstop must not take an automatic login's screen"
    );
    assert!(!run.fake.plymouthd_alive);
}

#[test]
fn a_session_that_ends_brings_the_greeter_back_within_80_seconds_of_its_end() {
    let mut scenario = Scenario::good();
    scenario.person = Some((20 * SECOND, Some(60 * SECOND)));
    let run = run(scenario, 100);
    run.check_invariants();
    assert_eq!(run.state.appearance, 2);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
    let end = T0 + 60 * SECOND;
    assert!(run.prompt_after(end).unwrap() <= end + limits::BUDGET);
}

#[test]
fn logind_crashing_at_logout_is_seen_through_greetds_tree() {
    // logind stops answering when the person logs out (ticket 256) and
    // still listed their session when it went: the end is seen from
    // greetd's tree, and the greeter greetd brings back counts.
    let mut scenario = Scenario::good();
    scenario.person = Some((20 * SECOND, Some(40 * SECOND)));
    scenario.logind_until = Some(40 * SECOND);
    let run = run(scenario, 140);
    run.check_invariants();
    assert_eq!(run.state.appearance, 2, "{:?}", run.fake.journal);
    let end = T0 + 40 * SECOND;
    assert!(run.prompt_after(end).unwrap() <= end + limits::BUDGET);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
}

#[test]
fn logind_gone_after_a_logout_with_no_greeter_back_ends_in_the_text_login() {
    // The same, with greetd bringing nothing back: the new appearance's
    // stage 3 fails, a text login within 80 s of the session's end.
    let mut scenario = Scenario::good();
    scenario.person = Some((20 * SECOND, Some(40 * SECOND)));
    scenario.logind_until = Some(40 * SECOND);
    scenario.nothing_after_logout = true;
    let run = run(scenario, 140);
    run.check_invariants();
    let end = T0 + 40 * SECOND;
    assert!(run.prompt_after(end).unwrap() <= end + limits::BUDGET);
    assert!(run.has_line("3", "fell-back", "logind-no-answer"));
    assert_eq!(run.fell_back(), Some(Tty::Tty1));
}

#[test]
fn a_greeter_that_hangs_after_ready_is_restarted_within_30_seconds() {
    let hangs = Greeter {
        hangs_after_ready: Some(5 * SECOND),
        ..GOOD
    };
    let run = run(Scenario::good().starts(&[hangs, GOOD]), 80);
    run.check_invariants();
    assert!(run.has_line("7", "retried", "heartbeat-stale"));
    assert_eq!(run.fake.greeter.starts, 2);
    assert_eq!(run.state.appearance, 2);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
}

#[test]
fn a_stale_logind_session_does_not_hold_the_greeter_once_greetds_tree_is_empty() {
    // Seen directly: logind still lists the person's session, greetd's tree
    // has no process of theirs. The session has ended.
    let seen = Seen {
        drm: None,
        logind: Some(Logind {
            can_graphical: true,
            sessions: vec![LogindSession {
                id: "2".into(),
                uid: PERSON_UID,
                class: "user".into(),
                service: "greetd".into(),
                seat: "seat0".into(),
                kind: "wayland".into(),
            }],
        }),
        greetd: Some(Greetd {
            processes: vec![Process {
                pid: 100,
                uid: 0,
                in_cgroup: true,
            }],
        }),
        greeter_uid: Some(GREETER_UID),
        ..Seen::default()
    };
    assert!(!seen.person_certain());
    assert!(!seen.person_possible());
}

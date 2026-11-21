//! The property (design/login-screen-spec.md section 5.10) on a fake
//! clock, for every injected outcome: within 80 seconds of T0 the login
//! screen accepts input, or a text login runs on the terminal that is on
//! screen with Plymouth gone and a journal line for every stage that did
//! not pass. The fake world stands in for logind, systemd, `/sys/class/drm`,
//! Plymouth, the greeter's cgroup, its session's scope and the clock; the
//! supervisor's own loop (`world::Supervisor`) runs unchanged over it,
//! restarts included.
//!
//! As on a real machine, `pam_systemd` puts the greeter's session (greetd's
//! session worker, the start program, the compositor) in a scope of its
//! own, `session-cN.scope`: a stop of greetd's unit reaches greetd only,
//! and greetd sends its session `SIGTERM` and nothing more.

use login_supervisor::rules::{
    Action, Card, Drm, Greetd, GreeterMode, HeartbeatFile, Job, Loaded, Logind, LogindSession,
    Micros, Needs, Phase, PlymouthCommand, Process, SECOND, ScopeSeen, Seen, State, TextTty, UNITS,
    Unit, UnitStart, Word, limits,
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
    /// A real card with no screen beside the firmware framebuffer that
    /// shows the picture.
    FirmwareBesideDarkReal,
    RealNoScreen,
    Real,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    /// greetd and its session both gone half a second after the stop.
    Clean,
    /// The compositor in the session's scope ignores `SIGTERM`: greetd is
    /// gone at once, the session only when it is killed.
    CompositorIgnoresTerm,
    /// greetd itself ignores `SIGTERM`: systemd's `SIGKILL` after 4 s ends
    /// it, and its session never gets even the `SIGTERM`.
    GreetdHung,
    /// Everything gone just inside the 6 s a stop may take.
    AtTheLimit,
    /// The compositor is in uninterruptible sleep: never gone.
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
    /// systemd refuses to start the text login on these terminals.
    refused: Vec<TextTty>,
    /// After the person's logout greetd's first start shows nothing.
    nothing_after_logout: bool,
    /// How long each `systemctl` call takes.
    latency: Micros,
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
            refused: Vec::new(),
            nothing_after_logout: false,
            latency: 0,
        }
    }

    fn starts(mut self, starts: &[Greeter]) -> Scenario {
        self.starts = starts.to_vec();
        self
    }
}

/// One start of the greeter's unit and its session.
#[derive(Clone, Copy, Debug, Default)]
struct GreeterRun {
    number: u32,
    started_at: Micros,
    /// greetd, in the unit's cgroup.
    greetd_alive: bool,
    /// The session's scope still has its processes.
    scope_alive: bool,
    stop_at: Option<Micros>,
    killed_at: Option<Micros>,
}

/// The fake machine.
struct Fake {
    scenario: Scenario,
    now: Micros,
    /// Every start of the greeter's unit, the last one current.
    runs: Vec<GreeterRun>,
    mode: Option<GreeterMode>,
    modes: Vec<GreeterMode>,
    /// The plymouth command running and when it was started.
    plymouth: Option<(PlymouthCommand, Micros)>,
    plymouth_finished: bool,
    plymouthd_alive: bool,
    plymouthd_killed: bool,
    fallbacks: Vec<(TextTty, Micros)>,
    active_vt: Option<TextTty>,
    resets: Vec<String>,
    issue: Option<LoginStage>,
    fallback_marker: Option<(TextTty, Micros)>,
    held: bool,
    records: Vec<RestartOutcome>,
    journal: Vec<String>,
    saved: Option<String>,
    units_named: Vec<&'static str>,
    stops_while_person: u32,
    /// Starts of the greeter or a text login while an earlier greeter's
    /// session still had processes and could have been killed.
    starts_under_a_live_greeter: u32,
    first_ready: Option<Micros>,
    readies: Vec<Micros>,
    accepting: bool,
    downtime: Micros,
}

impl Fake {
    fn new(scenario: Scenario) -> Fake {
        Fake {
            scenario,
            now: T0,
            runs: Vec::new(),
            mode: None,
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
            starts_under_a_live_greeter: 0,
            first_ready: None,
            readies: Vec::new(),
            accepting: false,
            downtime: 0,
        }
    }

    fn since_t0(&self) -> Micros {
        self.now - T0
    }

    fn current(&self) -> Option<&GreeterRun> {
        self.runs.last()
    }

    fn behaviour_of(&self, run: &GreeterRun) -> Greeter {
        let index = (run.number.max(1) - 1) as usize;
        self.scenario.starts[index.min(self.scenario.starts.len() - 1)]
    }

    fn behaviour(&self) -> Greeter {
        self.current().map(|r| self.behaviour_of(r)).unwrap_or(GOOD)
    }

    fn cards(&self) -> Cards {
        match self.scenario.cards_later {
            Some((at, cards)) if self.since_t0() >= at => cards,
            _ => self.scenario.cards,
        }
    }

    fn screen(&self) -> bool {
        !matches!(self.cards(), Cards::None | Cards::RealNoScreen)
    }

    fn logind_answers(&self) -> bool {
        let t = self.since_t0();
        self.scenario.logind_from.is_some_and(|from| t >= from)
            && self.scenario.logind_until.is_none_or(|until| t < until)
    }

    fn scope_path(number: u32) -> String {
        format!("/user.slice/user-{GREETER_UID}.slice/session-c{number}.scope")
    }

    /// Settles every stop as the stop behaviour says.
    fn settle(&mut self) {
        let now = self.now;
        let behaviours: Vec<Greeter> = self.runs.iter().map(|r| self.behaviour_of(r)).collect();
        for (run, behaviour) in self.runs.iter_mut().zip(behaviours) {
            let Some(stop) = run.stop_at else {
                continue;
            };
            let killed = run.killed_at.is_some_and(|k| now >= k + SECOND / 5);
            let (greetd_gone, scope_gone) = match behaviour.stop {
                Stop::Clean => (now >= stop + SECOND / 2, now >= stop + SECOND / 2),
                Stop::CompositorIgnoresTerm => (now >= stop + SECOND / 2, killed),
                Stop::GreetdHung => (now >= stop + 4 * SECOND + SECOND / 5, killed),
                Stop::AtTheLimit => {
                    let gone = now >= stop + 6 * SECOND - SECOND / 10;
                    (gone, gone)
                }
                Stop::Never => (now >= stop + SECOND / 2, false),
            };
            if greetd_gone {
                run.greetd_alive = false;
            }
            if scope_gone {
                run.scope_alive = false;
            }
        }
    }

    fn up(&self, after: Option<Micros>) -> bool {
        self.current().is_some_and(|r| {
            r.greetd_alive
                && r.stop_at.is_none()
                && after.is_some_and(|a| self.now >= r.started_at + a)
        })
    }

    fn logged_out(&self) -> bool {
        self.scenario
            .person
            .is_some_and(|(_, out)| out.is_some_and(|out| self.since_t0() >= out))
    }

    fn person_session(&self) -> bool {
        let Some(run) = self.current() else {
            return false;
        };
        if !run.greetd_alive {
            return false;
        }
        if self.scenario.automatic_login && run.number == 1 {
            return self.now >= run.started_at + SECOND;
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

    fn greeter_session_open(&self) -> bool {
        let b = self.behaviour();
        self.up(b.session)
            && self.scenario.account
            && !self.scenario.automatic_login
            && !self.person_session()
            && !(self.scenario.nothing_after_logout
                && self.logged_out()
                && self.current().is_some_and(|r| r.number == 1))
    }

    fn heartbeat(&self) -> Option<HeartbeatFile> {
        if !self.greeter_session_open() {
            return None;
        }
        let run = *self.current()?;
        let b = self.behaviour();
        let first = b.heartbeat?;
        if self.now < run.started_at + first {
            return None;
        }
        let since_first = self.now - (run.started_at + first);
        let ready_at = b.ready.map(|r| run.started_at + r);
        let ready = ready_at.is_some_and(|r| self.now >= r);
        let frozen = match (ready_at, b.hangs_after_ready) {
            (Some(r), Some(h)) if self.now >= r + h => {
                Some((r + h).saturating_sub(run.started_at + first))
            }
            _ => None,
        };
        let generation = frozen.unwrap_or(since_first) / (5 * SECOND);
        Some(HeartbeatFile {
            identity: (u64::from(run.number) << 40) | (generation << 1) | u64::from(ready),
            word: if ready { Word::Ready } else { Word::Starting },
        })
    }

    fn greeter_accepting_input(&self) -> bool {
        self.heartbeat().is_some_and(|h| h.word == Word::Ready) && self.screen()
    }

    /// The processes of a run's session scope: the worker (root) and the
    /// greeter, while the scope lives and its session was opened.
    fn scope_processes(&self, run: &GreeterRun) -> Vec<Process> {
        let b = self.behaviour_of(run);
        // Opened before the stop, if at all.
        let opened = b.session.is_some_and(|s| {
            let open_at = run.started_at + s;
            self.now >= open_at && run.stop_at.is_none_or(|stop| stop > open_at)
        });
        if !run.scope_alive || !opened || !self.scenario.account || self.scenario.automatic_login {
            return Vec::new();
        }
        vec![
            Process {
                pid: 150 + run.number * 10,
                uid: 0,
                in_cgroup: false,
            },
            Process {
                pid: 151 + run.number * 10,
                uid: GREETER_UID,
                in_cgroup: false,
            },
        ]
    }

    fn observe_now(&mut self, needs: &Needs) -> Seen {
        self.settle();
        let accepting = self.greeter_accepting_input();
        if accepting && !self.accepting {
            self.readies.push(self.now);
            self.first_ready.get_or_insert(self.now);
        }
        self.accepting = accepting;
        let drm = match self.cards() {
            Cards::None => None,
            cards => {
                let mut list = Vec::new();
                match cards {
                    Cards::Firmware => list.push(("card0", true, true)),
                    Cards::FirmwareBesideDarkReal => {
                        list.push(("card0", true, true));
                        list.push(("card1", false, false));
                    }
                    Cards::RealNoScreen => list.push(("card1", false, false)),
                    Cards::Real => list.push(("card1", false, true)),
                    Cards::None => {}
                }
                Some(Drm {
                    entries: list.iter().map(|c| c.0.to_string()).collect(),
                    cards: list
                        .into_iter()
                        .map(|(name, firmware, screen)| Card {
                            name: name.into(),
                            firmware,
                            screen,
                        })
                        .collect(),
                })
            }
        };
        let current = self.current().copied();
        let mut sessions = Vec::new();
        if self.greeter_session_open()
            && let Some(run) = current
        {
            sessions.push(LogindSession {
                id: format!("c{}", run.number),
                uid: GREETER_UID,
                class: "greeter".into(),
                service: "greetd-greeter".into(),
                seat: "seat0".into(),
                kind: "wayland".into(),
                scope: format!("session-c{}.scope", run.number),
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
                scope: "session-2.scope".into(),
            });
        }
        let logind = self.logind_answers().then_some(Logind {
            can_graphical: self.scenario.can_graphical,
            sessions: sessions.clone(),
        });
        // greetd's tree: greetd in the unit's cgroup and, while greetd
        // lives, its session's processes as its descendants.
        let mut processes = Vec::new();
        if let Some(run) = current
            && run.greetd_alive
        {
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
            } else {
                processes.extend(self.scope_processes(&run));
            }
        }
        // The scopes: logind's greeter session, the tree's, the asked-for.
        let mut scopes: Vec<ScopeSeen> = Vec::new();
        let mut paths: Vec<(String, bool)> = Vec::new();
        if self.logind_answers() {
            for s in sessions.iter().filter(|s| s.class == "greeter") {
                paths.push((
                    format!("/user.slice/user-{}.slice/{}", s.uid, s.scope),
                    true,
                ));
            }
        }
        if let Some(run) = current
            && run.greetd_alive
            && !self.scope_processes(&run).is_empty()
        {
            paths.push((Self::scope_path(run.number), false));
        }
        paths.extend(needs.scopes.iter().map(|p| (p.clone(), false)));
        for (path, greeter_session) in paths {
            if let Some(known) = scopes.iter_mut().find(|s| s.path == path) {
                known.greeter_session |= greeter_session;
                continue;
            }
            let processes = self
                .runs
                .iter()
                .find(|r| Self::scope_path(r.number) == path)
                .map(|r| self.scope_processes(r))
                .unwrap_or_default();
            scopes.push(ScopeSeen {
                path,
                greeter_session,
                processes: Some(processes),
            });
        }
        // A live compositor holds the card.
        let holders: Vec<u32> = self
            .runs
            .iter()
            .filter(|r| !self.scope_processes(r).is_empty())
            .map(|r| 151 + r.number * 10)
            .collect();
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
        let text_login_active = needs.text_login.map(|tty| {
            self.fallbacks.iter().any(|(t, _)| *t == tty) && !self.scenario.refused.contains(&tty)
        });
        Seen {
            drm,
            logind,
            greetd: Some(Greetd { processes }),
            scopes,
            drm_holders: needs.drm_holders.then_some(holders),
            text_login_active,
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

    /// An earlier greeter's session still runs and the supervisor could
    /// have killed it.
    fn live_earlier_greeter(&self) -> bool {
        self.runs.iter().any(|r| {
            r.stop_at.is_some()
                && !self.scope_processes(r).is_empty()
                && self.behaviour_of(r).stop != Stop::Never
        })
    }
}

impl World for Fake {
    fn now(&mut self) -> Micros {
        self.now
    }

    fn observe(&mut self, needs: &Needs) -> Seen {
        self.observe_now(needs)
    }

    fn perform(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::StartUnit(unit) => {
                self.now += self.scenario.latency;
                self.units_named.push(unit.name());
                self.settle();
                match unit {
                    Unit::Greeter => {
                        if self.live_earlier_greeter() {
                            self.starts_under_a_live_greeter += 1;
                        }
                        let running = self.current().is_some_and(|r| r.greetd_alive);
                        if !running {
                            let number = self.runs.len() as u32 + 1;
                            self.runs.push(GreeterRun {
                                number,
                                started_at: self.now,
                                greetd_alive: true,
                                scope_alive: true,
                                stop_at: None,
                                killed_at: None,
                            });
                            self.modes.push(self.mode.unwrap_or(GreeterMode::Plain));
                        }
                    }
                    Unit::Fallback(tty) => {
                        if self.live_earlier_greeter() {
                            self.starts_under_a_live_greeter += 1;
                        }
                        if self.scenario.refused.contains(tty) {
                            return Err("Unit is masked".into());
                        }
                        self.fallbacks.push((*tty, self.now));
                    }
                }
            }
            Action::StopUnit(unit) => {
                self.now += self.scenario.latency;
                self.units_named.push(unit.name());
                if *unit == Unit::Greeter {
                    if self.person_session() {
                        self.stops_while_person += 1;
                    }
                    let now = self.now;
                    if let Some(run) = self.runs.last_mut()
                        && run.greetd_alive
                        && run.stop_at.is_none()
                    {
                        run.stop_at = Some(now);
                    }
                } else {
                    // The history stays; the text login asked about is
                    // never one that was stopped.
                }
            }
            Action::KillScope { path, greeter } => {
                assert_eq!(*greeter, GREETER_UID);
                let now = self.now;
                if let Some(run) = self
                    .runs
                    .iter_mut()
                    .find(|r| Self::scope_path(r.number) == *path)
                {
                    run.killed_at.get_or_insert(now);
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
            Action::SetGreeterMode(mode) => self.mode = Some(*mode),
            Action::ResetVt(vt) => self.resets.push(vt.name().to_string()),
            Action::ActivateVt(tty) => self.active_vt = Some(*tty),
            Action::WriteIssue(stage) => self.issue = Some(*stage),
            Action::MarkFallbackActive(tty) => self.fallback_marker = Some((*tty, self.now)),
            Action::ClearFallbackActive => self.fallback_marker = None,
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
    /// When a person could first type at or after `from`: the greeter
    /// accepting input, or a text login on the terminal on screen three
    /// seconds after its start, with Plymouth gone.
    fn prompt_after(&self, from: Micros) -> Option<Micros> {
        let text = self
            .fake
            .fallbacks
            .iter()
            .filter(|(tty, at)| {
                *at >= from
                    && self.fake.active_vt == Some(*tty)
                    && !self.fake.plymouthd_alive
                    && !self.fake.scenario.refused.contains(tty)
            })
            .map(|(_, at)| at + limits::TEXT_LOGIN)
            .next();
        let ready = self.fake.readies.iter().copied().find(|at| *at >= from);
        [ready, text].into_iter().flatten().min()
    }

    fn prompt_at(&self) -> Option<Micros> {
        self.prompt_after(T0)
    }

    fn fell_back(&self) -> Option<TextTty> {
        self.fake.fallbacks.last().map(|(tty, _)| *tty)
    }

    fn has_line(&self, stage: &str, result: &str, reason: &str) -> bool {
        let line = format!("BETTER_OS_LOGIN_STAGE={stage} RESULT={result} REASON={reason}");
        self.fake.journal.contains(&line)
    }

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
        assert_eq!(
            self.fake.starts_under_a_live_greeter, 0,
            "a start under an earlier greeter's session that could have been killed; journal {:?}",
            self.fake.journal
        );
        if let Some((tty, marked)) = self.fake.fallback_marker {
            let started = self
                .fake
                .fallbacks
                .iter()
                .find(|(t, _)| *t == tty)
                .map(|(_, at)| *at)
                .expect("a marker for a text login that was never started");
            assert!(
                marked >= started,
                "the marker comes after the start is seen"
            );
            assert!(self.fake.resets.contains(&"tty1".to_string()));
            assert!(self.fake.issue.is_some(), "the message is written");
            assert!(self.fake.held);
        }
    }

    fn assert_property(&self) {
        self.assert_property_within(limits::BUDGET + self.fake.downtime);
    }

    fn assert_property_within(&self, bound: Micros) {
        self.check_invariants();
        let at = self.prompt_at().unwrap_or_else(|| {
            panic!(
                "no prompt at all; journal {:?}, state {:?}",
                self.fake.journal, self.state.phase
            )
        });
        assert!(
            at <= T0 + bound,
            "the prompt came {} s after T0; journal {:?}",
            (at - T0) as f64 / SECOND as f64,
            self.fake.journal
        );
    }
}

/// Runs the supervisor for `seconds` of fake time from T0, restarting it at
/// each `(seconds after T0, downtime)` of `restarts` from what it saved, as
/// systemd's automatic restart does (one more `NRestarts`).
fn run_with_restarts(scenario: Scenario, seconds: u64, restarts: &[(u64, Micros)]) -> Run {
    let mut fake = Fake::new(scenario);
    let mut supervisor = Supervisor::start(
        &mut fake,
        Loaded::Absent,
        Some("b".repeat(32)),
        UnitStart {
            began: Some(T0),
            restarts: Some(0),
        },
    );
    let end = T0 + seconds * SECOND;
    let mut done = 0;
    let mut restarted = 0;
    while fake.now < end {
        if let Some((at, downtime)) = restarts.get(done)
            && fake.now >= T0 + at * SECOND
        {
            done += 1;
            restarted += 1;
            fake.forget_children();
            fake.now += downtime;
            fake.downtime += downtime;
            let saved: State = serde_json::from_str(fake.saved.as_ref().unwrap()).unwrap();
            let now = fake.now;
            supervisor = Supervisor::start(
                &mut fake,
                Loaded::Found(Box::new(saved)),
                Some("b".repeat(32)),
                UnitStart {
                    // systemd's InactiveExit moved with the restart: never
                    // taken as T0.
                    began: Some(now),
                    restarts: Some(restarted),
                },
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
    assert_eq!(run.fake.runs.len(), 1);
    assert!(run.fake.held);
    assert!(!run.fake.plymouthd_alive);
    assert_eq!(run.fake.modes, [GreeterMode::Plain]);
}

#[test]
fn a_compositor_that_crashes_at_start_is_retried_twice_then_the_text_login() {
    let run = run(Scenario::good().starts(&[no_heartbeat()]), 100);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(TextTty::Tty8));
    assert_eq!(run.fake.runs.len(), 3);
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
    assert_eq!(run.fake.active_vt, Some(TextTty::Tty8));
}

#[test]
fn a_compositor_that_ignores_sigterm_in_its_own_scope_is_killed_before_anything_starts() {
    let stubborn = Greeter {
        stop: Stop::CompositorIgnoresTerm,
        ..no_heartbeat()
    };
    let run = run(Scenario::good().starts(&[stubborn]), 100);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(TextTty::Tty8));
    assert!(run.has_line("6", "ok", "session-killed"));
    assert!(
        run.fake
            .runs
            .iter()
            .all(|r| r.killed_at.is_some() && !r.scope_alive)
    );
    assert_eq!(run.fake.runs.len(), 3);
}

#[test]
fn greetd_hung_and_killed_by_systemd_leaves_a_session_the_supervisor_ends() {
    let hung = Greeter {
        stop: Stop::GreetdHung,
        ..no_heartbeat()
    };
    let run = run(Scenario::good().starts(&[hung]), 100);
    run.assert_property();
    assert!(run.has_line("6", "ok", "session-killed"));
    assert!(run.fake.runs.iter().all(|r| !r.scope_alive));
}

#[test]
fn greetd_hung_with_logind_gone_still_finds_the_session_it_remembered() {
    // logind stops answering once the greeter's session is open: its scope
    // is known from greetd's tree, and remembered once greetd is killed.
    let hung = Greeter {
        stop: Stop::GreetdHung,
        ..no_heartbeat()
    };
    let mut scenario = Scenario::good().starts(&[hung]);
    scenario.logind_until = Some(14 * SECOND);
    let run = run(scenario, 100);
    run.assert_property();
    assert!(run.fake.runs.iter().all(|r| !r.scope_alive));
}

#[test]
fn a_compositor_that_cannot_be_killed_gets_the_text_login_at_once_and_nothing_else() {
    let stuck = Greeter {
        stop: Stop::Never,
        ..no_heartbeat()
    };
    let run = run(Scenario::good().starts(&[stuck]), 60);
    run.assert_property();
    assert_eq!(run.fell_back(), Some(TextTty::Tty8));
    assert_eq!(
        run.fake.runs.len(),
        1,
        "no second start behind a stuck stop"
    );
    assert!(run.has_line("6", "fell-back", "stop-did-not-complete"));
    assert!(
        run.fake.resets.contains(&"tty1".to_string())
            && run.fake.resets.contains(&"tty8".to_string())
    );
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
        stop: Stop::GreetdHung,
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
    assert!(run.fake.runs.is_empty(), "no greeter without logind");
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
fn logind_stopping_after_the_greeter_started_uses_its_scope_for_stage_5() {
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
    for cards in [Cards::Firmware, Cards::FirmwareBesideDarkReal] {
        let mut scenario = Scenario::good();
        scenario.cards = cards;
        let run = run(scenario, 60);
        run.assert_property();
        assert!(
            run.has_line("1", "ok", "firmware-framebuffer-only"),
            "{cards:?}"
        );
        let ready = run.fake.first_ready.unwrap() - T0;
        assert!((12 * SECOND..20 * SECOND).contains(&ready), "{ready}");
    }
}

#[test]
fn a_firmware_framebuffer_beside_a_dark_real_card_whose_greeter_fails_ends_in_a_text_login() {
    let mut scenario = Scenario::good().starts(&[no_heartbeat()]);
    scenario.cards = Cards::FirmwareBesideDarkReal;
    let run = run(scenario, 100);
    run.assert_property();
    assert_eq!(run.fake.runs.len(), 3, "the usual attempts");
    assert_eq!(run.fell_back(), Some(TextTty::Tty8));
}

#[test]
fn a_late_real_driver_restarts_the_greeter_on_it_while_nobody_is_logged_in() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::Firmware;
    scenario.cards_later = Some((25 * SECOND, Cards::Real));
    let run = run(scenario, 80);
    run.assert_property();
    assert!(run.has_line("1a", "ok", "real-driver-bound"));
    assert_eq!(run.fake.runs.len(), 2);
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
    assert_eq!(run.fake.runs.len(), 1, "{:?}", run.fake.journal);
    assert!(matches!(run.state.phase, Phase::Session { .. }));
}

#[test]
fn a_card_with_no_screen_runs_the_attempts_and_the_greeter_shows_when_a_screen_comes() {
    let mut scenario = Scenario::good();
    scenario.cards = Cards::RealNoScreen;
    scenario.cards_later = Some((40 * SECOND, Cards::Real));
    let run = run(scenario, 120);
    run.check_invariants();
    assert!(run.has_line("1", "ok", "no-screen"));
    assert!(run.fake.fallbacks.is_empty(), "{:?}", run.fake.journal);
    // The greeter was ready with no screen; it shows once one is there.
    assert_eq!(run.fake.first_ready, Some(T0 + 40 * SECOND));
}

#[test]
fn a_card_with_no_screen_whose_greeter_fails_gives_a_text_login_then_the_greeter_once_a_screen_comes()
 {
    let mut scenario =
        Scenario::good().starts(&[no_heartbeat(), no_heartbeat(), no_heartbeat(), GOOD]);
    scenario.cards = Cards::RealNoScreen;
    scenario.cards_later = Some((120 * SECOND, Cards::Real));
    let run = run(scenario, 200);
    run.check_invariants();
    let (_, text_at) = *run
        .fake
        .fallbacks
        .first()
        .unwrap_or_else(|| panic!("{:?}", run.fake.journal));
    assert!(
        text_at + limits::TEXT_LOGIN <= T0 + limits::BUDGET,
        "a text login, usable when a screen is plugged in"
    );
    assert!(!run.fake.held || run.fake.fallback_marker.is_some() || run.state.appearance == 2);
    assert!(run.has_line("1", "ok", "screen-connected"));
    assert_eq!(run.state.appearance, 2);
    assert!(run.fake.first_ready.unwrap() >= T0 + 120 * SECOND);
    assert!(
        run.fake.fallback_marker.is_none(),
        "the marker went with the text login"
    );
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
    assert!(run.fake.runs[0].started_at <= T0 + limits::STAGES + limits::TICK);
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
fn a_refused_text_login_moves_to_the_other_terminal_and_is_marked_only_there() {
    let mut scenario = Scenario::good().starts(&[no_heartbeat()]);
    scenario.refused = vec![TextTty::Tty8];
    let run = run(scenario, 100);
    // One terminal refused costs its 3 s on top of the budget.
    run.assert_property_within(limits::BUDGET + limits::TEXT_LOGIN);
    assert_eq!(run.fell_back(), Some(TextTty::Tty9));
    assert!(run.has_line("F", "retried", "text-login-not-active"));
    assert_eq!(run.fake.active_vt, Some(TextTty::Tty9));
    assert_eq!(run.fake.fallback_marker.map(|m| m.0), Some(TextTty::Tty9));
}

#[test]
fn a_text_login_refused_on_both_terminals_never_silences_the_backstop() {
    let mut scenario = Scenario::good().starts(&[no_heartbeat()]);
    scenario.refused = vec![TextTty::Tty8, TextTty::Tty9];
    let run = run(scenario, 200);
    run.check_invariants();
    assert!(!run.fake.held, "the backstop stays armed");
    assert!(run.fake.fallback_marker.is_none());
}

#[test]
fn the_worst_case_with_every_stop_at_its_limit_and_slow_calls_is_inside_80_seconds() {
    // Each attempt runs its whole 15 s, each stop takes all of its 6 s, the
    // stages take their whole 12 s, and every systemctl call takes its
    // whole 2 s.
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
    scenario.latency = limits::CALL;
    let run = run(scenario, 120);
    run.assert_property();
    // With 2 s calls a stop at its limit does not finish inside the
    // supervisor's 6 s, so the text login comes at once.
    assert!(
        run.fake
            .journal
            .iter()
            .any(|l| l.starts_with("BETTER_OS_LOGIN_STAGE=7 RESULT=fell-back")),
        "{:?}",
        run.fake.journal
    );
    // Without the slow calls, the same machine takes every attempt.
    let mut scenario = Scenario::good().starts(&[worst]);
    scenario.cards = Cards::Firmware;
    scenario.plymouth_hangs = true;
    let run = super_run(scenario);
    run.assert_property();
    assert_eq!(run.fake.runs.len(), 3);
}

fn super_run(scenario: Scenario) -> Run {
    run(scenario, 120)
}

#[test]
fn a_supervisor_restarted_in_the_middle_continues_its_clock_and_pays_only_its_downtime() {
    let scenario = Scenario::good().starts(&[no_heartbeat()]);
    let plain = run(scenario.clone(), 100);
    let (_, plain_at) = *plain.fake.fallbacks.last().unwrap();
    for restarts in [
        &[(25, SECOND)][..],
        &[(18, SECOND), (32, 2 * SECOND)],
        &[(17, SECOND), (33, SECOND), (52, SECOND), (60, SECOND)],
    ] {
        let restarted = run_with_restarts(scenario.clone(), 120, restarts);
        restarted.assert_property();
        let (_, at) = *restarted.fake.fallbacks.last().unwrap();
        assert!(
            at <= plain_at + restarted.fake.downtime + 2 * limits::TICK,
            "restarted at {restarts:?}: fell back at {} s, not by {} s",
            (at - T0) / SECOND,
            (plain_at + restarted.fake.downtime - T0) / SECOND
        );
        assert_eq!(restarted.state.appearance, 1);
        assert!(restarted.has_line("supervisor", "ok", "restarted"));
    }
}

#[test]
fn a_supervisor_that_hangs_at_the_worst_moment_costs_its_watchdog_downtime_and_no_more() {
    // A hang just before the cut-off: watchdog 10 s, abort 5 s, restart 1 s.
    let scenario = Scenario::good().starts(&[no_heartbeat()]);
    let run = run_with_restarts(scenario, 140, &[(66, 16 * SECOND)]);
    run.assert_property_within(limits::BUDGET + 16 * SECOND);
}

#[test]
fn a_supervisor_restarted_while_plymouth_hangs_kills_it_by_the_first_deadline() {
    let mut scenario = Scenario::good();
    scenario.plymouth_hangs = true;
    scenario.cards = Cards::Firmware;
    let run = run_with_restarts(scenario, 60, &[(9, 0)]);
    run.assert_property();
    assert!(run.fake.plymouthd_killed);
    assert!(run.fake.runs[0].started_at <= T0 + limits::STAGES + limits::TICK);
}

#[test]
fn an_unreadable_state_file_falls_back_unless_a_person_or_a_ready_greeter_is_there() {
    let mut fake = Fake::new(Scenario::good());
    let mut supervisor =
        Supervisor::start(&mut fake, Loaded::Unreadable, None, UnitStart::default());
    for _ in 0..80 {
        supervisor.tick(&mut fake);
        fake.now += limits::TICK;
    }
    assert!(matches!(supervisor.state().phase, Phase::TextLogin { .. }));
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
    let mut scenario = Scenario::good();
    scenario.person = Some((20 * SECOND, Some(40 * SECOND)));
    scenario.logind_until = Some(40 * SECOND);
    scenario.nothing_after_logout = true;
    let run = run(scenario, 140);
    run.check_invariants();
    let end = T0 + 40 * SECOND;
    assert!(run.prompt_after(end).unwrap() <= end + limits::BUDGET);
    assert!(run.has_line("3", "fell-back", "logind-no-answer"));
    assert_eq!(run.fell_back(), Some(TextTty::Tty8));
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
    assert_eq!(run.fake.runs.len(), 2);
    assert_eq!(run.state.appearance, 2);
    assert!(matches!(run.state.phase, Phase::Ready { .. }));
}

/// A small deterministic generator, so a failing seed can be run again.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

fn random_greeter(rng: &mut Rng) -> Greeter {
    let at = |rng: &mut Rng, max: u64| Some(rng.below(max * 4) * SECOND / 4);
    Greeter {
        session: if rng.chance(85) { at(rng, 7) } else { None },
        heartbeat: if rng.chance(75) { at(rng, 12) } else { None },
        ready: if rng.chance(60) { at(rng, 17) } else { None },
        hangs_after_ready: if rng.chance(15) { at(rng, 40) } else { None },
        stop: rng.pick(&[
            Stop::Clean,
            Stop::Clean,
            Stop::CompositorIgnoresTerm,
            Stop::GreetdHung,
            Stop::AtTheLimit,
            Stop::Never,
        ]),
    }
}

#[test]
fn any_order_of_failures_ends_in_a_prompt_within_the_budget() {
    for seed in 1..=1500u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let cards = rng.pick(&[
            Cards::Real,
            Cards::Real,
            Cards::Real,
            Cards::Firmware,
            Cards::FirmwareBesideDarkReal,
            Cards::None,
        ]);
        let mut scenario = Scenario {
            cards,
            cards_later: if rng.chance(15) {
                Some((rng.below(60) * SECOND, Cards::Real))
            } else {
                None
            },
            logind_from: if rng.chance(85) {
                Some(rng.below(20) * SECOND / 2)
            } else {
                None
            },
            logind_until: if rng.chance(15) {
                Some(rng.below(80) * SECOND)
            } else {
                None
            },
            can_graphical: rng.chance(85),
            plymouth_hangs: rng.chance(10),
            quit_hangs: rng.chance(10),
            account: rng.chance(90),
            starts: (0..4).map(|_| random_greeter(&mut rng)).collect(),
            automatic_login: false,
            person: None,
            refused: Vec::new(),
            nothing_after_logout: false,
            latency: rng.below(9) * SECOND / 4,
        };
        if rng.chance(5) {
            scenario.refused = vec![TextTty::Tty8];
        }
        let restarts: Vec<(u64, Micros)> = (0..rng.below(3))
            .map(|i| {
                (
                    5 + i * 25 + rng.below(20),
                    SECOND + rng.below(5) * SECOND / 4,
                )
            })
            .collect();
        let refused = !scenario.refused.is_empty();
        let run = run_with_restarts(scenario.clone(), 150, &restarts);
        let bound =
            limits::BUDGET + run.fake.downtime + if refused { limits::TEXT_LOGIN } else { 0 };
        let prompt = run.prompt_at();
        assert!(
            prompt.is_some_and(|at| at <= T0 + bound),
            "seed {seed}: prompt {:?} s, bound {} s\n{scenario:?}\nrestarts {restarts:?}\n{:?}",
            prompt.map(|at| (at - T0) as f64 / SECOND as f64),
            bound / SECOND,
            run.fake.journal
        );
        run.check_invariants();
    }
}

#[test]
fn a_stale_logind_session_does_not_hold_the_greeter_once_greetds_tree_is_empty() {
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
                scope: "session-2.scope".into(),
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

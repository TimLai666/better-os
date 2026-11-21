//! Each rule on its own, against hand-made looks. The whole loop over a
//! fake world, restarts included, is `tests/property.rs`.

use super::*;

const T0: Micros = 5 * SECOND;
const GREETER: u32 = 118;

fn real() -> Option<Drm> {
    Some(Drm {
        cards: vec![Card {
            name: "card1".into(),
            firmware: false,
            screen: true,
        }],
        entries: vec!["card1".into()],
    })
}

fn logind(sessions: Vec<LogindSession>) -> Option<Logind> {
    Some(Logind {
        can_graphical: true,
        sessions,
    })
}

fn session(class: &str, service: &str, uid: u32) -> LogindSession {
    LogindSession {
        id: "s".into(),
        uid,
        class: class.into(),
        service: service.into(),
        seat: "seat0".into(),
        kind: "wayland".into(),
    }
}

fn tree(processes: &[(u32, bool)]) -> Option<Greetd> {
    Some(Greetd {
        processes: processes
            .iter()
            .enumerate()
            .map(|(i, (uid, in_cgroup))| Process {
                pid: 100 + i as u32,
                uid: *uid,
                in_cgroup: *in_cgroup,
            })
            .collect(),
    })
}

fn good() -> Seen {
    Seen {
        drm: real(),
        logind: logind(Vec::new()),
        greetd: tree(&[]),
        heartbeat: None,
        plymouth: None,
        greeter_uid: Some(GREETER),
    }
}

fn machine() -> Machine {
    Machine {
        state: State::fresh(None, Some(T0), T0),
    }
}

/// A machine at the start of attempt `number`, `T0 + 12 s`.
fn in_attempt(number: u8) -> Machine {
    let mut m = machine();
    m.state.plymouth.deactivated = true;
    m.state.phase = Phase::Attempt {
        number,
        started: T0 + 12 * SECOND,
        baseline: None,
        session_seen: false,
        heartbeat_seen: false,
    };
    m
}

fn stops(actions: &[Action]) -> bool {
    actions.contains(&Action::StopUnit(Unit::Greeter))
}

#[test]
fn every_unit_name_is_on_the_fixed_list_once() {
    let all = [
        Unit::Greeter,
        Unit::Fallback(Tty::Tty1),
        Unit::Fallback(Tty::Tty2),
    ];
    let names: Vec<&str> = all.iter().map(|u| u.name()).collect();
    assert_eq!(names, UNITS);
}

#[test]
fn deactivate_is_sent_when_the_stages_pass_and_the_greeter_waits_for_it() {
    let mut m = machine();
    let actions = m.step(T0, &good());
    assert_eq!(actions, [Action::Plymouth(PlymouthCommand::Deactivate)]);
    // Still running: nothing is started.
    let mut seen = good();
    seen.plymouth = Some(Job::Running);
    assert!(m.step(T0 + SECOND, &seen).is_empty());
    seen.plymouth = Some(Job::Finished);
    let actions = m.step(T0 + 2 * SECOND, &seen);
    assert_eq!(
        actions,
        [
            Action::SetGreeterMode(GreeterMode::Plain),
            Action::StartUnit(Unit::Greeter)
        ]
    );
    assert!(m.state.plymouth.deactivated);
}

#[test]
fn deactivate_is_sent_at_t0_plus_7_seconds_when_the_stages_have_not_passed() {
    let mut m = machine();
    let mut seen = good();
    seen.drm = None;
    assert!(m.step(T0 + 6 * SECOND, &seen).is_empty());
    assert_eq!(
        m.step(T0 + 7 * SECOND, &seen),
        [Action::Plymouth(PlymouthCommand::Deactivate)]
    );
    // Hung: killed at its own limit, which is T0 + 12 s.
    seen.plymouth = Some(Job::Running);
    let actions = m.step(T0 + 12 * SECOND, &seen);
    assert!(actions.contains(&Action::KillPlymouth), "{actions:?}");
    // And with no card at all at 12 s, the fall back at once.
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Journal(l) if l.ends_with("REASON=no-graphics-device"))),
        "{actions:?}"
    );
}

#[test]
fn stage_5_misses_at_five_seconds_into_the_attempt_and_not_before() {
    let mut m = in_attempt(1);
    let start = T0 + 12 * SECOND;
    let mut seen = good();
    seen.greetd = tree(&[(0, true)]);
    assert!(m.step(start + 5 * SECOND - 1, &seen).is_empty());
    let actions = m.step(start + 5 * SECOND, &seen);
    assert!(stops(&actions));
    assert!(matches!(
        m.state.phase,
        Phase::Stopping {
            then: Then::Attempt { number: 2 },
            step: Step::GreeterSession,
            ..
        }
    ));
}

#[test]
fn stage_5_without_logind_is_a_second_process_in_the_cgroup() {
    let mut m = in_attempt(1);
    let start = T0 + 12 * SECOND;
    let mut seen = good();
    seen.logind = None;
    seen.greetd = tree(&[(0, true), (GREETER, true)]);
    m.step(start + SECOND, &seen);
    // Past stage 5, waiting for stage 6.
    assert!(m.step(start + 6 * SECOND, &seen).is_empty());
    let actions = m.step(start + 10 * SECOND, &seen);
    assert!(stops(&actions), "{actions:?}");
    assert!(matches!(
        m.state.phase,
        Phase::Stopping {
            step: Step::Compositor,
            ..
        }
    ));
}

#[test]
fn stage_7_misses_at_fifteen_seconds_and_the_third_miss_falls_back() {
    let mut m = in_attempt(3);
    let start = T0 + 12 * SECOND;
    let mut seen = good();
    seen.logind = logind(vec![session("greeter", "greetd-greeter", GREETER)]);
    seen.heartbeat = Some(HeartbeatFile {
        identity: 1,
        word: Word::Starting,
    });
    assert!(m.step(start + 14 * SECOND, &seen).is_empty());
    let actions = m.step(start + 15 * SECOND, &seen);
    assert!(stops(&actions));
    assert!(actions.contains(&Action::Plymouth(PlymouthCommand::Quit)));
    assert!(matches!(
        m.state.phase,
        Phase::FallingBack {
            step: Step::Greeter,
            ..
        }
    ));
}

#[test]
fn the_third_attempt_is_in_safe_mode_and_the_others_plain() {
    assert_eq!(mode(1), GreeterMode::Plain);
    assert_eq!(mode(2), GreeterMode::Plain);
    assert_eq!(mode(3), GreeterMode::Safe);
}

#[test]
fn a_ready_heartbeat_from_before_the_attempt_does_not_count() {
    let mut m = in_attempt(2);
    let old = HeartbeatFile {
        identity: 9,
        word: Word::Ready,
    };
    m.state.heartbeat = Some(Beat {
        identity: 9,
        word: Word::Ready,
        changed_at: T0,
    });
    m.state.phase = Phase::Attempt {
        number: 2,
        started: T0 + 12 * SECOND,
        baseline: Some(9),
        session_seen: true,
        heartbeat_seen: false,
    };
    let mut seen = good();
    seen.heartbeat = Some(old);
    m.step(T0 + 13 * SECOND, &seen);
    assert!(matches!(m.state.phase, Phase::Attempt { .. }));
    seen.heartbeat = Some(HeartbeatFile {
        identity: 10,
        word: Word::Ready,
    });
    let actions = m.step(T0 + 14 * SECOND, &seen);
    assert!(matches!(m.state.phase, Phase::Ready { .. }));
    assert!(actions.contains(&Action::MarkHeld));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Record(r) if matches!(
                r.restart,
                RestartOutcome::LoginScreenRestarted { restarts: 1, .. }
            ))),
        "{actions:?}"
    );
}

#[test]
fn a_person_who_may_be_there_is_never_stopped_even_at_a_deadline() {
    // The greeter's account unknown and a process of uid 1000 under
    // greetd: not certainly a person, but possibly.
    let mut m = in_attempt(1);
    let mut seen = good();
    seen.greeter_uid = None;
    seen.logind = None;
    seen.greetd = tree(&[(0, true), (1000, false)]);
    assert!(!seen.person_certain());
    assert!(seen.person_possible());
    for second in 0..40 {
        let actions = m.step(T0 + 12 * SECOND + second * SECOND, &seen);
        assert!(!stops(&actions), "{actions:?}");
    }
}

#[test]
fn a_person_seen_by_logind_or_greetds_tree_satisfies_the_attempt() {
    for seen in [
        Seen {
            logind: logind(vec![session("user", "greetd", 1000)]),
            greetd: None,
            ..good()
        },
        Seen {
            greetd: tree(&[(0, true), (0, false), (1000, false)]),
            ..good()
        },
    ] {
        let mut m = in_attempt(1);
        let actions = m.step(T0 + 13 * SECOND, &seen);
        assert!(matches!(m.state.phase, Phase::Session { .. }), "{seen:?}");
        assert!(actions.contains(&Action::Plymouth(PlymouthCommand::Quit)));
        assert!(actions.contains(&Action::MarkHeld));
    }
    // A text login on tty3 is a person, but not greetd's.
    let tty = Seen {
        logind: logind(vec![LogindSession {
            seat: "seat0".into(),
            ..session("user", "login", 1000)
        }]),
        ..good()
    };
    assert!(!tty.person_certain());
}

#[test]
fn a_stop_that_has_not_emptied_the_cgroup_in_six_seconds_starts_tty2() {
    let mut m = machine();
    m.state.phase = Phase::Stopping {
        then: Then::Attempt { number: 2 },
        deadline: T0 + 30 * SECOND,
        step: Step::Compositor,
    };
    let mut seen = good();
    seen.greetd = tree(&[(0, true)]);
    assert!(m.step(T0 + 29 * SECOND, &seen).is_empty());
    let actions = m.step(T0 + 30 * SECOND, &seen);
    let start = actions
        .iter()
        .position(|a| *a == Action::StartUnit(Unit::Fallback(Tty::Tty2)))
        .expect("tty2 started");
    let marker = actions
        .iter()
        .position(|a| *a == Action::MarkFallbackActive(Tty::Tty2))
        .unwrap();
    assert!(marker < start, "the marker comes before the start");
    assert!(actions.contains(&Action::ResetTty(Tty::Tty1)));
    assert!(actions.contains(&Action::ResetTty(Tty::Tty2)));
    assert!(actions.contains(&Action::ActivateVt(Tty::Tty2)));
    assert!(!actions.contains(&Action::StartUnit(Unit::Greeter)));
}

#[test]
fn an_unknown_cgroup_is_never_taken_for_a_stopped_unit() {
    let mut m = machine();
    m.state.phase = Phase::FallingBack {
        step: Step::Greeter,
        deadline: T0 + 30 * SECOND,
    };
    let mut seen = good();
    seen.greetd = None;
    assert!(m.step(T0 + 29 * SECOND, &seen).is_empty());
    let actions = m.step(T0 + 30 * SECOND, &seen);
    assert!(actions.contains(&Action::StartUnit(Unit::Fallback(Tty::Tty2))));
}

#[test]
fn a_restart_asks_again_for_what_it_may_have_been_cut_off_asking() {
    let mut attempt = in_attempt(3).state;
    attempt.boot_id = Some("b".repeat(32));
    let (m, actions) = start(
        Loaded::Found(Box::new(attempt.clone())),
        Some("b".repeat(32)),
        Some(T0 + 50 * SECOND),
        T0 + 50 * SECOND,
    );
    assert_eq!(m.state, attempt, "the same appearance and the same clock");
    assert_eq!(
        &actions[1..],
        [
            Action::SetGreeterMode(GreeterMode::Safe),
            Action::StartUnit(Unit::Greeter)
        ]
    );
    let mut stopping = attempt.clone();
    stopping.phase = Phase::Stopping {
        then: Then::Attempt { number: 3 },
        deadline: T0,
        step: Step::Greeter,
    };
    let (_, actions) = start(Loaded::Found(Box::new(stopping)), None, None, T0);
    assert_eq!(actions[1..], [Action::StopUnit(Unit::Greeter)]);
    // Another boot's state is not this boot's.
    let (m, actions) = start(
        Loaded::Found(Box::new(attempt)),
        Some("c".repeat(32)),
        Some(T0),
        T0 + SECOND,
    );
    assert!(actions.is_empty());
    assert_eq!(m.state.phase, Phase::Checking { baseline: None });
    assert_eq!(m.state.t0, T0, "T0 is when the unit began to start");
}

#[test]
fn a_plymouth_command_cut_off_by_a_restart_is_sent_again_or_killed_by_its_first_deadline() {
    let mut m = machine();
    m.state.plymouth.job = Some(PlymouthJob {
        command: PlymouthCommand::Deactivate,
        deadline: T0 + 9 * SECOND,
    });
    let mut seen = good();
    seen.drm = None;
    // The edges know of no command: sent again.
    let actions = m.step(T0 + 8 * SECOND, &seen);
    assert!(actions.contains(&Action::Plymouth(PlymouthCommand::Deactivate)));
    assert_eq!(m.state.plymouth.job.unwrap().deadline, T0 + 9 * SECOND);
    // Past its deadline with nothing known: killed, not sent a third time.
    let actions = m.step(T0 + 9 * SECOND, &seen);
    assert!(actions.contains(&Action::KillPlymouth), "{actions:?}");
    assert!(m.state.plymouth.job.is_none());
}

#[test]
fn an_unreadable_state_falls_back_unless_a_person_or_a_ready_greeter_is_there() {
    let unreadable = || start(Loaded::Unreadable, None, Some(T0), T0 + 30 * SECOND).0;
    let mut m = unreadable();
    m.step(T0 + 30 * SECOND, &good());
    assert!(matches!(
        m.state.phase,
        Phase::FallingBack {
            step: Step::Supervisor,
            ..
        }
    ));
    let mut m = unreadable();
    let mut seen = good();
    seen.greetd = tree(&[(0, true), (1000, false)]);
    m.step(T0 + 30 * SECOND, &seen);
    assert!(matches!(m.state.phase, Phase::Session { .. }));
    let mut m = unreadable();
    let mut seen = good();
    seen.heartbeat = Some(HeartbeatFile {
        identity: 3,
        word: Word::Ready,
    });
    m.step(T0 + 30 * SECOND, &seen);
    assert!(matches!(m.state.phase, Phase::Ready { .. }));
    // ...and held to it: thirty seconds without a change is a restart.
    seen.greetd = tree(&[(0, true), (GREETER, true)]);
    let actions = m.step(T0 + 61 * SECOND, &seen);
    assert!(stops(&actions), "{actions:?}");
}

#[test]
fn a_session_ends_when_greetds_tree_says_so_whatever_logind_lists() {
    let mut m = machine();
    m.state.phase = Phase::Session { since: T0 };
    let stale = Seen {
        logind: logind(vec![session("user", "greetd", 1000)]),
        greetd: tree(&[(0, true), (1000, false)]),
        ..good()
    };
    m.step(T0 + SECOND, &stale);
    assert!(matches!(m.state.phase, Phase::Session { .. }));
    let ended = Seen {
        greetd: tree(&[(0, true)]),
        ..stale
    };
    m.step(T0 + 2 * SECOND, &ended);
    assert_eq!(m.state.appearance, 2);
    assert_eq!(m.state.t0, T0 + 2 * SECOND);
    assert!(matches!(m.state.phase, Phase::Checking { .. }));
    // With neither readable, it has not ended.
    let mut m = machine();
    m.state.phase = Phase::Session { since: T0 };
    m.step(
        T0 + SECOND,
        &Seen {
            logind: None,
            greetd: None,
            ..good()
        },
    );
    assert!(matches!(m.state.phase, Phase::Session { .. }));
}

#[test]
fn journal_lines_carry_the_stage_the_result_and_the_reason() {
    let note = Note {
        step: Step::DriverLate,
        outcome: Outcome::Ok,
        reason: Reason::RealDriverBound,
    };
    assert_eq!(
        note.journal_line(),
        "BETTER_OS_LOGIN_STAGE=1a RESULT=ok REASON=real-driver-bound"
    );
    assert_eq!(Step::Compositor.stage(), LoginStage::DisplayServer);
    assert_eq!(Step::Supervisor.stage(), LoginStage::Supervisor);
}

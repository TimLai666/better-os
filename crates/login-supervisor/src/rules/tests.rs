//! Each rule on its own, against hand-made looks. The whole loop over a
//! fake world, restarts included, is `tests/property.rs`.

use super::*;

const T0: Micros = 5 * SECOND;
const GREETER: u32 = 118;
const SCOPE: &str = "/user.slice/user-118.slice/session-c1.scope";

fn card(name: &str, firmware: bool, screen: bool) -> Card {
    Card {
        name: name.into(),
        firmware,
        screen,
    }
}

fn drm(cards: Vec<Card>) -> Option<Drm> {
    Some(Drm {
        entries: cards.iter().map(|c| c.name.clone()).collect(),
        cards,
    })
}

fn real() -> Option<Drm> {
    drm(vec![card("card1", false, true)])
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
        scope: "session-s.scope".into(),
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

fn scope(path: &str, uids: &[u32]) -> ScopeSeen {
    ScopeSeen {
        path: path.into(),
        greeter_session: false,
        processes: Some(
            uids.iter()
                .enumerate()
                .map(|(i, uid)| Process {
                    pid: 200 + i as u32,
                    uid: *uid,
                    in_cgroup: false,
                })
                .collect(),
        ),
    }
}

fn good() -> Seen {
    Seen {
        drm: real(),
        logind: logind(Vec::new()),
        greetd: tree(&[]),
        scopes: Vec::new(),
        drm_holders: Some(Vec::new()),
        text_login_active: None,
        heartbeat: None,
        plymouth: None,
        greeter_uid: Some(GREETER),
    }
}

fn unit() -> UnitStart {
    UnitStart {
        began: Some(T0),
        restarts: Some(0),
    }
}

fn machine() -> Machine {
    Machine {
        state: State::fresh(None, unit(), T0),
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

fn starts_text_login(actions: &[Action]) -> Option<TextTty> {
    actions.iter().find_map(|a| match a {
        Action::StartUnit(Unit::Fallback(tty)) => Some(*tty),
        _ => None,
    })
}

fn journal(actions: &[Action], suffix: &str) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, Action::Journal(l) if l.ends_with(suffix)))
}

#[test]
fn every_unit_name_is_on_the_fixed_list_once() {
    let all = [
        Unit::Greeter,
        Unit::Fallback(TextTty::Tty8),
        Unit::Fallback(TextTty::Tty9),
    ];
    let names: Vec<&str> = all.iter().map(|u| u.name()).collect();
    assert_eq!(names, UNITS);
}

#[test]
fn only_a_session_scopes_shape_is_ever_acted_on() {
    assert!(is_session_scope(SCOPE));
    assert!(is_session_scope(
        "/user.slice/user-1000.slice/session-2.scope"
    ));
    for path in [
        "/system.slice/better-os-greeter.service",
        "/user.slice/user-118.slice/user@118.service",
        "/user.slice/user-118.slice/session-c1.scope/x",
        "/user.slice/user-.slice/session-c1.scope",
        "/user.slice/user-118.slice/session-.scope",
        "/user.slice/user-118.slice/session-../../.scope",
        "user.slice/user-118.slice/session-c1.scope",
    ] {
        assert!(!is_session_scope(path), "{path}");
    }
}

#[test]
fn no_screen_is_said_only_when_no_card_at_all_reports_one() {
    let of = |cards| Cards::of(drm(cards).as_ref());
    assert_eq!(of(vec![card("card1", false, true)]), Cards::RealWithScreen);
    // A real card with no screen beside the firmware's framebuffer that
    // shows the picture: the framebuffer is used, with the usual attempts.
    assert_eq!(
        of(vec![card("card0", true, true), card("card1", false, false)]),
        Cards::FirmwareWithScreen
    );
    assert_eq!(
        of(vec![card("card0", true, true)]),
        Cards::FirmwareWithScreen
    );
    assert_eq!(
        of(vec![card("card0", true, true), card("card1", false, true)]),
        Cards::RealWithScreen
    );
    assert_eq!(of(vec![card("card1", false, false)]), Cards::NoScreen);
    assert_eq!(
        of(vec![
            card("card0", true, false),
            card("card1", false, false)
        ]),
        Cards::NoScreen
    );
    assert_eq!(of(vec![]), Cards::None);
    assert_eq!(Cards::of(None), Cards::None);
}

#[test]
fn every_combination_of_cards_starts_the_attempts_or_falls_back_and_none_marks_held() {
    for (cards, firmware, no_screen) in [
        (
            vec![card("card0", true, true), card("card1", false, false)],
            true,
            false,
        ),
        (vec![card("card0", true, true)], true, false),
        (vec![card("card1", false, false)], false, true),
        (vec![card("card0", true, false)], false, true),
    ] {
        let mut m = machine();
        let mut seen = good();
        seen.drm = drm(cards.clone());
        m.step(T0 + 7 * SECOND, &seen);
        seen.plymouth = Some(Job::Finished);
        let actions = m.step(T0 + 12 * SECOND, &seen);
        assert!(
            matches!(m.state.phase, Phase::Attempt { number: 1, .. }),
            "{cards:?}: {:?}",
            m.state.phase
        );
        assert!(!actions.contains(&Action::MarkHeld), "{cards:?}");
        assert_eq!(m.state.on_firmware, firmware, "{cards:?}");
        assert_eq!(m.state.no_screen, no_screen, "{cards:?}");
        // The deadlines run: stage 5 misses five seconds later.
        let actions = m.step(T0 + 17 * SECOND, &seen);
        assert!(stops(&actions), "{cards:?}");
    }
}

#[test]
fn deactivate_is_sent_when_the_stages_pass_and_the_greeter_waits_for_it() {
    let mut m = machine();
    let actions = m.step(T0, &good());
    assert_eq!(actions, [Action::Plymouth(PlymouthCommand::Deactivate)]);
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
    seen.plymouth = Some(Job::Running);
    let actions = m.step(T0 + 12 * SECOND, &seen);
    assert!(actions.contains(&Action::KillPlymouth), "{actions:?}");
    assert!(
        journal(&actions, "REASON=no-graphics-device"),
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
fn stage_5_without_logind_is_the_greeters_scope_or_a_second_process_under_greetd() {
    for seen in [
        Seen {
            logind: None,
            greetd: tree(&[(0, true), (0, false), (GREETER, false)]),
            ..good()
        },
        Seen {
            logind: None,
            greetd: tree(&[(0, true)]),
            scopes: vec![scope(SCOPE, &[0, GREETER])],
            ..good()
        },
    ] {
        let mut m = in_attempt(1);
        let start = T0 + 12 * SECOND;
        m.step(start + SECOND, &seen);
        assert!(m.step(start + 6 * SECOND, &seen).is_empty(), "{seen:?}");
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
    seen.heartbeat = Some(HeartbeatFile {
        identity: 9,
        word: Word::Ready,
    });
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

/// A machine stopping the greeter for its second attempt since `issued`,
/// with the greeter's session scope remembered.
fn stopping(issued: Micros) -> Machine {
    let mut m = machine();
    m.state.scopes.push(SCOPE.into());
    m.state.phase = Phase::Stopping {
        then: Then::Attempt { number: 2 },
        issued,
        step: Step::Compositor,
    };
    m
}

#[test]
fn a_greeter_session_scope_that_outlives_greetd_is_killed_and_waited_for() {
    // greetd's unit empties at once; the compositor in its session's scope
    // ignores SIGTERM.
    let issued = T0 + 20 * SECOND;
    let mut m = stopping(issued);
    let mut seen = good();
    seen.scopes = vec![scope(SCOPE, &[0, GREETER])];
    seen.drm_holders = Some(vec![201]);
    let actions = m.step(issued + SECOND, &seen);
    assert!(
        actions.is_empty(),
        "nothing starts while the scope runs: {actions:?}"
    );
    let actions = m.step(issued + 4 * SECOND, &seen);
    assert!(
        actions.contains(&Action::KillScope {
            path: SCOPE.into(),
            greeter: GREETER
        }),
        "{actions:?}"
    );
    assert!(journal(&actions, "REASON=session-killed"));
    // Its processes are gone, but a card is still held: still waiting.
    seen.scopes = vec![scope(SCOPE, &[])];
    assert!(
        !m.step(issued + 5 * SECOND, &seen)
            .contains(&Action::StartUnit(Unit::Greeter))
    );
    seen.drm_holders = Some(Vec::new());
    let actions = m.step(issued + 5 * SECOND + TICK_FOR_TEST, &seen);
    assert!(
        actions.contains(&Action::StartUnit(Unit::Greeter)),
        "{actions:?}"
    );
    assert!(m.state.scopes.is_empty());
}

const TICK_FOR_TEST: Micros = limits::TICK;

#[test]
fn a_scope_with_a_persons_process_is_never_killed() {
    let issued = T0 + 20 * SECOND;
    let mut m = stopping(issued);
    let mut seen = good();
    seen.scopes = vec![scope(SCOPE, &[0, GREETER, 1000])];
    let actions = m.step(issued + 4 * SECOND, &seen);
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::KillScope { .. })),
        "{actions:?}"
    );
}

#[test]
fn a_greeter_that_cannot_be_killed_gets_the_text_login_at_the_stops_deadline() {
    let issued = T0 + 20 * SECOND;
    let mut m = stopping(issued);
    let mut seen = good();
    seen.scopes = vec![scope(SCOPE, &[GREETER])];
    seen.drm_holders = Some(vec![200]);
    assert!(starts_text_login(&m.step(issued + 5 * SECOND, &seen)).is_none());
    let actions = m.step(issued + 6 * SECOND, &seen);
    assert_eq!(starts_text_login(&actions), Some(TextTty::Tty8));
    assert!(!actions.contains(&Action::StartUnit(Unit::Greeter)));
    assert!(journal(&actions, "REASON=stop-did-not-complete"));
    assert!(actions.contains(&Action::ResetVt(Vt::Tty1)));
    // Unknown is not gone either.
    let mut m = stopping(issued);
    seen.scopes = vec![ScopeSeen {
        processes: None,
        ..scope(SCOPE, &[])
    }];
    seen.drm_holders = None;
    assert!(starts_text_login(&m.step(issued + 6 * SECOND, &seen)).is_some());
}

#[test]
fn the_text_login_is_marked_only_once_seen_active() {
    let mut m = machine();
    let mut out = Vec::new();
    m.start_text_login(T0 + 50 * SECOND, Step::Compositor, TextTty::Tty8, &mut out);
    assert!(!out.contains(&Action::MarkHeld));
    assert!(
        !out.iter()
            .any(|a| matches!(a, Action::MarkFallbackActive(_)))
    );
    assert_eq!(m.needs().text_login, Some(TextTty::Tty8));
    let mut seen = good();
    seen.plymouth = Some(Job::Finished);
    seen.text_login_active = Some(false);
    assert!(m.step(T0 + 51 * SECOND, &seen).is_empty());
    // Asked again once, then the other terminal.
    let again = m.step(T0 + 51 * SECOND + SECOND / 2, &seen);
    assert_eq!(again, [Action::StartUnit(Unit::Fallback(TextTty::Tty8))]);
    let other = m.step(T0 + 53 * SECOND, &seen);
    assert_eq!(starts_text_login(&other), Some(TextTty::Tty9));
    assert!(journal(&other, "REASON=text-login-not-active"));
    seen.text_login_active = Some(true);
    let confirmed = m.step(T0 + 54 * SECOND, &seen);
    assert!(confirmed.contains(&Action::MarkFallbackActive(TextTty::Tty9)));
    assert!(confirmed.contains(&Action::MarkHeld));
    assert!(confirmed.iter().any(|a| matches!(a, Action::Record(_))));
    assert_eq!(m.needs().text_login, None);
}

#[test]
fn a_text_login_that_never_starts_keeps_trying_without_ever_silencing_the_backstop() {
    let mut m = machine();
    let mut out = Vec::new();
    m.start_text_login(T0, Step::Greeter, TextTty::Tty8, &mut out);
    let mut seen = good();
    seen.plymouth = Some(Job::Finished);
    seen.text_login_active = Some(false);
    let mut now = T0;
    let mut switches = 0;
    while now < T0 + 600 * SECOND {
        now += limits::TICK;
        let actions = m.step(now, &seen);
        assert!(!actions.contains(&Action::MarkHeld));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::MarkFallbackActive(_)))
        );
        switches += actions
            .iter()
            .filter(|a| matches!(a, Action::ActivateVt(_)))
            .count();
    }
    // Every 3 s at first, then every 30 s.
    assert!((20..40).contains(&switches), "{switches}");
}

#[test]
fn a_person_who_may_be_there_is_never_stopped_and_the_cut_off_still_gives_a_text_login() {
    let mut m = in_attempt(1);
    let mut seen = good();
    seen.greeter_uid = None;
    seen.logind = logind(Vec::new());
    seen.greetd = tree(&[(0, true), (1000, false)]);
    assert!(!seen.person_certain());
    assert!(seen.person_possible());
    let mut text_login = None;
    let mut now = T0 + 12 * SECOND;
    while now < T0 + limits::FALL_BACK_BY + SECOND {
        let actions = m.step(now, &seen);
        assert!(!stops(&actions), "{actions:?}");
        text_login = text_login.or(starts_text_login(&actions));
        now += limits::TICK;
    }
    assert_eq!(text_login, Some(TextTty::Tty8), "the cut-off gave none");
}

#[test]
fn with_neither_greetds_tree_nor_logind_readable_the_unit_is_never_stopped() {
    let mut m = in_attempt(1);
    let seen = Seen {
        logind: None,
        greetd: None,
        ..good()
    };
    assert!(seen.sessions_unknown());
    let actions = m.step(T0 + 12 * SECOND + 5 * SECOND, &seen);
    assert!(!stops(&actions), "{actions:?}");
    assert_eq!(starts_text_login(&actions), Some(TextTty::Tty8));
    assert!(journal(&actions, "REASON=sessions-unknown"));
}

#[test]
fn a_stale_heartbeat_whose_sessions_cannot_be_told_apart_ends_in_a_text_login_unstopped() {
    let mut m = machine();
    m.state.phase = Phase::Ready { since: T0 };
    m.state.heartbeat = Some(Beat {
        identity: 1,
        word: Word::Ready,
        changed_at: T0,
    });
    let mut seen = good();
    seen.greeter_uid = None;
    seen.logind = logind(Vec::new());
    seen.greetd = tree(&[(0, true), (118, false)]);
    let mut text_login = None;
    let mut now = T0;
    while now < T0 + 70 * SECOND {
        let actions = m.step(now, &seen);
        assert!(!stops(&actions));
        text_login = text_login.or(starts_text_login(&actions).map(|t| (t, now)));
        now += limits::TICK;
    }
    let (tty, at) = text_login.expect("no text login");
    assert_eq!(tty, TextTty::Tty8);
    assert!(at >= T0 + limits::HEARTBEAT_STALE + limits::UNSURE_GRACE);
}

#[test]
fn the_fall_back_begins_by_t0_plus_68_seconds_however_the_time_went() {
    // A slow machine: still in the second attempt at the cut-off.
    let mut m = in_attempt(2);
    m.state.phase = Phase::Attempt {
        number: 2,
        started: T0 + 60 * SECOND,
        baseline: None,
        session_seen: true,
        heartbeat_seen: true,
    };
    let mut seen = good();
    seen.greetd = tree(&[(0, true), (GREETER, false)]);
    assert!(m.step(T0 + 67 * SECOND, &seen).is_empty());
    let actions = m.step(T0 + 68 * SECOND, &seen);
    assert!(stops(&actions));
    assert!(journal(&actions, "RESULT=fell-back REASON=out-of-time"));
    assert!(matches!(m.state.phase, Phase::FallingBack { .. }));
    // In the middle of a stop for another attempt: the stop goes on, the
    // text login follows it.
    let mut m = stopping(T0 + 66 * SECOND);
    let actions = m.step(T0 + 68 * SECOND, &seen);
    assert!(
        matches!(m.state.phase, Phase::FallingBack { .. }),
        "{actions:?}"
    );
}

#[test]
fn a_stop_that_has_not_emptied_the_unit_in_six_seconds_starts_the_text_login_at_once() {
    let mut m = machine();
    m.state.phase = Phase::Stopping {
        then: Then::Attempt { number: 2 },
        issued: T0 + 24 * SECOND,
        step: Step::Compositor,
    };
    let mut seen = good();
    seen.greetd = tree(&[(0, true)]);
    assert!(m.step(T0 + 29 * SECOND, &seen).is_empty());
    let actions = m.step(T0 + 30 * SECOND, &seen);
    assert_eq!(starts_text_login(&actions), Some(TextTty::Tty8));
    assert!(actions.contains(&Action::ResetVt(Vt::Tty1)));
    assert!(actions.contains(&Action::ResetVt(Vt::Tty8)));
    assert!(actions.contains(&Action::ActivateVt(TextTty::Tty8)));
    assert!(!actions.contains(&Action::StartUnit(Unit::Greeter)));
}

#[test]
fn an_unknown_cgroup_is_never_taken_for_a_stopped_unit() {
    let mut m = machine();
    m.state.phase = Phase::FallingBack {
        step: Step::Greeter,
        issued: T0 + 24 * SECOND,
    };
    let mut seen = good();
    seen.greetd = None;
    assert!(m.step(T0 + 29 * SECOND, &seen).is_empty());
    let actions = m.step(T0 + 30 * SECOND, &seen);
    assert_eq!(starts_text_login(&actions), Some(TextTty::Tty8));
}

#[test]
fn the_supervisors_own_restart_continues_and_any_other_start_begins_anew() {
    let mut attempt = in_attempt(3).state;
    attempt.boot_id = Some("b".repeat(32));
    attempt.unit_restarts = Some(0);
    attempt.plymouth.quit = true;
    let restarted = UnitStart {
        began: Some(T0 + 50 * SECOND),
        restarts: Some(1),
    };
    let (m, actions) = start(
        Loaded::Found(Box::new(attempt.clone())),
        Some("b".repeat(32)),
        restarted,
        T0 + 50 * SECOND,
    );
    assert_eq!(m.state.phase, attempt.phase, "the same appearance");
    assert_eq!(m.state.t0, attempt.t0, "and the same clock");
    assert_eq!(m.state.unit_restarts, Some(1));
    assert_eq!(
        &actions[1..],
        [
            Action::SetGreeterMode(GreeterMode::Safe),
            Action::StartUnit(Unit::Greeter)
        ]
    );
    // `systemctl restart display-manager`: systemd did not count it.
    let again = UnitStart {
        began: Some(T0 + 300 * SECOND),
        restarts: Some(0),
    };
    let (m, actions) = start(
        Loaded::Found(Box::new(attempt.clone())),
        Some("b".repeat(32)),
        again,
        T0 + 301 * SECOND,
    );
    assert_eq!(m.state.appearance, 2);
    assert_eq!(m.state.t0, T0 + 300 * SECOND);
    assert!(matches!(m.state.phase, Phase::Checking { .. }));
    assert!(m.state.plymouth.quit, "what Plymouth did stays done");
    assert!(journal(&actions, "REASON=started-again"));
    // A state of a text login started again: the greeter is tried again.
    let mut fell_back = attempt.clone();
    fell_back.phase = Phase::TextLogin {
        step: Step::Greeter,
        tty: TextTty::Tty8,
        started: T0,
        starts: 1,
        confirmed: true,
    };
    let (m, _) = start(
        Loaded::Found(Box::new(fell_back)),
        None,
        again,
        T0 + 301 * SECOND,
    );
    assert!(matches!(m.state.phase, Phase::Checking { .. }));
    // Another boot's state is not this boot's.
    let (m, actions) = start(
        Loaded::Found(Box::new(attempt)),
        Some("c".repeat(32)),
        unit(),
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
    let actions = m.step(T0 + 8 * SECOND, &seen);
    assert!(actions.contains(&Action::Plymouth(PlymouthCommand::Deactivate)));
    assert_eq!(m.state.plymouth.job.unwrap().deadline, T0 + 9 * SECOND);
    let actions = m.step(T0 + 9 * SECOND, &seen);
    assert!(actions.contains(&Action::KillPlymouth), "{actions:?}");
    assert!(m.state.plymouth.job.is_none());
}

#[test]
fn an_unreadable_state_falls_back_unless_a_person_or_a_ready_greeter_is_there() {
    let unreadable = || start(Loaded::Unreadable, None, unit(), T0 + 30 * SECOND).0;
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
    seen.greetd = tree(&[(0, true), (GREETER, false)]);
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
fn a_screen_connected_after_a_text_login_on_a_machine_with_none_brings_the_greeter_back() {
    let mut m = machine();
    m.state.no_screen = true;
    m.state.phase = Phase::TextLogin {
        step: Step::Greeter,
        tty: TextTty::Tty8,
        started: T0,
        starts: 1,
        confirmed: true,
    };
    let mut seen = good();
    seen.drm = drm(vec![card("card1", false, false)]);
    assert!(m.step(T0 + 100 * SECOND, &seen).is_empty());
    // Someone signed in on the text login: it stays.
    seen.drm = real();
    seen.logind = logind(vec![LogindSession {
        service: "login".into(),
        ..session("user", "login", 1000)
    }]);
    assert!(m.step(T0 + 101 * SECOND, &seen).is_empty());
    seen.logind = logind(Vec::new());
    let actions = m.step(T0 + 102 * SECOND, &seen);
    assert!(actions.contains(&Action::StopUnit(Unit::Fallback(TextTty::Tty8))));
    assert!(actions.contains(&Action::ClearFallbackActive));
    assert!(matches!(m.state.phase, Phase::Checking { .. }));
    assert_eq!(m.state.t0, T0 + 102 * SECOND);
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

use super::*;
use crate::sessions::parse_session;
use app_catalog_core::NoProbe;

const SECOND: Millis = 1000;

fn account(user: &str, real: &str, login: i64, session: &str) -> Account {
    Account {
        user_name: user.into(),
        real_name: real.into(),
        login_time: login,
        session: session.into(),
        language: String::new(),
        icon_file: String::new(),
    }
}

fn sessions() -> Vec<Session> {
    let better =
        "[Desktop Entry]\nName=Better OS\nExec=better-os-session\nDesktopNames=BetterOS;COSMIC;\n";
    let zorin = "[Desktop Entry]\nName=Zorin OS\nExec=env GNOME_SHELL_SESSION_MODE=zorin /usr/bin/gnome-session --session=zorin\n";
    let sway = "[Desktop Entry]\nName=Sway\nExec=sway\n";
    vec![
        parse_session("better-os", better.as_bytes(), &NoProbe)
            .unwrap()
            .unwrap(),
        parse_session("sway", sway.as_bytes(), &NoProbe)
            .unwrap()
            .unwrap(),
        parse_session("zorin", zorin.as_bytes(), &NoProbe)
            .unwrap()
            .unwrap(),
    ]
}

fn login() -> Screen {
    Screen::login(
        vec![
            account("lin", "林小明", 200, "sway"),
            account("wang", "王大同", 100, ""),
        ],
        sessions(),
        Some("zh_TW.UTF-8".into()),
        0,
    )
}

fn lock() -> Screen {
    Screen::lock(
        account("lin", "林小明", 200, ""),
        Some("zh_TW.UTF-8".into()),
        false,
        0,
    )
}

/// The requests in words, conversation ids left out: `begin lin`,
/// `answer`, `cancel`, …
fn said(effects: &Effects) -> Vec<String> {
    effects
        .requests
        .iter()
        .map(|request| match request {
            Request::Begin { user, .. } => format!("begin {user}"),
            Request::Answer => "answer".into(),
            Request::Acknowledge => "acknowledge".into(),
            Request::Cancel => "cancel".into(),
            Request::StartSession { session_id } => format!("start {session_id}"),
            Request::Power(action) => format!("power {action:?}"),
        })
        .collect()
}

fn current(screen: &Screen) -> ConversationId {
    screen.conversation().expect("a conversation runs")
}

fn prompt(screen: &mut Screen, kind: PromptKind, text: &str, now: Millis) -> Effects {
    let conversation = current(screen);
    screen.handle(
        Event::Prompt {
            conversation,
            kind,
            text: text.into(),
        },
        now,
    )
}

/// PAM asks for the password.
fn ask(screen: &mut Screen, now: Millis) -> Effects {
    prompt(screen, PromptKind::Secret, "Password: ", now)
}

fn message(screen: &mut Screen, kind: PamMessageKind, text: &str, now: Millis) -> Effects {
    let conversation = current(screen);
    screen.handle(
        Event::PamMessage {
            conversation,
            kind,
            text: text.into(),
        },
        now,
    )
}

fn refuse(screen: &mut Screen, refusal: Refusal, now: Millis) -> Effects {
    let conversation = current(screen);
    screen.handle(
        Event::Refused {
            conversation,
            refusal,
        },
        now,
    )
}

fn authenticate(screen: &mut Screen, now: Millis) -> Effects {
    let conversation = current(screen);
    screen.handle(Event::Authenticated { conversation }, now)
}

/// Shows the login screen and has PAM ask for the password.
fn ready_login(now: Millis) -> Screen {
    let mut screen = login();
    let shown = screen.handle(Event::Shown, now);
    assert_eq!(said(&shown), ["begin lin"]);
    assert!(shown.focus_field);
    ask(&mut screen, now);
    screen
}

fn fail(screen: &mut Screen, now: Millis) -> Effects {
    screen.handle(Event::Submit, now);
    refuse(screen, Refusal::WrongPassword, now)
}

#[test]
fn the_login_screen_opens_in_the_entry_view_for_the_last_account() {
    let screen = ready_login(0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(screen.account().unwrap().user_name, "lin");
    assert_eq!(screen.caption(), None);
    assert_eq!(screen.submit_label(), "登入");
    assert_eq!(screen.field_label(), "密碼");
    assert_eq!(screen.field_accessible_name(), "林小明 的密碼");
    assert_eq!(screen.screen_name(), "登入畫面");
    assert!(screen.field_enabled());
    assert!(screen.field_is_secret());
    assert!(screen.shows_switch_account());
    // The account's last session is preselected.
    assert_eq!(screen.chosen_session(), Some("sway"));
    assert_eq!(screen.session_button_label().unwrap(), "工作階段：Sway");
    assert_eq!(screen.message_line(0), None);
}

#[test]
fn submitting_answers_the_prompt_and_wipes_the_field() {
    let mut screen = ready_login(0);
    let effects = screen.handle(Event::Submit, 10);
    assert_eq!(said(&effects), ["answer"]);
    assert!(effects.wipe_field);
    assert_eq!(screen.phase(), Phase::Checking { since: 10 });
    assert!(screen.busy());
    assert!(!screen.field_enabled());
    assert_eq!(screen.caption(), Some("正在確認…"));
    // A second submit while checking sends nothing.
    assert!(screen.handle(Event::Submit, 20).requests.is_empty());
}

#[test]
fn typing_before_the_question_arrives_answers_it_when_it_does() {
    let mut screen = login();
    screen.handle(Event::Shown, 0);
    let early = screen.handle(Event::Submit, 5);
    assert!(early.requests.is_empty());
    assert!(
        !early.wipe_field,
        "the typed answer is kept for the question"
    );
    assert!(screen.busy());
    let asked = ask(&mut screen, 6);
    assert_eq!(said(&asked), ["answer"]);
    assert!(asked.wipe_field);
}

#[test]
fn a_visible_question_wipes_what_was_typed_hidden() {
    // Typed and queued before the question, which turns out to be shown.
    let mut screen = login();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Submit, 5);
    let shown = prompt(&mut screen, PromptKind::Visible, "Token code: ", 6);
    assert!(shown.wipe_field, "a queued answer must not appear in clear");
    assert!(
        shown.requests.is_empty(),
        "nor be sent to a visible question"
    );
    assert!(!screen.field_is_secret());
    assert_eq!(screen.phase(), Phase::Ready);

    // Typed without Enter before the question.
    let mut screen = login();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::FieldEdited, 1);
    let shown = prompt(&mut screen, PromptKind::Visible, "Token code: ", 2);
    assert!(shown.wipe_field);

    // A hidden question after a visible one keeps what was typed for it.
    let mut screen = ready_login(0);
    screen.handle(Event::FieldEdited, 1);
    let hidden = ask(&mut screen, 2);
    assert!(!hidden.wipe_field);
}

#[test]
fn an_abandoned_queued_answer_is_wiped() {
    let mut screen = login();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Submit, 5);
    // greetd never asks; the person goes to choose another account.
    let tick = screen.handle(Event::Tick, 5 + 60 * SECOND);
    assert!(tick.wipe_field);
    assert_eq!(said(&tick), ["cancel"]);
}

#[test]
fn a_wrong_password_clears_the_field_and_keeps_focus() {
    let mut screen = ready_login(0);
    let effects = fail(&mut screen, SECOND);
    assert!(effects.wipe_field);
    assert!(effects.focus_field);
    assert!(effects.attempts_changed);
    assert_eq!(said(&effects), ["cancel", "begin lin"]);
    assert_eq!(screen.phase(), Phase::Ready);
    assert_eq!(screen.attempts().get("lin").failures, 1);
    let line = screen.message_line(SECOND).unwrap();
    assert_eq!(line.kind, MessageKind::Error);
    assert_eq!(line.text, "密碼不正確，請再試一次。");
    // Typing again clears the error.
    screen.handle(Event::FieldEdited, 2 * SECOND);
    assert_eq!(screen.message_line(2 * SECOND), None);
}

#[test]
fn the_doubling_wait_from_the_second_failure() {
    let mut screen = ready_login(0);
    fail(&mut screen, 0);
    ask(&mut screen, 0);
    let second = fail(&mut screen, 10 * SECOND);
    assert_eq!(
        said(&second),
        ["cancel"],
        "no new conversation during the wait"
    );
    assert_eq!(screen.phase(), Phase::Waiting { until: 11 * SECOND });
    assert!(!screen.field_enabled());
    assert_eq!(
        screen.message_line(10 * SECOND).unwrap().text,
        "密碼不正確。請在 1 秒後再試一次。"
    );
    let ended = screen.handle(Event::Tick, 11 * SECOND);
    assert_eq!(said(&ended), ["begin lin"]);
    assert_eq!(screen.phase(), Phase::Ready);
    ask(&mut screen, 11 * SECOND);
    // The third failure in a row waits 2 seconds, the seventh 30.
    fail(&mut screen, 20 * SECOND);
    assert_eq!(screen.phase(), Phase::Waiting { until: 22 * SECOND });
    assert_eq!(
        screen.message_line(20 * SECOND).unwrap().text,
        "密碼不正確。請在 2 秒後再試一次。"
    );
    let mut now = 22 * SECOND;
    for _ in 0..4 {
        screen.handle(Event::Tick, now + 30 * SECOND);
        now += 30 * SECOND;
        ask(&mut screen, now);
        fail(&mut screen, now);
    }
    assert_eq!(
        screen.phase(),
        Phase::Waiting {
            until: now + 30 * SECOND
        }
    );
}

/// Two failures for lin, so a 1 s wait from `now` (and 2 s for the next).
fn waiting_login(now: Millis) -> Screen {
    let mut screen = ready_login(0);
    fail(&mut screen, 0);
    ask(&mut screen, 0);
    screen.handle(Event::Tick, now);
    fail(&mut screen, now);
    assert!(matches!(screen.phase(), Phase::Waiting { .. }));
    screen
}

#[test]
fn the_chooser_is_not_a_way_out_of_a_wait() {
    let mut screen = waiting_login(10 * SECOND);
    assert!(!screen.shows_switch_account());
    let effects = screen.handle(Event::ShowChooser, 10 * SECOND);
    assert_eq!(effects, Effects::default());
    assert_eq!(screen.view(), View::Entry);
}

#[test]
fn the_wait_stays_with_the_account_across_switching() {
    let mut screen = waiting_login(10 * SECOND);
    // After the wait the chooser is offered again; lin has failed twice.
    screen.handle(Event::Tick, 11 * SECOND);
    ask(&mut screen, 11 * SECOND);
    fail(&mut screen, 11 * SECOND);
    let until = 13 * SECOND;
    assert_eq!(screen.phase(), Phase::Waiting { until });
    // Once that wait is over, switch to wang and back to lin.
    screen.handle(Event::Tick, until);
    screen.handle(Event::ShowChooser, until);
    let wang = screen.handle(Event::ChooseAccount(1), until);
    assert_eq!(said(&wang), ["begin wang"]);
    let current_id = current(&screen);
    prompt(&mut screen, PromptKind::Secret, "Password: ", until);
    assert_eq!(current(&screen), current_id);
    screen.handle(Event::ShowChooser, until);
    screen.handle(Event::ChooseAccount(0), until);
    ask(&mut screen, until);
    // lin's next failure is the fourth in a row: 4 seconds, not none.
    fail(&mut screen, until);
    assert_eq!(
        screen.phase(),
        Phase::Waiting {
            until: until + 4 * SECOND
        }
    );
    assert_eq!(screen.attempts().get("lin").failures, 4);
    assert_eq!(screen.attempts().get("wang").failures, 0);
}

#[test]
fn choosing_an_account_puts_its_own_running_wait_in_force() {
    let mut attempts = Attempts::default();
    attempts.record_failure("wang", 0);
    attempts.record_failure("wang", 0);
    let mut screen = login().with_attempts(attempts);
    screen.handle(Event::Shown, 0);
    ask(&mut screen, 0);
    screen.handle(Event::ShowChooser, 0);
    let chosen = screen.handle(Event::ChooseAccount(1), 0);
    assert!(
        chosen.requests.is_empty(),
        "no conversation while wang waits"
    );
    assert_eq!(screen.phase(), Phase::Waiting { until: SECOND });
    assert!(screen.message_line(0).is_some());
    let ended = screen.handle(Event::Tick, SECOND);
    assert_eq!(said(&ended), ["begin wang"]);
}

#[test]
fn other_account_with_a_waiting_name_waits_too() {
    let mut attempts = Attempts::default();
    attempts.record_failure("guest", 0);
    attempts.record_failure("guest", 0);
    let mut screen = ready_login(0).with_attempts(attempts);
    screen.handle(Event::ShowChooser, 0);
    screen.handle(Event::ChooseOtherAccount, 0);
    screen.handle(Event::UserNameEdited("guest".into()), 0);
    let submitted = screen.handle(Event::Submit, 0);
    assert!(submitted.requests.is_empty());
    assert_eq!(screen.phase(), Phase::Waiting { until: SECOND });
    // Another name is another account's own count.
    screen.handle(Event::UserNameEdited("other".into()), 0);
    assert_eq!(screen.phase(), Phase::Ready);
    assert_eq!(said(&screen.handle(Event::Submit, 0)), ["begin other"]);
}

#[test]
fn a_restart_does_not_end_a_wait() {
    let screen = waiting_login(10 * SECOND);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attempts.json");
    screen.attempts().save(&path).unwrap();
    // The greeter starts again a moment later.
    let mut restarted = login().with_attempts(Attempts::load(&path).unwrap());
    let shown = restarted.handle(Event::Shown, 10 * SECOND + 500);
    assert!(shown.requests.is_empty());
    assert_eq!(restarted.phase(), Phase::Waiting { until: 11 * SECOND });
    // And the next failure continues the row.
    restarted.handle(Event::Tick, 11 * SECOND);
    ask(&mut restarted, 11 * SECOND);
    fail(&mut restarted, 11 * SECOND);
    assert_eq!(restarted.attempts().get("lin").failures, 3);
}

#[test]
fn a_success_resets_the_failures() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    ask(&mut screen, 0);
    fail(&mut screen, 0);
    ask(&mut screen, 0);
    screen.handle(Event::Submit, 0);
    let unlocked = authenticate(&mut screen, 0);
    assert!(unlocked.unlock);
    assert!(unlocked.wipe_field);
    assert!(unlocked.attempts_changed);
    assert!(screen.attempts().is_empty());
}

#[test]
fn a_stale_reply_from_a_cancelled_conversation_is_dropped() {
    let mut screen = ready_login(0);
    let old = current(&screen);
    screen.handle(Event::Submit, 0);
    // The person switches account while the answer is with greetd.
    screen.handle(Event::ShowChooser, 0);
    let wang = screen.handle(Event::ChooseAccount(1), 0);
    assert_eq!(said(&wang), ["begin wang"]);
    let new = current(&screen);
    assert_ne!(old, new);
    // The old conversation's replies arrive late.
    for stale in [
        Event::Refused {
            conversation: old,
            refusal: Refusal::WrongPassword,
        },
        Event::Authenticated { conversation: old },
        Event::Prompt {
            conversation: old,
            kind: PromptKind::Visible,
            text: "Old question: ".into(),
        },
        Event::PamMessage {
            conversation: old,
            kind: PamMessageKind::Error,
            text: "old".into(),
        },
    ] {
        assert_eq!(
            screen.handle(stale.clone(), 0),
            Effects::default(),
            "{stale:?}"
        );
    }
    assert!(screen.attempts().is_empty(), "nobody failed");
    assert_eq!(screen.field_label(), "密碼");
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(screen.message_line(0), None);
    // The new one's are taken.
    ask(&mut screen, 0);
    screen.handle(Event::Submit, 0);
    let started = authenticate(&mut screen, 0);
    assert_eq!(said(&started), ["start better-os"]);

    // Cancel then Begin of the same account: the first reply is stale too.
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    let first = current(&screen);
    screen.handle(Event::Key(Key::Escape), 0);
    screen.handle(Event::Key(Key::Escape), 0);
    screen.handle(Event::Pointer, 0);
    assert_ne!(current(&screen), first);
    let late = screen.handle(
        Event::Authenticated {
            conversation: first,
        },
        0,
    );
    assert!(!late.unlock, "a cancelled conversation never unlocks");
    // With no conversation running at all, nothing is taken either.
    let mut idle = lock();
    idle.handle(Event::Shown, 0);
    assert!(
        !idle
            .handle(
                Event::Authenticated {
                    conversation: ConversationId(1)
                },
                0
            )
            .unlock
    );
}

#[test]
fn the_lock_screen_opens_quiet_and_the_first_key_is_typed() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    assert_eq!(screen.view(), View::Quiet);
    assert_eq!(screen.quiet_hint(), Some("按任意鍵或點一下來解鎖"));
    assert_eq!(screen.screen_name(), "已鎖定");
    let woke = screen.handle(Event::Key(Key::Printable('p')), SECOND);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(woke.type_into_field, Some('p'));
    assert_eq!(woke.transition, Some(Transition::CardIn));
    assert!(woke.focus_field);
    assert_eq!(said(&woke), ["begin lin"]);
    assert_eq!(screen.caption(), Some("已鎖定"));
    assert_eq!(screen.submit_label(), "解鎖");
    assert!(!screen.shows_switch_account());
    assert_eq!(screen.session_button_label(), None);
}

#[test]
fn enter_space_and_escape_wake_without_typing() {
    for key in [Key::Enter, Key::Space, Key::Escape, Key::Other] {
        let mut screen = lock();
        screen.handle(Event::Shown, 0);
        let woke = screen.handle(Event::Key(key), 0);
        assert_eq!(screen.view(), View::Entry, "{key:?}");
        assert_eq!(woke.type_into_field, None, "{key:?}");
    }
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    assert_eq!(screen.view(), View::Entry);
}

#[test]
fn a_minute_without_input_wipes_and_the_lock_screen_goes_quiet() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Key(Key::Printable('a')), 0);
    ask(&mut screen, 0);
    screen.handle(Event::FieldEdited, 10 * SECOND);
    assert!(!screen.handle(Event::Tick, 69 * SECOND).wipe_field);
    let idle = screen.handle(Event::Tick, 70 * SECOND);
    assert!(idle.wipe_field);
    assert_eq!(said(&idle), ["cancel"]);
    assert_eq!(idle.transition, Some(Transition::QuietReturn));
    assert_eq!(screen.view(), View::Quiet);
    // Nothing more happens on later ticks.
    assert_eq!(screen.handle(Event::Tick, 200 * SECOND), Effects::default());

    let mut screen = ready_login(0);
    let idle = screen.handle(Event::Tick, 60 * SECOND);
    assert!(idle.wipe_field);
    assert_eq!(
        screen.view(),
        View::Entry,
        "the login screen stays in the entry view"
    );
    assert!(!screen.handle(Event::Tick, 61 * SECOND).wipe_field);
}

#[test]
fn escape_clears_and_a_second_escape_goes_quiet_on_the_lock_screen() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    ask(&mut screen, 0);
    let first = screen.handle(Event::Key(Key::Escape), 0);
    assert!(first.wipe_field);
    assert_eq!(screen.view(), View::Entry);
    let second = screen.handle(Event::Key(Key::Escape), 0);
    assert_eq!(screen.view(), View::Quiet);
    assert!(second.wipe_field);
    assert_eq!(said(&second), ["cancel"]);

    // Typing between the two disarms it.
    screen.handle(Event::Pointer, 0);
    screen.handle(Event::Key(Key::Escape), 0);
    screen.handle(Event::FieldEdited, 0);
    screen.handle(Event::Key(Key::Escape), 0);
    assert_eq!(screen.view(), View::Entry);

    // On the login screen Escape only clears.
    let mut screen = ready_login(0);
    screen.handle(Event::Key(Key::Escape), 0);
    let again = screen.handle(Event::Key(Key::Escape), 0);
    assert!(again.wipe_field);
    assert_eq!(screen.view(), View::Entry);
}

#[test]
fn enter_in_the_field_submits() {
    let mut screen = ready_login(0);
    let effects = screen.handle(Event::Key(Key::Enter), 0);
    assert_eq!(said(&effects), ["answer"]);
}

#[test]
fn the_chooser_switches_accounts() {
    let mut screen = ready_login(0);
    let opened = screen.handle(Event::ShowChooser, 0);
    assert_eq!(screen.view(), View::Chooser);
    assert!(opened.wipe_field);
    assert_eq!(said(&opened), ["cancel"]);
    let rows = screen.chooser_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "林小明");
    assert_eq!(rows[0].initial, "明");
    assert_eq!(rows[0].caption, Some("上次使用"));
    assert_eq!(rows[1].caption, None);
    let chosen = screen.handle(Event::ChooseAccount(1), 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(said(&chosen), ["begin wang"]);
    assert_eq!(screen.account().unwrap().user_name, "wang");
    // wang has no last session: Better OS.
    assert_eq!(screen.chosen_session(), Some("better-os"));
    screen.handle(Event::ShowChooser, 0);
    screen.handle(Event::Key(Key::Escape), 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(
        screen.handle(Event::ChooseAccount(9), 0),
        Effects::default()
    );
}

#[test]
fn other_account_asks_for_a_user_name_first() {
    let mut screen = ready_login(0);
    screen.handle(Event::ShowChooser, 0);
    let other = screen.handle(Event::ChooseOtherAccount, 0);
    assert!(other.requests.is_empty(), "no conversation without a name");
    assert!(screen.shows_user_name_field());
    assert_eq!(screen.account(), None);
    // No name: submit does nothing.
    assert!(screen.handle(Event::Submit, 0).requests.is_empty());
    assert_eq!(screen.phase(), Phase::Ready);
    screen.handle(Event::UserNameEdited("guest".into()), 0);
    let submitted = screen.handle(Event::Submit, 0);
    assert_eq!(said(&submitted), ["begin guest"]);
    assert_eq!(said(&ask(&mut screen, 0)), ["answer"]);
    assert_eq!(screen.field_accessible_name(), "密碼");
}

#[test]
fn a_screen_with_no_listed_account_asks_for_a_name() {
    let mut screen = Screen::login(Vec::new(), sessions(), None, 0);
    assert!(screen.shows_user_name_field());
    assert!(!screen.shows_switch_account());
    assert!(screen.handle(Event::Shown, 0).requests.is_empty());
    assert_eq!(screen.language(), Language::EnUs);
}

#[test]
fn the_session_menu_lists_and_chooses() {
    let mut screen = ready_login(0);
    let opened = screen.handle(Event::OpenSessionMenu, 0);
    assert!(opened.wipe_field);
    assert_eq!(screen.view(), View::SessionMenu);
    let items = screen.session_items();
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["better-os", "sway", "zorin"]);
    assert!(items[1].checked);
    assert_eq!(items[1].caption, Some("上次使用"));
    // A session the menu does not list is not taken.
    screen.handle(Event::ChooseSession("gone".into()), 0);
    assert_eq!(screen.view(), View::SessionMenu);
    screen.handle(Event::ChooseSession("better-os".into()), 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(screen.chosen_session(), Some("better-os"));
}

#[test]
fn gnome_leaves_the_menu_while_the_unlock_service_is_silent() {
    let mut screen = ready_login(0);
    screen.handle(Event::OpenSessionMenu, 0);
    screen.handle(Event::ChooseSession("zorin".into()), 0);
    assert_eq!(screen.chosen_session(), Some("zorin"));
    screen.handle(Event::GnomeUnlockAnswers(false), 0);
    assert_eq!(screen.chosen_session(), Some("sway"));
    let ids: Vec<String> = screen
        .session_items()
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(ids, ["better-os", "sway"]);
    screen.handle(Event::GnomeUnlockAnswers(true), 0);
    assert_eq!(screen.session_items().len(), 3);
}

#[test]
fn a_successful_login_starts_the_chosen_session_and_leaves() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    let authenticated = authenticate(&mut screen, 0);
    assert_eq!(said(&authenticated), ["start sway"]);
    assert!(authenticated.wipe_field);
    assert!(!authenticated.attempts_changed, "nothing failed");
    assert_eq!(screen.view(), View::Starting);
    assert_eq!(screen.starting_text().unwrap(), "正在啟動 Sway…");
    let started = screen.handle(Event::SessionStarted, 0);
    assert_eq!(started.transition, Some(Transition::Leave));
}

#[test]
fn gnome_that_cannot_lock_is_not_started() {
    let mut screen = ready_login(0);
    screen.handle(Event::OpenSessionMenu, 0);
    screen.handle(Event::ChooseSession("zorin".into()), 0);
    ask(&mut screen, 0);
    screen.handle(Event::Submit, 0);
    authenticate(&mut screen, 0);
    let refused = screen.handle(Event::GnomeCannotLock, 0);
    assert_eq!(screen.view(), View::Entry);
    assert!(refused.focus_field);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "GNOME 目前無法鎖定，所以沒有啟動。請選擇其他工作階段，或再試一次。"
    );
    assert_ne!(screen.chosen_session(), Some("zorin"));
    // Typing does not hide why GNOME was not started.
    screen.handle(Event::FieldEdited, 0);
    assert!(screen.message_line(0).is_some());
}

#[test]
fn a_session_that_did_not_start() {
    let mut screen = login();
    screen.handle(Event::Shown, 0);
    let shown = screen.handle(
        Event::FailedToStart {
            session_id: "sway".into(),
        },
        0,
    );
    assert_eq!(said(&shown), ["cancel"]);
    assert_eq!(screen.view(), View::FailedToStart);
    let panel = screen.failed_to_start().unwrap();
    assert_eq!(panel.title, "Sway 無法啟動。");
    assert_eq!(panel.body, "你可以再試一次，或改用其他工作階段登入。");
    assert_eq!(panel.try_again, "再試一次");
    assert_eq!(panel.choose_another, "選擇其他工作階段");
    screen.handle(Event::ChooseAnotherSession, 0);
    assert_eq!(screen.view(), View::SessionMenu);
    screen.handle(Event::Back, 0);
    assert_eq!(screen.view(), View::Entry);

    let mut screen = login();
    screen.handle(
        Event::FailedToStart {
            session_id: "sway".into(),
        },
        0,
    );
    let again = screen.handle(Event::TryAgain, 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(said(&again), ["begin lin"]);
}

#[test]
fn an_expired_password_is_changed_one_prompt_at_a_time() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    let told = message(
        &mut screen,
        PamMessageKind::Error,
        "You are required to change your password immediately (password expired).",
        0,
    );
    assert_eq!(said(&told), ["acknowledge"]);
    prompt(&mut screen, PromptKind::Secret, "Current password: ", 0);
    assert_eq!(screen.view(), View::PasswordChange);
    assert_eq!(screen.field_label(), "目前的密碼");
    assert_eq!(screen.field_accessible_name(), "目前的密碼");
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "你的密碼已過期，請設定新密碼。"
    );
    assert_eq!(said(&screen.handle(Event::Submit, 0)), ["answer"]);
    prompt(&mut screen, PromptKind::Secret, "New password: ", 0);
    assert_eq!(screen.field_label(), "新密碼");
    screen.handle(Event::Submit, 0);
    prompt(&mut screen, PromptKind::Secret, "Retype new password: ", 0);
    assert_eq!(screen.field_label(), "再輸入一次新密碼");
    screen.handle(Event::Submit, 0);
    authenticate(&mut screen, 0);
    assert_eq!(screen.view(), View::Starting);
    assert_eq!(screen.message_line(0).unwrap().text, "密碼已變更。");
}

#[test]
fn an_unknown_prompt_is_shown_as_pam_wrote_it() {
    let mut screen = ready_login(0);
    prompt(&mut screen, PromptKind::Visible, "Token code: ", 0);
    assert_eq!(screen.field_label(), "Token code:");
    assert!(!screen.field_is_secret());
    assert_eq!(screen.view(), View::Entry);
}

#[test]
fn locked_and_expired_accounts() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    let locked = refuse(
        &mut screen,
        Refusal::CannotSignIn {
            pam_text: "Account locked due to 3 failed logins".into(),
        },
        0,
    );
    assert!(locked.wipe_field);
    let line = screen.message_line(0).unwrap();
    assert_eq!(line.text, "這個帳號目前無法登入。");
    assert_eq!(
        line.detail.as_deref(),
        Some("Account locked due to 3 failed logins")
    );
    assert_eq!(
        screen.phase(),
        Phase::Ready,
        "a lock is not a wrong password: no wait"
    );

    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    refuse(&mut screen, Refusal::AccountExpired, 0);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "這個帳號已停用，請聯絡系統管理員。"
    );
}

#[test]
fn pam_information_is_shown_and_acknowledged() {
    let mut screen = ready_login(0);
    let effects = message(
        &mut screen,
        PamMessageKind::Info,
        "Warning: your password will expire in 3 days.",
        0,
    );
    assert_eq!(said(&effects), ["acknowledge"]);
    let line = screen.message_line(0).unwrap();
    assert_eq!(line.kind, MessageKind::Notice);
    assert_eq!(line.text, "Warning: your password will expire in 3 days.");
}

#[test]
fn caps_lock_shows_while_focused_and_an_error_comes_first() {
    let mut screen = ready_login(0);
    screen.handle(Event::CapsLock(true), 0);
    assert_eq!(screen.message_line(0), None, "not without focus");
    screen.handle(Event::FieldFocus(true), 0);
    let line = screen.message_line(0).unwrap();
    assert_eq!(line.text, "Caps Lock 已開啟");
    assert_eq!(line.kind, MessageKind::Notice);
    fail(&mut screen, 0);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "密碼不正確，請再試一次。"
    );
    screen.handle(Event::FieldEdited, 0);
    assert_eq!(screen.message_line(0).unwrap().text, "Caps Lock 已開啟");
    screen.handle(Event::CapsLock(false), 0);
    assert_eq!(screen.message_line(0), None);
}

#[test]
fn the_input_method_notice_shows_once_per_lock() {
    let mut screen = Screen::lock(
        account("lin", "林小明", 1, ""),
        Some("zh_TW.UTF-8".into()),
        true,
        0,
    );
    screen.handle(Event::Shown, 0);
    assert_eq!(screen.message_line(0), None, "not in the quiet view");
    screen.handle(Event::Pointer, 0);
    ask(&mut screen, 0);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "注音在這裡暫停，請用英文鍵盤輸入密碼。"
    );
    fail(&mut screen, 0);
    screen.handle(Event::FieldEdited, 0);
    assert_eq!(screen.message_line(0), None, "gone after the first attempt");
}

#[test]
fn greetd_that_does_not_answer() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    assert_eq!(screen.handle(Event::Tick, 59 * SECOND), Effects::default());
    let timed_out = screen.handle(Event::Tick, 60 * SECOND);
    assert_eq!(said(&timed_out), ["cancel"]);
    assert!(timed_out.wipe_field);
    assert_eq!(screen.phase(), Phase::Ready);
    assert_eq!(
        screen.message_line(60 * SECOND).unwrap().text,
        "登入服務沒有回應。請重新開機。"
    );

    let mut screen = ready_login(0);
    let gone = screen.handle(Event::ServiceNotResponding, 0);
    assert!(gone.wipe_field);
    assert_eq!(screen.phase(), Phase::Unavailable);
    assert!(!screen.field_enabled());
    assert!(screen.handle(Event::Submit, 0).requests.is_empty());
    // The power menu still works.
    let power = screen.handle(Event::PowerChosen(PowerAction::Restart), 0);
    assert_eq!(said(&power), ["power Restart"]);
}

#[test]
fn power_overlays_and_sleep() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    ask(&mut screen, 0);
    let sleeping = screen.handle(Event::PowerChosen(PowerAction::Sleep), 0);
    assert!(sleeping.wipe_field);
    assert_eq!(said(&sleeping), ["cancel", "power Sleep"]);
    assert_eq!(screen.overlay_text(), Some("正在進入睡眠…"));
    screen.handle(Event::Resumed, 0);
    assert_eq!(screen.view(), View::Quiet);

    let mut screen = ready_login(0);
    screen.handle(Event::PowerChosen(PowerAction::Sleep), 0);
    let resumed = screen.handle(Event::Resumed, 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(said(&resumed), ["begin lin"]);

    let mut screen = ready_login(0);
    screen.handle(
        Event::PowerRefused(Language::ZhTw.copy().someone_else_signed_in),
        0,
    );
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "還有其他人登入，現在無法從這裡重新開機或關機。"
    );
}

#[test]
fn sleep_from_anywhere_wipes_a_half_typed_password() {
    // The lid closes on the lock screen with half a password in the field.
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    ask(&mut screen, 0);
    screen.handle(Event::FieldEdited, 0);
    let suspending = screen.handle(Event::Suspending, 0);
    assert!(suspending.wipe_field);
    assert_eq!(said(&suspending), ["cancel"]);
    assert_eq!(screen.view(), View::Quiet);
    let resumed = screen.handle(Event::Resumed, SECOND);
    assert!(resumed.wipe_field);
    assert_eq!(screen.view(), View::Quiet);

    // A wake with no warning before it wipes too.
    let mut screen = ready_login(0);
    screen.handle(Event::FieldEdited, 0);
    let resumed = screen.handle(Event::Resumed, SECOND);
    assert!(resumed.wipe_field);
    assert_eq!(screen.view(), View::Entry);
    let mut screen = ready_login(0);
    let suspending = screen.handle(Event::Suspending, 0);
    assert!(suspending.wipe_field);
    assert_eq!(said(&suspending), ["cancel"]);
    assert_eq!(said(&screen.handle(Event::Resumed, SECOND)), ["begin lin"]);
}

#[test]
fn every_change_of_view_wipes_the_field() {
    let mut screen = ready_login(0);
    for event in [
        Event::ShowChooser,
        Event::Back,
        Event::OpenSessionMenu,
        Event::Back,
        Event::PowerChosen(PowerAction::ShutDown),
    ] {
        let before = screen.view();
        let effects = screen.handle(event.clone(), 0);
        assert_ne!(screen.view(), before, "{event:?}");
        assert!(effects.wipe_field, "{event:?}");
    }
}

#[test]
fn the_screen_speaks_the_accounts_language() {
    let mut screen = Screen::login(
        vec![
            Account {
                language: "en_US.UTF-8".into(),
                ..account("tim", "Tim", 5, "")
            },
            Account {
                language: "fr_FR.UTF-8".into(),
                ..account("zoe", "Zoé", 1, "")
            },
        ],
        sessions(),
        Some("zh_TW.UTF-8".into()),
        0,
    );
    assert_eq!(screen.language(), Language::EnUs);
    assert_eq!(screen.submit_label(), "Sign in");
    screen.handle(Event::ShowChooser, 0);
    screen.handle(Event::ChooseAccount(1), 0);
    // French is not shipped: the system's language.
    assert_eq!(screen.language(), Language::ZhTw);
}

#[test]
fn no_session_to_start_is_said_not_guessed() {
    let mut screen = Screen::login(vec![account("lin", "林小明", 1, "")], Vec::new(), None, 0);
    screen.handle(Event::Shown, 0);
    ask(&mut screen, 0);
    screen.handle(Event::Submit, 0);
    let effects = authenticate(&mut screen, 0);
    assert_eq!(said(&effects), ["cancel"]);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "There's no session to start."
    );
    assert_eq!(screen.session_button_label(), None);
}

#[test]
fn the_boot_clock_reads() {
    let first = boot_time_now().expect("CLOCK_BOOTTIME");
    let second = boot_time_now().unwrap();
    assert!(second >= first);
}

use super::*;
use crate::sessions::parse_session;

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
        parse_session("better-os", better.as_bytes())
            .unwrap()
            .unwrap(),
        parse_session("sway", sway.as_bytes()).unwrap().unwrap(),
        parse_session("zorin", zorin.as_bytes()).unwrap().unwrap(),
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

fn password_prompt() -> Event {
    Event::Prompt {
        kind: PromptKind::Secret,
        text: "Password: ".into(),
    }
}

/// Shows the login screen and has PAM ask for the password.
fn ready_login(now: Millis) -> Screen {
    let mut screen = login();
    let shown = screen.handle(Event::Shown, now);
    assert_eq!(shown.requests, [Request::Begin { user: "lin".into() }]);
    assert!(shown.focus_field);
    screen.handle(password_prompt(), now);
    screen
}

fn fail(screen: &mut Screen, now: Millis) -> Effects {
    screen.handle(Event::Submit, now);
    screen.handle(Event::Refused(Refusal::WrongPassword), now)
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
    assert_eq!(effects.requests, [Request::Answer]);
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
    let asked = screen.handle(password_prompt(), 6);
    assert_eq!(asked.requests, [Request::Answer]);
    assert!(asked.wipe_field);
}

#[test]
fn a_wrong_password_clears_the_field_and_keeps_focus() {
    let mut screen = ready_login(0);
    let effects = fail(&mut screen, SECOND);
    assert!(effects.wipe_field);
    assert!(effects.focus_field);
    assert_eq!(
        effects.requests,
        [Request::Cancel, Request::Begin { user: "lin".into() }]
    );
    assert_eq!(screen.phase(), Phase::Ready);
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
    screen.handle(password_prompt(), 0);
    let second = fail(&mut screen, 10 * SECOND);
    assert!(
        !second
            .requests
            .iter()
            .any(|request| matches!(request, Request::Begin { .. })),
        "no new conversation during the wait"
    );
    assert_eq!(screen.phase(), Phase::Waiting { until: 11 * SECOND });
    assert!(!screen.field_enabled());
    assert_eq!(
        screen.message_line(10 * SECOND).unwrap().text,
        "密碼不正確。請在 1 秒後再試一次。"
    );
    let ended = screen.handle(Event::Tick, 11 * SECOND);
    assert_eq!(ended.requests, [Request::Begin { user: "lin".into() }]);
    assert_eq!(screen.phase(), Phase::Ready);
    screen.handle(password_prompt(), 11 * SECOND);
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
        screen.handle(password_prompt(), now);
        fail(&mut screen, now);
    }
    assert_eq!(
        screen.phase(),
        Phase::Waiting {
            until: now + 30 * SECOND
        }
    );
}

#[test]
fn a_success_resets_the_failures() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    screen.handle(password_prompt(), 0);
    fail(&mut screen, 0);
    screen.handle(password_prompt(), 0);
    screen.handle(Event::Submit, 0);
    let unlocked = screen.handle(Event::Authenticated, 0);
    assert!(unlocked.unlock);
    assert!(unlocked.wipe_field);
    // A later failure is a first failure again: no wait.
    screen.handle(Event::Pointer, 0);
    screen.handle(password_prompt(), 0);
    fail(&mut screen, SECOND);
    assert_eq!(screen.phase(), Phase::Ready);
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
    assert_eq!(woke.requests, [Request::Begin { user: "lin".into() }]);
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
    screen.handle(password_prompt(), 0);
    screen.handle(Event::FieldEdited, 10 * SECOND);
    assert!(!screen.handle(Event::Tick, 69 * SECOND).wipe_field);
    let idle = screen.handle(Event::Tick, 70 * SECOND);
    assert!(idle.wipe_field);
    assert_eq!(idle.requests, [Request::Cancel]);
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
    screen.handle(password_prompt(), 0);
    let first = screen.handle(Event::Key(Key::Escape), 0);
    assert!(first.wipe_field);
    assert_eq!(screen.view(), View::Entry);
    let second = screen.handle(Event::Key(Key::Escape), 0);
    assert_eq!(screen.view(), View::Quiet);
    assert!(second.wipe_field);
    assert_eq!(second.requests, [Request::Cancel]);

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
    assert_eq!(effects.requests, [Request::Answer]);
}

#[test]
fn the_chooser_switches_accounts() {
    let mut screen = ready_login(0);
    let opened = screen.handle(Event::ShowChooser, 0);
    assert_eq!(screen.view(), View::Chooser);
    assert!(opened.wipe_field);
    assert_eq!(opened.requests, [Request::Cancel]);
    let rows = screen.chooser_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "林小明");
    assert_eq!(rows[0].initial, "明");
    assert_eq!(rows[0].caption, Some("上次使用"));
    assert_eq!(rows[1].caption, None);
    let chosen = screen.handle(Event::ChooseAccount(1), 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(
        chosen.requests,
        [Request::Begin {
            user: "wang".into()
        }]
    );
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
    assert_eq!(
        submitted.requests,
        [Request::Begin {
            user: "guest".into()
        }]
    );
    assert_eq!(
        screen.handle(password_prompt(), 0).requests,
        [Request::Answer]
    );
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
    let authenticated = screen.handle(Event::Authenticated, 0);
    assert_eq!(
        authenticated.requests,
        [Request::StartSession {
            session_id: "sway".into()
        }]
    );
    assert!(authenticated.wipe_field);
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
    screen.handle(password_prompt(), 0);
    screen.handle(Event::Submit, 0);
    screen.handle(Event::Authenticated, 0);
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
    assert_eq!(shown.requests, [Request::Cancel]);
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
    assert_eq!(again.requests, [Request::Begin { user: "lin".into() }]);
}

#[test]
fn an_expired_password_is_changed_one_prompt_at_a_time() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    let told = screen.handle(
        Event::PamMessage {
            kind: PamMessageKind::Error,
            text: "You are required to change your password immediately (password expired).".into(),
        },
        0,
    );
    assert_eq!(told.requests, [Request::Acknowledge]);
    screen.handle(
        Event::Prompt {
            kind: PromptKind::Secret,
            text: "Current password: ".into(),
        },
        0,
    );
    assert_eq!(screen.view(), View::PasswordChange);
    assert_eq!(screen.field_label(), "目前的密碼");
    assert_eq!(screen.field_accessible_name(), "目前的密碼");
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "你的密碼已過期，請設定新密碼。"
    );
    assert_eq!(screen.handle(Event::Submit, 0).requests, [Request::Answer]);
    screen.handle(
        Event::Prompt {
            kind: PromptKind::Secret,
            text: "New password: ".into(),
        },
        0,
    );
    assert_eq!(screen.field_label(), "新密碼");
    screen.handle(Event::Submit, 0);
    screen.handle(
        Event::Prompt {
            kind: PromptKind::Secret,
            text: "Retype new password: ".into(),
        },
        0,
    );
    assert_eq!(screen.field_label(), "再輸入一次新密碼");
    screen.handle(Event::Submit, 0);
    screen.handle(Event::Authenticated, 0);
    assert_eq!(screen.view(), View::Starting);
    assert_eq!(screen.message_line(0).unwrap().text, "密碼已變更。");
}

#[test]
fn an_unknown_prompt_is_shown_as_pam_wrote_it() {
    let mut screen = ready_login(0);
    screen.handle(
        Event::Prompt {
            kind: PromptKind::Visible,
            text: "Token code: ".into(),
        },
        0,
    );
    assert_eq!(screen.field_label(), "Token code:");
    assert!(!screen.field_is_secret());
    assert_eq!(screen.view(), View::Entry);
}

#[test]
fn locked_and_expired_accounts() {
    let mut screen = ready_login(0);
    screen.handle(Event::Submit, 0);
    let locked = screen.handle(
        Event::Refused(Refusal::CannotSignIn {
            pam_text: "Account locked due to 3 failed logins".into(),
        }),
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
    screen.handle(Event::Refused(Refusal::AccountExpired), 0);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "這個帳號已停用，請聯絡系統管理員。"
    );
}

#[test]
fn pam_information_is_shown_and_acknowledged() {
    let mut screen = ready_login(0);
    let effects = screen.handle(
        Event::PamMessage {
            kind: PamMessageKind::Info,
            text: "Warning: your password will expire in 3 days.".into(),
        },
        0,
    );
    assert_eq!(effects.requests, [Request::Acknowledge]);
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
    screen.handle(password_prompt(), 0);
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
    assert_eq!(timed_out.requests, [Request::Cancel]);
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
    assert_eq!(power.requests, [Request::Power(PowerAction::Restart)]);
}

#[test]
fn power_overlays_and_sleep() {
    let mut screen = lock();
    screen.handle(Event::Shown, 0);
    screen.handle(Event::Pointer, 0);
    screen.handle(password_prompt(), 0);
    let sleeping = screen.handle(Event::PowerChosen(PowerAction::Sleep), 0);
    assert!(sleeping.wipe_field);
    assert_eq!(
        sleeping.requests,
        [Request::Cancel, Request::Power(PowerAction::Sleep)]
    );
    assert_eq!(screen.overlay_text(), Some("正在進入睡眠…"));
    screen.handle(Event::Resumed, 0);
    assert_eq!(screen.view(), View::Quiet);

    let mut screen = ready_login(0);
    screen.handle(Event::PowerChosen(PowerAction::Sleep), 0);
    let resumed = screen.handle(Event::Resumed, 0);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(resumed.requests, [Request::Begin { user: "lin".into() }]);

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
    screen.handle(password_prompt(), 0);
    screen.handle(Event::Submit, 0);
    let effects = screen.handle(Event::Authenticated, 0);
    assert_eq!(effects.requests, [Request::Cancel]);
    assert_eq!(screen.view(), View::Entry);
    assert_eq!(
        screen.message_line(0).unwrap().text,
        "There's no session to start."
    );
    assert_eq!(screen.session_button_label(), None);
}

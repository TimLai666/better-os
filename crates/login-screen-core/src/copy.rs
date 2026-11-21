//! Every sentence of design/login-screen-spec.md's tables, in both shipped
//! languages.
//!
//! Following the project's pattern (`touchpad-views`, `awake-gui`): one struct
//! of `&'static str` per language, so a sentence missing in one language is a
//! missing field and a compile error, with no map lookup and no fallback to
//! English. The struct is declared through [`copy_table!`], which also lists
//! every field by name, so the completeness test reads all of them rather than
//! a hand-kept list.
//!
//! Sentences with a value in them carry `{name}`-style placeholders, filled by
//! [`fill`]; a test holds that both languages use the same placeholders.

/// The languages Better OS ships.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Language {
    ZhTw,
    EnUs,
}

impl Language {
    /// The shipped language a POSIX locale or a BCP 47 tag names, or `None`
    /// when Better OS does not ship it.
    pub fn from_locale_name(locale: &str) -> Option<Self> {
        let lowered = locale.trim().to_ascii_lowercase();
        let base = lowered
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('-', "_");
        match base.as_str() {
            "zh_tw" | "zh_hant" | "zh_hant_tw" => Some(Self::ZhTw),
            "en" | "en_us" => Some(Self::EnUs),
            _ if base.starts_with("en_") => Some(Self::EnUs),
            _ => None,
        }
    }

    /// The language the screen speaks (spec section 5.7): the chosen
    /// account's AccountsService `Language` when Better OS ships it, else the
    /// system's (`locale1`'s `LANG`), else English.
    pub fn for_screen(account_language: Option<&str>, system_language: Option<&str>) -> Self {
        account_language
            .and_then(Self::from_locale_name)
            .or_else(|| system_language.and_then(Self::from_locale_name))
            .unwrap_or(Self::EnUs)
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::ZhTw => "zh-TW",
            Self::EnUs => "en-US",
        }
    }

    pub fn copy(self) -> &'static Copy {
        match self {
            Self::ZhTw => &ZH_TW,
            Self::EnUs => &EN_US,
        }
    }
}

/// Fills `{key}` placeholders. A placeholder with no value is left as it is,
/// which a test catches, rather than becoming an empty string that reads as a
/// finished sentence.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match values.iter().find(|(name, _)| *name == key) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The placeholder names a template uses, in order.
pub fn placeholders(template: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        names.push(&after[..end]);
        rest = &after[end + 1..];
    }
    names
}

macro_rules! copy_table {
    ($( $(#[$meta:meta])* $field:ident ),* $(,)?) => {
        /// Every sentence the two screens, the text fall back and Better
        /// Manager's switch show.
        pub struct Copy {
            $( $(#[$meta])* pub $field: &'static str, )*
        }

        impl Copy {
            /// Every field, by name.
            pub fn entries(&self) -> Vec<(&'static str, &'static str)> {
                vec![ $( (stringify!($field), self.$field), )* ]
            }
        }
    };
}

copy_table! {
    // ---- The screen (sections 3.1, 3.3, 3.4, 3.15, 4.1) -----------------
    /// The screen's accessible name on the login screen.
    login_screen,
    /// The lock screen's accessible name, and the caption under the name.
    locked,
    password,
    /// The field's accessible name.
    password_for,
    show_password,
    hide_password,
    sign_in,
    unlock,
    checking,
    quiet_hint,
    keyboard_layout,
    accessibility,
    network_connected,
    network_not_connected,
    power,

    // ---- Several accounts (3.5) ------------------------------------------
    switch_account,
    choose_account,
    last_used,
    other_account,
    user_name,
    back,

    // ---- Session choice (3.6, 5.6) ---------------------------------------
    session_button,
    choose_session,
    gnome_cannot_lock,
    /// `/usr/share/wayland-sessions/` lists nothing the menu can offer.
    no_session,

    // ---- Keyboard and Caps Lock (3.7, 3.8) -------------------------------
    input_method_paused,
    caps_lock_on,

    // ---- Failures (3.9) ---------------------------------------------------
    wrong_password,
    wrong_password_wait,
    /// The wait's last second, which English cannot say with "seconds".
    wrong_password_wait_one,
    account_locked,
    account_expired,
    password_expired,
    current_password,
    new_password,
    retype_new_password,
    password_changed,
    service_not_responding,

    // ---- Reserved for later (3.10) ---------------------------------------
    fingerprint_hint,
    smart_card_hint,

    // ---- Accessibility switches (3.11) -----------------------------------
    screen_reader,
    high_contrast,
    large_text,
    on_screen_keyboard,

    // ---- Power (3.12) ------------------------------------------------------
    sleep,
    going_to_sleep,
    restart,
    restarting,
    shut_down,
    shutting_down,
    someone_else_signed_in,
    unsaved_work_lost,

    // ---- Starting the session (3.16) -------------------------------------
    starting_session,
    session_did_not_start,
    session_did_not_start_body,
    try_again,
    choose_another_session,

    // ---- The lock screen's card bottom (4.2, 4.3) ------------------------
    while_you_were_away,
    more_notifications,
    /// One more, which English cannot say with "notifications".
    more_notifications_one,
    new_notification,
    now_playing,
    previous,
    play,
    pause,
    next,

    // ---- The Notifications page's two settings (4.2, ticket 238) ---------
    show_notifications_on_lock_screen,
    show_their_content,

    // ---- The text login the boot falls back to (5.10) --------------------
    fallback_title,
    fallback_sign_in,
    fallback_status,
    fallback_gdm,
    stage_no_graphics,
    stage_login_manager,
    stage_sign_in_service,
    stage_display_server,
    stage_login_screen_program,
    stage_supervisor,

    // ---- Better Manager's switch (6, ticket 266) -------------------------
    switch_title,
    switch_description,
    switch_confirm,
    switch_next_start,
    restart_now,
    switch_on,
    go_back_to_gnome,
    last_start_fell_back,
    see_details,
    last_start_retried,
    automatic_login_carried,
    going_back_next_start,
}

pub static ZH_TW: Copy = Copy {
    login_screen: "登入畫面",
    locked: "已鎖定",
    password: "密碼",
    password_for: "{name} 的密碼",
    show_password: "顯示密碼",
    hide_password: "隱藏密碼",
    sign_in: "登入",
    unlock: "解鎖",
    checking: "正在確認…",
    quiet_hint: "按任意鍵或點一下來解鎖",
    keyboard_layout: "鍵盤配置：{layout}",
    accessibility: "協助工具",
    network_connected: "網路：已連線到 {name}",
    network_not_connected: "網路：未連線",
    power: "電源",

    switch_account: "切換帳號",
    choose_account: "選擇帳號",
    last_used: "上次使用",
    other_account: "其他帳號…",
    user_name: "帳號名稱",
    back: "返回",

    session_button: "工作階段：{name}",
    choose_session: "選擇工作階段",
    gnome_cannot_lock: "GNOME 目前無法鎖定，所以沒有啟動。請選擇其他工作階段，或再試一次。",
    no_session: "找不到可以啟動的工作階段。",

    input_method_paused: "注音在這裡暫停，請用英文鍵盤輸入密碼。",
    caps_lock_on: "Caps Lock 已開啟",

    wrong_password: "密碼不正確，請再試一次。",
    wrong_password_wait: "密碼不正確。請在 {n} 秒後再試一次。",
    wrong_password_wait_one: "密碼不正確。請在 1 秒後再試一次。",
    account_locked: "這個帳號目前無法登入。",
    account_expired: "這個帳號已停用，請聯絡系統管理員。",
    password_expired: "你的密碼已過期，請設定新密碼。",
    current_password: "目前的密碼",
    new_password: "新密碼",
    retype_new_password: "再輸入一次新密碼",
    password_changed: "密碼已變更。",
    service_not_responding: "登入服務沒有回應。請重新開機。",

    fingerprint_hint: "或將手指放在指紋感應器上",
    smart_card_hint: "或插入智慧卡",

    screen_reader: "螢幕閱讀器",
    high_contrast: "高對比",
    large_text: "大字體",
    on_screen_keyboard: "螢幕小鍵盤",

    sleep: "睡眠",
    going_to_sleep: "正在進入睡眠…",
    restart: "重新開機",
    restarting: "正在重新開機…",
    shut_down: "關機",
    shutting_down: "正在關機…",
    someone_else_signed_in: "還有其他人登入，現在無法從這裡重新開機或關機。",
    unsaved_work_lost: "未儲存的工作會遺失",

    starting_session: "正在啟動 {session}…",
    session_did_not_start: "{session} 無法啟動。",
    session_did_not_start_body: "你可以再試一次，或改用其他工作階段登入。",
    try_again: "再試一次",
    choose_another_session: "選擇其他工作階段",

    while_you_were_away: "你離開時收到的通知",
    more_notifications: "還有 {n} 則通知",
    more_notifications_one: "還有 1 則通知",
    new_notification: "新通知",
    now_playing: "正在播放",
    previous: "上一首",
    play: "播放",
    pause: "暫停",
    next: "下一首",

    show_notifications_on_lock_screen: "在鎖定畫面上顯示通知",
    show_their_content: "顯示內容",

    fallback_title: "Better OS 登入畫面無法顯示（{stage}，{time}）。",
    fallback_sign_in: "請在下方輸入帳號名稱和密碼登入文字模式。",
    fallback_status: "登入後輸入 better-os-login-status 查看原因。",
    fallback_gdm: "要改回 GNOME 登入畫面，輸入 sudo dpkg-reconfigure gdm3 並選擇 gdm3。",
    stage_no_graphics: "找不到顯示卡",
    stage_login_manager: "登入管理服務沒有回應",
    stage_sign_in_service: "登入服務無法啟動",
    stage_display_server: "顯示伺服器無法啟動",
    stage_login_screen_program: "登入畫面程式沒有回應",
    stage_supervisor: "登入監督程式無法執行",

    switch_title: "使用 Better OS 登入畫面",
    switch_description: "開機和登出後看到的畫面會換成 Better OS 的樣式，鎖定畫面也不再需要 GNOME 登入畫面。同一時間只能有一個人登入。",
    switch_confirm: "改用 Better OS 登入畫面",
    switch_next_start: "下次開機時會使用 Better OS 登入畫面。",
    restart_now: "立即重新開機",
    switch_on: "正在使用 Better OS 登入畫面。",
    go_back_to_gnome: "改回 GNOME 登入畫面",
    last_start_fell_back: "上次開機時登入畫面無法顯示（{stage}），已改用文字模式登入。",
    see_details: "查看詳細資料",
    last_start_retried: "上次開機時登入畫面重新啟動了一次（{stage}）。",
    automatic_login_carried: "你目前設定開機後自動登入 {name}，改用後會繼續自動登入。",
    going_back_next_start: "下次開機時會回到 GNOME 登入畫面。",
};

pub static EN_US: Copy = Copy {
    login_screen: "Login screen",
    locked: "Locked",
    password: "Password",
    password_for: "Password for {name}",
    show_password: "Show password",
    hide_password: "Hide password",
    sign_in: "Sign in",
    unlock: "Unlock",
    checking: "Checking…",
    quiet_hint: "Press any key or click to unlock",
    keyboard_layout: "Keyboard layout: {layout}",
    accessibility: "Accessibility",
    network_connected: "Network: connected to {name}",
    network_not_connected: "Network: not connected",
    power: "Power",

    switch_account: "Switch account",
    choose_account: "Choose an account",
    last_used: "Last used",
    other_account: "Other account…",
    user_name: "User name",
    back: "Back",

    session_button: "Session: {name}",
    choose_session: "Choose a session",
    gnome_cannot_lock: "GNOME can't lock right now, so it wasn't started. Choose another session or try again.",
    no_session: "There's no session to start.",

    input_method_paused: "Zhuyin is paused here. Type your password with the English keyboard.",
    caps_lock_on: "Caps Lock is on",

    wrong_password: "That password isn't right. Try again.",
    wrong_password_wait: "That password isn't right. Try again in {n} seconds.",
    wrong_password_wait_one: "That password isn't right. Try again in 1 second.",
    account_locked: "This account can't sign in right now.",
    account_expired: "This account has been turned off. Ask your administrator.",
    password_expired: "Your password has expired. Set a new one.",
    current_password: "Current password",
    new_password: "New password",
    retype_new_password: "Type the new password again",
    password_changed: "Password changed.",
    service_not_responding: "The sign-in service isn't responding. Restart the computer.",

    fingerprint_hint: "Or touch the fingerprint sensor",
    smart_card_hint: "Or insert your smart card",

    screen_reader: "Screen reader",
    high_contrast: "High contrast",
    large_text: "Large text",
    on_screen_keyboard: "On-screen keyboard",

    sleep: "Sleep",
    going_to_sleep: "Going to sleep…",
    restart: "Restart",
    restarting: "Restarting…",
    shut_down: "Shut down",
    shutting_down: "Shutting down…",
    someone_else_signed_in: "Someone else is still signed in, so you can't restart or shut down from here.",
    unsaved_work_lost: "Unsaved work will be lost",

    starting_session: "Starting {session}…",
    session_did_not_start: "{session} didn't start.",
    session_did_not_start_body: "You can try again, or sign in to another session.",
    try_again: "Try again",
    choose_another_session: "Choose another session",

    while_you_were_away: "While you were away",
    more_notifications: "{n} more notifications",
    more_notifications_one: "1 more notification",
    new_notification: "New notification",
    now_playing: "Now playing",
    previous: "Previous",
    play: "Play",
    pause: "Pause",
    next: "Next",

    show_notifications_on_lock_screen: "Show notifications on the lock screen",
    show_their_content: "Show their content",

    fallback_title: "The Better OS login screen couldn't be shown ({stage}, {time}).",
    fallback_sign_in: "Sign in below with your user name and password to use text mode.",
    fallback_status: "After signing in, type better-os-login-status to see why.",
    fallback_gdm: "To go back to the GNOME login screen, type sudo dpkg-reconfigure gdm3 and choose gdm3.",
    stage_no_graphics: "no graphics device",
    stage_login_manager: "the login manager didn't respond",
    stage_sign_in_service: "the sign-in service didn't start",
    stage_display_server: "the display server didn't start",
    stage_login_screen_program: "the login screen program didn't respond",
    stage_supervisor: "the login supervisor couldn't run",

    switch_title: "Use the Better OS login screen",
    switch_description: "The screen you see after starting up or signing out changes to Better OS's, and locking no longer needs the GNOME login screen. Only one person can be signed in at a time.",
    switch_confirm: "Switch to the Better OS login screen",
    switch_next_start: "You'll see the Better OS login screen the next time you start up.",
    restart_now: "Restart now",
    switch_on: "You're using the Better OS login screen.",
    go_back_to_gnome: "Go back to the GNOME login screen",
    last_start_fell_back: "The login screen couldn't be shown last time you started up ({stage}), so a text sign-in was used.",
    see_details: "See details",
    last_start_retried: "The login screen had to be restarted once last time you started up ({stage}).",
    automatic_login_carried: "You're set to sign in as {name} automatically. That will continue after the switch.",
    going_back_next_start: "You'll see the GNOME login screen the next time you start up.",
};

/// The stages the text fall back names (spec section 5.10). The supervisor
/// (ticket 267) writes the English name on the console; Better Manager
/// (ticket 266) shows the zh-TW one for the same record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackStage {
    /// Stage 1: no DRM device.
    NoGraphics,
    /// Stage 3: logind did not answer.
    LoginManager,
    /// Stage 5: greetd or the greeter's PAM session did not start.
    SignInService,
    /// Stage 6: the compositor did not start.
    DisplayServer,
    /// Stage 7: the greeter did not become ready.
    LoginScreenProgram,
    /// The backstop's row: the supervisor itself could not run.
    Supervisor,
}

impl FallbackStage {
    pub fn name(self, language: Language) -> &'static str {
        let copy = language.copy();
        match self {
            Self::NoGraphics => copy.stage_no_graphics,
            Self::LoginManager => copy.stage_login_manager,
            Self::SignInService => copy.stage_sign_in_service,
            Self::DisplayServer => copy.stage_display_server,
            Self::LoginScreenProgram => copy.stage_login_screen_program,
            Self::Supervisor => copy.stage_supervisor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sentence_exists_in_both_languages_and_is_translated() {
        let english = EN_US.entries();
        let chinese = ZH_TW.entries();
        assert_eq!(english.len(), chinese.len());
        let mut same = Vec::new();
        for ((key, en), (key_zh, zh)) in english.iter().zip(chinese.iter()) {
            assert_eq!(key, key_zh);
            assert!(!en.trim().is_empty(), "English {key} is empty");
            assert!(!zh.trim().is_empty(), "zh-TW {key} is empty");
            if en == zh {
                same.push(*key);
            }
        }
        // Nothing reads the same in both languages: even the product name
        // appears inside sentences that are translated.
        assert!(same.is_empty(), "never translated: {same:?}");
    }

    #[test]
    fn both_languages_use_the_same_placeholders() {
        for ((key, en), (_, zh)) in EN_US.entries().iter().zip(ZH_TW.entries().iter()) {
            let mut en_names = placeholders(en);
            let mut zh_names = placeholders(zh);
            en_names.sort_unstable();
            zh_names.sort_unstable();
            assert_eq!(en_names, zh_names, "{key}");
        }
    }

    #[test]
    fn the_spec_sentences_are_used_verbatim() {
        assert_eq!(ZH_TW.wrong_password, "密碼不正確，請再試一次。");
        assert_eq!(
            EN_US.wrong_password,
            "That password isn't right. Try again."
        );
        assert_eq!(ZH_TW.quiet_hint, "按任意鍵或點一下來解鎖");
        assert_eq!(ZH_TW.while_you_were_away, "你離開時收到的通知");
        assert_eq!(
            EN_US.service_not_responding,
            "The sign-in service isn't responding. Restart the computer."
        );
        assert_eq!(
            ZH_TW.show_notifications_on_lock_screen,
            "在鎖定畫面上顯示通知"
        );
    }

    #[test]
    fn filling_replaces_known_placeholders_and_keeps_unknown_ones() {
        assert_eq!(
            fill(ZH_TW.wrong_password_wait, &[("n", "4")]),
            "密碼不正確。請在 4 秒後再試一次。"
        );
        assert_eq!(
            fill(EN_US.password_for, &[("name", "Tim")]),
            "Password for Tim"
        );
        assert_eq!(fill("{a} and {b}", &[("a", "1")]), "1 and {b}");
        assert_eq!(fill("no brace {", &[]), "no brace {");
        // A value that itself looks like a placeholder is not filled again.
        assert_eq!(fill("{a}{b}", &[("a", "{b}"), ("b", "x")]), "{b}x");
    }

    #[test]
    fn languages_resolve_from_locales() {
        assert_eq!(
            Language::from_locale_name("zh_TW.UTF-8"),
            Some(Language::ZhTw)
        );
        assert_eq!(
            Language::from_locale_name("zh-Hant-TW"),
            Some(Language::ZhTw)
        );
        assert_eq!(
            Language::from_locale_name("en_GB.UTF-8"),
            Some(Language::EnUs)
        );
        assert_eq!(Language::from_locale_name("lzh_TW"), None);
        assert_eq!(Language::from_locale_name("fr_FR.UTF-8"), None);
        assert_eq!(Language::from_locale_name(""), None);
        assert_eq!(
            Language::for_screen(Some("fr_FR.UTF-8"), Some("zh_TW.UTF-8")),
            Language::ZhTw
        );
        assert_eq!(
            Language::for_screen(Some("en_US.UTF-8"), Some("zh_TW.UTF-8")),
            Language::EnUs
        );
        assert_eq!(Language::for_screen(None, None), Language::EnUs);
    }

    #[test]
    fn every_fallback_stage_has_a_name_in_both_languages() {
        for stage in [
            FallbackStage::NoGraphics,
            FallbackStage::LoginManager,
            FallbackStage::SignInService,
            FallbackStage::DisplayServer,
            FallbackStage::LoginScreenProgram,
            FallbackStage::Supervisor,
        ] {
            assert_ne!(stage.name(Language::ZhTw), stage.name(Language::EnUs));
        }
    }
}

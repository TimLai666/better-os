//! The view model both screens draw (design/login-screen-spec.md sections 3.3
//! to 3.9, 3.12, 3.16, 4.1 and 4.4; the security review's "The password").
//!
//! [`Screen`] holds what the screen shows and moves between the views on
//! [`Event`]s, returning [`Effects`] for the program to carry out: wipe the
//! password field, focus it, type the key that woke the screen, send a
//! request to greetd (the greeter) or to PAM (the lock program), unlock.
//!
//! The view model never sees a password. The field's [`crate::Secret`]
//! belongs to the program; the model only says when it is sent
//! ([`Request::Answer`]) and when it is wiped ([`Effects::wipe_field`]):
//! after it is sent, on every change of view, on Escape, after 60 seconds
//! without input, after a refusal, when PAM's question turns out to be one
//! whose answer is shown, when the machine is about to sleep and when it
//! wakes, and when it restarts or shuts down.
//!
//! Every conversation has a [`ConversationId`], named in its
//! [`Request::Begin`]; a question, a message or an outcome that names any
//! other conversation (one cancelled while its reply was on the way) is
//! dropped here, never applied to the current one.
//!
//! Failures in a row and the wait after them are kept per account in
//! [`Attempts`], which survives switching account and, through its file, a
//! restart of the program ([`Effects::attempts_changed`]).
//!
//! Time is passed in as `CLOCK_BOOTTIME` milliseconds ([`boot_time_now`]),
//! so every timer (the wait after failures, the minute without input,
//! greetd's 60 seconds) is tested without sleeping, keeps counting through
//! suspend, and means the same after a restart of the program.

use std::time::Duration;

use crate::accounts::Account;
use crate::attempts::Attempts;
use crate::copy::{Language, fill};
use crate::pam::{PamText, Refusal, classify};
use crate::power::PowerAction;
use crate::sessions::{Session, menu_sessions, preselected_session};
use crate::wait::seconds_shown;

/// `CLOCK_BOOTTIME` milliseconds: since boot, counting suspend.
pub type Millis = u64;

/// The time now, for [`Screen::handle`]. `None` only when the kernel refuses
/// `CLOCK_BOOTTIME`, which Linux has offered since 2.6.39.
pub fn boot_time_now() -> Option<Millis> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes into the struct it is given.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
        return None;
    }
    let seconds = u64::try_from(time.tv_sec).ok()?;
    let nanos = u64::try_from(time.tv_nsec).ok()?;
    Some(
        seconds
            .saturating_mul(1000)
            .saturating_add(nanos / 1_000_000),
    )
}

/// Names one PAM conversation (greetd session, or `pam_start` in the lock
/// program).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConversationId(pub u64);

/// Without input for this long, the field is wiped, and the lock screen goes
/// back to its quiet view.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the screen waits for greetd or PAM to answer a password before
/// it cancels (spec section 5.10, stage 8).
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

fn millis(duration: Duration) -> Millis {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenKind {
    /// `better-greeter`, under greetd.
    Login,
    /// `better-lock`, in the person's session.
    Lock,
}

/// The views of the spec.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    /// The clock alone (the lock screen when it wakes, L3 (b)).
    Quiet,
    /// The identity group and the password field.
    Entry,
    /// "Choose an account" (login screen, more than one account).
    Chooser,
    /// The session menu open over the entry view (login screen).
    SessionMenu,
    /// PAM asks for the current and a new password, one prompt at a time.
    PasswordChange,
    /// "Starting {session}…", then the screen leaves (login screen).
    Starting,
    /// "{session} didn't start." (login screen).
    FailedToStart,
    /// "Going to sleep…", "Restarting…", "Shutting down…".
    PowerOverlay(PowerAction),
}

/// The field's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Takes input.
    Ready,
    /// greetd or PAM is working; PAM's own delay runs here.
    Checking { since: Millis },
    /// The wait after failures; the field is disabled until `until`.
    Waiting { until: Millis },
    /// The sign-in service does not answer; only the power menu works.
    Unavailable,
}

/// A key the screen reacts to. Characters typed into a field reach the
/// program's field; the model only needs to know that something was typed
/// ([`Event::FieldEdited`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Printable(char),
    Enter,
    Space,
    Escape,
    Other,
}

/// What a PAM prompt's answer is shown as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptKind {
    /// Hidden (greetd's `secret`, `PAM_PROMPT_ECHO_OFF`).
    Secret,
    /// Shown (greetd's `visible`, `PAM_PROMPT_ECHO_ON`).
    Visible,
}

/// The two messages PAM may send that are not questions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PamMessageKind {
    Info,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// The screen has appeared (the greeter's first frame, the lock taken).
    Shown,
    Key(Key),
    /// A click or a touch anywhere.
    Pointer,
    /// The person typed or deleted in a field.
    FieldEdited,
    FieldFocus(bool),
    CapsLock(bool),
    /// Enter in the field, or the submit button.
    Submit,
    ShowChooser,
    ChooseAccount(usize),
    ChooseOtherAccount,
    /// The "User name" field of "Other account…".
    UserNameEdited(String),
    Back,
    OpenSessionMenu,
    ChooseSession(String),
    /// Whether the GNOME unlock service answers on the system bus.
    GnomeUnlockAnswers(bool),
    /// The power item was refused (logind said `challenge` on the login
    /// screen); carries the sentence to show.
    PowerRefused(&'static str),
    /// logind was asked to sleep, restart or shut down.
    PowerChosen(PowerAction),
    /// The machine is about to sleep, whatever asked for it (the power menu,
    /// the lid, idle): logind's `PrepareForSleep(true)`.
    Suspending,
    /// The machine woke from sleep: `PrepareForSleep(false)`.
    Resumed,
    /// PAM asks a question in a conversation.
    Prompt {
        conversation: ConversationId,
        kind: PromptKind,
        text: String,
    },
    /// PAM says something.
    PamMessage {
        conversation: ConversationId,
        kind: PamMessageKind,
        text: String,
    },
    /// PAM refused.
    Refused {
        conversation: ConversationId,
        refusal: Refusal,
    },
    /// PAM said yes to every question.
    Authenticated {
        conversation: ConversationId,
    },
    /// greetd's `success` to `start_session`: the greeter leaves.
    SessionStarted,
    /// The greeter found its record of a session that ended within 15
    /// seconds of starting (spec section 3.16).
    FailedToStart {
        session_id: String,
    },
    TryAgain,
    ChooseAnotherSession,
    /// The GNOME unlock service did not answer right before GNOME would
    /// start (spec section 5.6).
    GnomeCannotLock,
    /// greetd's socket is gone or does not answer at all.
    ServiceNotResponding,
    /// Time passed.
    Tick,
}

/// What the program sends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// greetd's `create_session`; PAM's `pam_start` for the lock program.
    /// Every reply to it is reported with `conversation`.
    Begin {
        user: String,
        conversation: ConversationId,
    },
    /// Answer the current prompt with the field's [`crate::Secret`], then
    /// wipe it.
    Answer,
    /// Answer an `info` or `error` message (greetd needs an empty response).
    Acknowledge,
    /// greetd's `cancel_session`; ending the PAM conversation.
    Cancel,
    /// greetd's `start_session` with this session.
    StartSession { session_id: String },
    /// Call logind.
    Power(PowerAction),
}

/// An animation to run; with animations off, it is instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Transition {
    /// The card comes in from the end (250 ms).
    CardIn,
    /// Back to the quiet view (200 ms).
    QuietReturn,
    /// The card and the clock leave after a login (600 / 400 ms).
    Leave,
}

/// What the program does after an event, in this order: requests, wipe,
/// focus, type.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Effects {
    pub requests: Vec<Request>,
    pub wipe_field: bool,
    pub focus_field: bool,
    /// The printable key that woke the quiet view; it is the field's first
    /// character (Enter, Space and Escape are not typed).
    pub type_into_field: Option<char>,
    /// The lock program unlocks the session (`unlock_and_destroy`).
    pub unlock: bool,
    pub transition: Option<Transition>,
    /// [`Screen::attempts`] changed: save it to the program's record.
    pub attempts_changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Selection {
    Listed(usize),
    Other { user_name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Conversation {
    NotStarted,
    /// Begun; no question yet.
    Begun,
    Prompted {
        kind: PromptKind,
        text: PamText,
    },
    /// The answer is with greetd or PAM.
    Answered,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Problem {
    WrongPassword,
    AccountExpired,
    CannotSignIn { pam_text: String },
    PamError(String),
    ServiceNotResponding,
    GnomeCannotLock,
    NoSession,
    PowerRefused(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Notice {
    PasswordExpired,
    PasswordChanged,
    PamInfo(String),
}

/// The message line's two looks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageKind {
    /// The critical colour after an error glyph.
    Error,
    /// Secondary text after an information glyph.
    Notice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageLine {
    pub kind: MessageKind,
    pub text: String,
    /// PAM's own text, in secondary text below the title.
    pub detail: Option<String>,
}

/// One row of "Choose an account".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChooserRow {
    pub index: usize,
    pub name: String,
    pub initial: String,
    pub tint: u8,
    pub icon_file: String,
    /// 「上次使用」 / "Last used" for the preselected account.
    pub caption: Option<&'static str>,
}

/// One item of the session menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionItem {
    pub id: String,
    pub name: String,
    pub checked: bool,
    /// 「上次使用」 for the account's last session.
    pub caption: Option<&'static str>,
}

/// The "didn't start" panel of spec section 3.16.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedToStart {
    pub title: String,
    pub body: &'static str,
    /// The accent button.
    pub try_again: &'static str,
    pub choose_another: &'static str,
}

/// The screen's state.
pub struct Screen {
    kind: ScreenKind,
    system_language: Option<String>,
    view: View,
    phase: Phase,
    conversation: Conversation,
    /// The current conversation's id, and the next one's.
    conversation_id: ConversationId,
    next_conversation: u64,
    queued_submit: bool,
    changing_password: bool,
    attempts: Attempts,
    /// The time of the event being handled.
    now: Millis,
    problem: Option<Problem>,
    notice: Option<Notice>,
    caps_lock: bool,
    field_focused: bool,
    input_method_notice: bool,
    last_input: Millis,
    idle_handled: bool,
    escape_armed: bool,
    accounts: Vec<Account>,
    selection: Selection,
    sessions: Vec<Session>,
    gnome_unlock_answers: bool,
    chosen_session: Option<String>,
    failed_session: Option<String>,
}

impl Screen {
    /// The login screen. `accounts` in [`crate::accounts::order_accounts`]'s
    /// order, `sessions` in [`crate::sessions::order_sessions`]'s; with no
    /// account listed the screen asks for a user name.
    pub fn login(
        accounts: Vec<Account>,
        sessions: Vec<Session>,
        system_language: Option<String>,
        now: Millis,
    ) -> Self {
        let selection = if accounts.is_empty() {
            Selection::Other {
                user_name: String::new(),
            }
        } else {
            Selection::Listed(0)
        };
        let mut screen = Self::base(ScreenKind::Login, accounts, sessions, system_language, now);
        screen.selection = selection;
        screen.view = View::Entry;
        screen.chosen_session = screen.preselected_session_id();
        screen
    }

    /// The lock screen for the person whose session it is.
    /// `input_method_was_active` shows the input method notice once.
    pub fn lock(
        person: Account,
        system_language: Option<String>,
        input_method_was_active: bool,
        now: Millis,
    ) -> Self {
        let mut screen = Self::base(
            ScreenKind::Lock,
            vec![person],
            Vec::new(),
            system_language,
            now,
        );
        screen.input_method_notice = input_method_was_active;
        screen
    }

    /// Starts from the failures the program's record kept
    /// ([`crate::attempts`]), so a restart does not end a wait.
    pub fn with_attempts(mut self, attempts: Attempts) -> Self {
        self.attempts = attempts;
        self
    }

    /// The failures to save when [`Effects::attempts_changed`] is set.
    pub fn attempts(&self) -> &Attempts {
        &self.attempts
    }

    /// The conversation replies are expected for, if one is running.
    pub fn conversation(&self) -> Option<ConversationId> {
        (self.conversation != Conversation::NotStarted).then_some(self.conversation_id)
    }

    fn base(
        kind: ScreenKind,
        accounts: Vec<Account>,
        sessions: Vec<Session>,
        system_language: Option<String>,
        now: Millis,
    ) -> Self {
        Self {
            kind,
            system_language,
            view: View::Quiet,
            phase: Phase::Ready,
            conversation: Conversation::NotStarted,
            conversation_id: ConversationId(0),
            next_conversation: 1,
            queued_submit: false,
            changing_password: false,
            attempts: Attempts::default(),
            now,
            problem: None,
            notice: None,
            caps_lock: false,
            field_focused: false,
            input_method_notice: false,
            last_input: now,
            idle_handled: false,
            escape_armed: false,
            accounts,
            selection: Selection::Listed(0),
            sessions,
            gnome_unlock_answers: true,
            chosen_session: None,
            failed_session: None,
        }
    }

    // ---- What the screen shows --------------------------------------------

    pub fn kind(&self) -> ScreenKind {
        self.kind
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The chosen account's language when Better OS ships it, else the
    /// system's (spec section 5.7).
    pub fn language(&self) -> Language {
        let account_language = self.account().map(|account| account.language.as_str());
        Language::for_screen(account_language, self.system_language.as_deref())
    }

    /// The account the entry view is for; `None` for "Other account…".
    pub fn account(&self) -> Option<&Account> {
        match self.selection {
            Selection::Listed(index) => self.accounts.get(index),
            Selection::Other { .. } => None,
        }
    }

    /// Whether the entry view shows the "User name" field above the password.
    pub fn shows_user_name_field(&self) -> bool {
        matches!(self.selection, Selection::Other { .. })
    }

    /// The screen's accessible name.
    pub fn screen_name(&self) -> &'static str {
        let copy = self.language().copy();
        match self.kind {
            ScreenKind::Login => copy.login_screen,
            ScreenKind::Lock => copy.locked,
        }
    }

    /// The quiet view's hint.
    pub fn quiet_hint(&self) -> Option<&'static str> {
        (self.view == View::Quiet).then(|| self.language().copy().quiet_hint)
    }

    /// The Caption line under the name.
    pub fn caption(&self) -> Option<&'static str> {
        let copy = self.language().copy();
        match (self.phase, self.kind) {
            (Phase::Checking { .. }, _) => Some(copy.checking),
            (_, ScreenKind::Lock) => Some(copy.locked),
            (_, ScreenKind::Login) => None,
        }
    }

    /// Whether the field and its buttons take input.
    pub fn field_enabled(&self) -> bool {
        self.phase == Phase::Ready && matches!(self.view, View::Entry | View::PasswordChange)
    }

    /// Whether a progress ring replaces the submit glyph.
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Checking { .. })
    }

    /// The field's placeholder: 「密碼」 for the ordinary prompt, Better OS's
    /// words for a known `pam_unix` prompt, PAM's text for any other.
    pub fn field_label(&self) -> String {
        let language = self.language();
        match &self.conversation {
            Conversation::Prompted { text, .. } => text.text(language),
            _ => language.copy().password.to_string(),
        }
    }

    /// Whether the field hides what is typed.
    pub fn field_is_secret(&self) -> bool {
        !matches!(
            self.conversation,
            Conversation::Prompted {
                kind: PromptKind::Visible,
                ..
            }
        )
    }

    /// The field's accessible name: 「{name} 的密碼」 for the password,
    /// the label for any other question.
    pub fn field_accessible_name(&self) -> String {
        let language = self.language();
        let is_password = matches!(
            self.conversation,
            Conversation::Prompted {
                text: PamText::Password,
                ..
            }
        ) || !matches!(self.conversation, Conversation::Prompted { .. });
        match (is_password, self.account()) {
            (true, Some(account)) => fill(
                language.copy().password_for,
                &[("name", account.display_name())],
            ),
            _ => self.field_label(),
        }
    }

    /// 「登入」 / "Sign in", or 「解鎖」 / "Unlock".
    pub fn submit_label(&self) -> &'static str {
        let copy = self.language().copy();
        match self.kind {
            ScreenKind::Login => copy.sign_in,
            ScreenKind::Lock => copy.unlock,
        }
    }

    /// The message line under the field, by priority: an error, then a
    /// notice from PAM, then Caps Lock (only while the field has focus),
    /// then the input method notice.
    pub fn message_line(&self, now: Millis) -> Option<MessageLine> {
        let language = self.language();
        let copy = language.copy();
        let error = |text: String, detail: Option<String>| MessageLine {
            kind: MessageKind::Error,
            text,
            detail,
        };
        let notice = |text: String| MessageLine {
            kind: MessageKind::Notice,
            text,
            detail: None,
        };
        if let Some(problem) = &self.problem {
            return Some(match problem {
                Problem::WrongPassword => match self.phase {
                    Phase::Waiting { until } => {
                        let left = seconds_shown(Duration::from_millis(until.saturating_sub(now)));
                        let text = if left <= 1 {
                            copy.wrong_password_wait_one.to_string()
                        } else {
                            fill(copy.wrong_password_wait, &[("n", &left.to_string())])
                        };
                        error(text, None)
                    }
                    _ => error(copy.wrong_password.to_string(), None),
                },
                Problem::AccountExpired => error(copy.account_expired.to_string(), None),
                Problem::CannotSignIn { pam_text } => error(
                    copy.account_locked.to_string(),
                    Some(pam_text.clone()).filter(|text| !text.is_empty()),
                ),
                Problem::PamError(text) => error(text.clone(), None),
                Problem::ServiceNotResponding => {
                    error(copy.service_not_responding.to_string(), None)
                }
                Problem::GnomeCannotLock => error(copy.gnome_cannot_lock.to_string(), None),
                Problem::NoSession => error(copy.no_session.to_string(), None),
                Problem::PowerRefused(text) => error((*text).to_string(), None),
            });
        }
        if let Some(found) = &self.notice {
            return Some(notice(match found {
                Notice::PasswordExpired => copy.password_expired.to_string(),
                Notice::PasswordChanged => copy.password_changed.to_string(),
                Notice::PamInfo(text) => text.clone(),
            }));
        }
        if self.caps_lock && self.field_focused && self.field_enabled() {
            return Some(notice(copy.caps_lock_on.to_string()));
        }
        if self.input_method_notice && self.view == View::Entry {
            return Some(notice(copy.input_method_paused.to_string()));
        }
        None
    }

    /// Whether the bottom row shows "Switch account". Not during a wait: the
    /// chooser is not a way out of it (the wait stays with the account
    /// anyway, in [`Attempts`]).
    pub fn shows_switch_account(&self) -> bool {
        self.kind == ScreenKind::Login
            && self.accounts.len() > 1
            && self.view == View::Entry
            && !matches!(self.phase, Phase::Waiting { .. })
    }

    /// The rows of "Choose an account", the preselected (most recent) first
    /// with 「上次使用」.
    pub fn chooser_rows(&self) -> Vec<ChooserRow> {
        let last_used = self.language().copy().last_used;
        self.accounts
            .iter()
            .enumerate()
            .map(|(index, account)| ChooserRow {
                index,
                name: account.display_name().to_string(),
                initial: account.initial(),
                tint: account.tint(),
                icon_file: account.icon_file.clone(),
                caption: (index == 0 && account.login_time > 0).then_some(last_used),
            })
            .collect()
    }

    fn listed_sessions(&self) -> Vec<&Session> {
        menu_sessions(&self.sessions, self.gnome_unlock_answers)
    }

    fn preselected_session_id(&self) -> Option<String> {
        let last = self.account().map(|account| account.session.as_str());
        preselected_session(&self.listed_sessions(), last).map(|session| session.id.clone())
    }

    fn session_name(&self, id: &str) -> String {
        self.sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.name(self.language()).to_string())
            .unwrap_or_else(|| id.to_string())
    }

    /// The chosen session's id.
    pub fn chosen_session(&self) -> Option<&str> {
        self.chosen_session.as_deref()
    }

    /// The bottom row's session button, 「工作階段：{name}」.
    pub fn session_button_label(&self) -> Option<String> {
        if self.kind != ScreenKind::Login {
            return None;
        }
        let id = self.chosen_session.as_deref()?;
        Some(fill(
            self.language().copy().session_button,
            &[("name", &self.session_name(id))],
        ))
    }

    /// The session menu's items.
    pub fn session_items(&self) -> Vec<SessionItem> {
        let language = self.language();
        let last = self.account().map(|account| account.session.as_str());
        self.listed_sessions()
            .into_iter()
            .map(|session| SessionItem {
                id: session.id.clone(),
                name: session.name(language).to_string(),
                checked: self.chosen_session.as_deref() == Some(session.id.as_str()),
                caption: (last == Some(session.id.as_str())).then_some(language.copy().last_used),
            })
            .collect()
    }

    /// 「正在啟動 {session}…」 in the starting view.
    pub fn starting_text(&self) -> Option<String> {
        if self.view != View::Starting {
            return None;
        }
        let id = self.chosen_session.as_deref()?;
        Some(fill(
            self.language().copy().starting_session,
            &[("session", &self.session_name(id))],
        ))
    }

    /// The panel of a session that did not start.
    pub fn failed_to_start(&self) -> Option<FailedToStart> {
        if self.view != View::FailedToStart {
            return None;
        }
        let copy = self.language().copy();
        let id = self.failed_session.as_deref()?;
        Some(FailedToStart {
            title: fill(
                copy.session_did_not_start,
                &[("session", &self.session_name(id))],
            ),
            body: copy.session_did_not_start_body,
            try_again: copy.try_again,
            choose_another: copy.choose_another_session,
        })
    }

    /// The power overlay's line.
    pub fn overlay_text(&self) -> Option<&'static str> {
        match self.view {
            View::PowerOverlay(action) => Some(action.overlay(self.language())),
            _ => None,
        }
    }

    // ---- Transitions ------------------------------------------------------

    fn user(&self) -> Option<String> {
        match &self.selection {
            Selection::Listed(index) => self.accounts.get(*index).map(|a| a.user_name.clone()),
            Selection::Other { user_name } => {
                let name = user_name.trim();
                (!name.is_empty()).then(|| name.to_string())
            }
        }
    }

    /// The end of the chosen account's wait, if one runs now.
    fn current_wait(&self) -> Option<Millis> {
        self.user()
            .and_then(|user| self.attempts.waiting_until(&user, self.now))
    }

    /// Puts the chosen account's own wait in force (or ends one that
    /// belonged to another account).
    fn refresh_wait(&mut self) {
        if matches!(self.phase, Phase::Ready | Phase::Waiting { .. }) {
            match self.current_wait() {
                Some(until) => {
                    self.phase = Phase::Waiting { until };
                    if self.problem.is_none() {
                        self.problem = Some(Problem::WrongPassword);
                    }
                }
                None => {
                    if matches!(self.phase, Phase::Waiting { .. }) {
                        self.phase = Phase::Ready;
                    }
                }
            }
        }
    }

    fn set_view(&mut self, view: View, effects: &mut Effects) {
        if self.view != view {
            self.view = view;
            effects.wipe_field = true;
            self.escape_armed = false;
        }
    }

    fn begin(&mut self, effects: &mut Effects) {
        if self.conversation != Conversation::NotStarted {
            return;
        }
        let Some(user) = self.user() else { return };
        self.refresh_wait();
        if self.phase != Phase::Ready {
            return;
        }
        self.conversation_id = ConversationId(self.next_conversation);
        self.next_conversation = self.next_conversation.wrapping_add(1);
        effects.requests.push(Request::Begin {
            user,
            conversation: self.conversation_id,
        });
        self.conversation = Conversation::Begun;
    }

    fn cancel(&mut self, effects: &mut Effects) {
        if self.conversation != Conversation::NotStarted {
            effects.requests.push(Request::Cancel);
        }
        if self.queued_submit {
            // What was typed for a question that never came is not kept.
            effects.wipe_field = true;
        }
        self.conversation = Conversation::NotStarted;
        self.queued_submit = false;
        self.changing_password = false;
        if matches!(self.phase, Phase::Checking { .. }) {
            self.phase = Phase::Ready;
        }
    }

    fn enter(&mut self, effects: &mut Effects) {
        self.set_view(View::Entry, effects);
        effects.focus_field = true;
        self.refresh_wait();
        if self.phase == Phase::Ready {
            self.begin(effects);
        }
    }

    fn answer(&mut self, now: Millis, effects: &mut Effects) {
        if let Conversation::Prompted { text, .. } = &self.conversation
            && text.is_password_change()
        {
            self.changing_password = true;
        }
        effects.requests.push(Request::Answer);
        effects.wipe_field = true;
        self.conversation = Conversation::Answered;
        self.phase = Phase::Checking { since: now };
        self.queued_submit = false;
    }

    fn submit(&mut self, now: Millis, effects: &mut Effects) {
        if self.phase != Phase::Ready || !matches!(self.view, View::Entry | View::PasswordChange) {
            return;
        }
        self.problem = None;
        self.input_method_notice = false;
        match self.conversation {
            Conversation::Prompted { .. } => {
                if !matches!(self.notice, Some(Notice::PasswordExpired)) {
                    self.notice = None;
                }
                self.answer(now, effects)
            }
            Conversation::Answered => {}
            Conversation::NotStarted | Conversation::Begun => {
                // The person typed before PAM's question arrived: the answer
                // goes with the first question if it is a secret one.
                self.begin(effects);
                if self.conversation == Conversation::Begun {
                    self.notice = None;
                    self.queued_submit = true;
                    self.phase = Phase::Checking { since: now };
                }
            }
        }
    }

    fn refused(&mut self, refusal: Refusal, now: Millis, effects: &mut Effects) {
        let user = self.user();
        self.cancel(effects);
        self.notice = None;
        effects.wipe_field = true;
        effects.focus_field = true;
        if self.view == View::PasswordChange {
            self.set_view(View::Entry, effects);
        }
        match refusal {
            Refusal::WrongPassword => {
                self.problem = Some(Problem::WrongPassword);
                let until = user.map(|user| {
                    effects.attempts_changed = true;
                    self.attempts.record_failure(&user, now).wait_until
                });
                match until {
                    Some(until) if until > now => self.phase = Phase::Waiting { until },
                    _ => {
                        self.phase = Phase::Ready;
                        self.begin(effects);
                    }
                }
            }
            Refusal::AccountExpired => {
                self.problem = Some(Problem::AccountExpired);
                self.phase = Phase::Ready;
            }
            Refusal::CannotSignIn { pam_text } => {
                self.problem = Some(Problem::CannotSignIn { pam_text });
                self.phase = Phase::Ready;
            }
        }
    }

    fn note_input(&mut self, now: Millis) {
        self.last_input = now;
        self.idle_handled = false;
    }

    fn quiet(&mut self, effects: &mut Effects) {
        self.cancel(effects);
        self.set_view(View::Quiet, effects);
        effects.wipe_field = true;
        effects.transition = Some(Transition::QuietReturn);
    }

    fn tick(&mut self, now: Millis, effects: &mut Effects) {
        match self.phase {
            Phase::Waiting { until } if now >= until => {
                self.phase = Phase::Ready;
                effects.focus_field = true;
                if matches!(self.view, View::Entry | View::PasswordChange) {
                    self.begin(effects);
                }
            }
            Phase::Checking { since } if now.saturating_sub(since) >= millis(CHECK_TIMEOUT) => {
                // A PAM module that hangs (spec section 5.10, stage 8):
                // cancel, say so, and let the person try again.
                self.cancel(effects);
                self.phase = Phase::Ready;
                self.problem = Some(Problem::ServiceNotResponding);
                effects.wipe_field = true;
            }
            _ => {}
        }
        let idle = now.saturating_sub(self.last_input) >= millis(IDLE_TIMEOUT);
        let busy = matches!(self.phase, Phase::Checking { .. });
        let interactive = matches!(
            self.view,
            View::Entry | View::Chooser | View::SessionMenu | View::PasswordChange
        );
        if idle && !busy && interactive && !self.idle_handled {
            self.idle_handled = true;
            effects.wipe_field = true;
            match self.kind {
                ScreenKind::Lock => self.quiet(effects),
                ScreenKind::Login => {
                    if self.view == View::PasswordChange {
                        // An abandoned password change starts over.
                        self.cancel(effects);
                        self.notice = None;
                        self.set_view(View::Entry, effects);
                    }
                }
            }
        }
    }

    /// Whether a reply belongs to the conversation running now.
    fn is_current(&self, conversation: ConversationId) -> bool {
        self.conversation != Conversation::NotStarted && conversation == self.conversation_id
    }

    /// Moves the screen on one event.
    pub fn handle(&mut self, event: Event, now: Millis) -> Effects {
        self.now = now;
        let mut effects = Effects::default();
        match event {
            Event::Shown => match self.kind {
                ScreenKind::Login => {
                    self.note_input(now);
                    if self.view == View::Entry {
                        effects.focus_field = true;
                        self.refresh_wait();
                        self.begin(&mut effects);
                    }
                }
                ScreenKind::Lock => {
                    self.view = View::Quiet;
                }
            },
            Event::Key(key) => {
                self.note_input(now);
                match (self.view, key) {
                    (View::Quiet, key) => {
                        effects.transition = Some(Transition::CardIn);
                        if let Key::Printable(character) = key {
                            effects.type_into_field = Some(character);
                        }
                        self.enter(&mut effects);
                    }
                    (View::Entry | View::PasswordChange, Key::Escape) => {
                        if self.escape_armed && self.kind == ScreenKind::Lock {
                            self.quiet(&mut effects);
                        } else {
                            effects.wipe_field = true;
                            self.escape_armed = true;
                        }
                    }
                    (View::Entry | View::PasswordChange, Key::Enter) => {
                        self.escape_armed = false;
                        self.submit(now, &mut effects);
                    }
                    (View::Chooser | View::SessionMenu, Key::Escape) => {
                        return self.handle(Event::Back, now);
                    }
                    _ => self.escape_armed = false,
                }
            }
            Event::Pointer => {
                self.note_input(now);
                if self.view == View::Quiet {
                    effects.transition = Some(Transition::CardIn);
                    self.enter(&mut effects);
                }
            }
            Event::FieldEdited => {
                self.note_input(now);
                self.escape_armed = false;
                if self.phase == Phase::Ready
                    && !matches!(
                        self.problem,
                        Some(Problem::ServiceNotResponding) | Some(Problem::GnomeCannotLock)
                    )
                {
                    self.problem = None;
                }
            }
            Event::FieldFocus(focused) => self.field_focused = focused,
            Event::CapsLock(on) => self.caps_lock = on,
            Event::Submit => {
                self.note_input(now);
                self.submit(now, &mut effects);
            }
            Event::ShowChooser => {
                self.note_input(now);
                if self.shows_switch_account() {
                    self.cancel(&mut effects);
                    self.set_view(View::Chooser, &mut effects);
                }
            }
            Event::ChooseAccount(index) => {
                self.note_input(now);
                if self.view == View::Chooser && index < self.accounts.len() {
                    self.selection = Selection::Listed(index);
                    self.after_account_change(&mut effects);
                }
            }
            Event::ChooseOtherAccount => {
                self.note_input(now);
                if self.view == View::Chooser {
                    self.selection = Selection::Other {
                        user_name: String::new(),
                    };
                    self.after_account_change(&mut effects);
                }
            }
            Event::UserNameEdited(name) => {
                self.note_input(now);
                if let Selection::Other { user_name } = &mut self.selection {
                    *user_name = name;
                    // A new name is a new conversation, with its own wait.
                    self.cancel(&mut effects);
                    if matches!(self.phase, Phase::Waiting { .. }) {
                        self.phase = Phase::Ready;
                        self.problem = None;
                    }
                }
            }
            Event::Back => {
                self.note_input(now);
                if matches!(self.view, View::Chooser | View::SessionMenu) {
                    self.enter(&mut effects);
                }
            }
            Event::OpenSessionMenu => {
                self.note_input(now);
                if self.kind == ScreenKind::Login
                    && matches!(self.view, View::Entry | View::FailedToStart)
                {
                    self.set_view(View::SessionMenu, &mut effects);
                }
            }
            Event::ChooseSession(id) => {
                self.note_input(now);
                if self.view == View::SessionMenu
                    && self
                        .listed_sessions()
                        .iter()
                        .any(|session| session.id == id)
                {
                    self.chosen_session = Some(id);
                    self.problem = None;
                    self.enter(&mut effects);
                }
            }
            Event::GnomeUnlockAnswers(answers) => {
                self.gnome_unlock_answers = answers;
                let still_listed = self.chosen_session.as_deref().is_some_and(|id| {
                    self.listed_sessions()
                        .iter()
                        .any(|session| session.id == id)
                });
                if !still_listed {
                    self.chosen_session = self.preselected_session_id();
                }
            }
            Event::PowerRefused(text) => self.problem = Some(Problem::PowerRefused(text)),
            Event::PowerChosen(action) => {
                self.cancel(&mut effects);
                self.set_view(View::PowerOverlay(action), &mut effects);
                effects.wipe_field = true;
                effects.requests.push(Request::Power(action));
            }
            Event::Suspending => {
                // However the sleep was asked for, nothing half typed stays.
                effects.wipe_field = true;
                self.cancel(&mut effects);
                if self.kind == ScreenKind::Lock && self.view != View::Quiet {
                    self.set_view(View::Quiet, &mut effects);
                }
            }
            Event::Resumed => {
                self.note_input(now);
                effects.wipe_field = true;
                match self.kind {
                    ScreenKind::Lock => {
                        self.cancel(&mut effects);
                        self.set_view(View::Quiet, &mut effects);
                    }
                    ScreenKind::Login => {
                        if matches!(self.view, View::PowerOverlay(_) | View::Entry) {
                            self.enter(&mut effects);
                        }
                    }
                }
            }
            Event::Prompt {
                conversation,
                kind,
                text,
            } => {
                if self.is_current(conversation) {
                    self.prompted(kind, &text, &mut effects);
                }
            }
            Event::PamMessage {
                conversation,
                kind,
                text,
            } => {
                if !self.is_current(conversation) {
                    return effects;
                }
                effects.requests.push(Request::Acknowledge);
                match (classify(&text), kind) {
                    (PamText::PasswordExpired, _) => self.notice = Some(Notice::PasswordExpired),
                    (PamText::AccountExpired, _) => self.problem = Some(Problem::AccountExpired),
                    (_, PamMessageKind::Info) => {
                        self.notice = Some(Notice::PamInfo(text.trim().to_string()))
                    }
                    (_, PamMessageKind::Error) => {
                        self.problem = Some(Problem::PamError(text.trim().to_string()))
                    }
                }
            }
            Event::Refused {
                conversation,
                refusal,
            } => {
                if self.is_current(conversation) {
                    self.refused(refusal, now, &mut effects);
                }
            }
            Event::Authenticated { conversation } => {
                if !self.is_current(conversation) {
                    return effects;
                }
                if let Some(user) = self.user()
                    && self.attempts.get(&user).failures > 0
                {
                    self.attempts.record_success(&user);
                    effects.attempts_changed = true;
                }
                self.problem = None;
                self.conversation = Conversation::NotStarted;
                effects.wipe_field = true;
                self.notice = self.changing_password.then_some(Notice::PasswordChanged);
                self.changing_password = false;
                match self.kind {
                    ScreenKind::Lock => {
                        self.phase = Phase::Ready;
                        effects.unlock = true;
                    }
                    ScreenKind::Login => match self.chosen_session.clone() {
                        Some(session_id) => {
                            self.set_view(View::Starting, &mut effects);
                            effects.requests.push(Request::StartSession { session_id });
                        }
                        None => {
                            // Nothing in the session menu to start.
                            effects.requests.push(Request::Cancel);
                            self.phase = Phase::Ready;
                            self.problem = Some(Problem::NoSession);
                        }
                    },
                }
            }
            Event::SessionStarted => {
                if self.view == View::Starting {
                    effects.transition = Some(Transition::Leave);
                }
            }
            Event::FailedToStart { session_id } => {
                if self.kind == ScreenKind::Login {
                    self.cancel(&mut effects);
                    self.failed_session = Some(session_id);
                    self.set_view(View::FailedToStart, &mut effects);
                }
            }
            Event::TryAgain => {
                self.note_input(now);
                if self.view == View::FailedToStart {
                    self.enter(&mut effects);
                }
            }
            Event::ChooseAnotherSession => {
                self.note_input(now);
                if self.view == View::FailedToStart {
                    self.set_view(View::SessionMenu, &mut effects);
                }
            }
            Event::GnomeCannotLock => {
                self.cancel(&mut effects);
                self.phase = Phase::Ready;
                self.notice = None;
                self.gnome_unlock_answers = false;
                self.chosen_session = self.preselected_session_id();
                self.set_view(View::Entry, &mut effects);
                self.problem = Some(Problem::GnomeCannotLock);
                effects.focus_field = true;
            }
            Event::ServiceNotResponding => {
                self.cancel(&mut effects);
                self.phase = Phase::Unavailable;
                self.problem = Some(Problem::ServiceNotResponding);
                effects.wipe_field = true;
            }
            Event::Tick => self.tick(now, &mut effects),
        }
        effects
    }

    fn after_account_change(&mut self, effects: &mut Effects) {
        // The failures stay with the account that made them (Attempts); the
        // newly chosen account's own wait, if any, is put in force by enter.
        self.problem = None;
        self.notice = None;
        self.cancel(effects);
        self.chosen_session = self.preselected_session_id();
        self.enter(effects);
    }

    fn prompted(&mut self, kind: PromptKind, text: &str, effects: &mut Effects) {
        let text = classify(text);
        if text.is_password_change() && self.view == View::Entry {
            self.set_view(View::PasswordChange, effects);
        }
        let queued = self.queued_submit && kind == PromptKind::Secret;
        self.conversation = Conversation::Prompted { kind, text };
        if kind == PromptKind::Visible {
            // The field is about to show what it holds: anything typed while
            // it was hidden, or queued for a secret question, goes first.
            effects.wipe_field = true;
        }
        if queued {
            // Sent now with what was typed before the question came.
            effects.requests.push(Request::Answer);
            effects.wipe_field = true;
            self.conversation = Conversation::Answered;
            self.queued_submit = false;
        } else {
            self.queued_submit = false;
            if matches!(self.phase, Phase::Checking { .. }) {
                self.phase = Phase::Ready;
            }
            effects.focus_field = true;
        }
    }
}

#[cfg(test)]
mod tests;

//! Window attention (ticket 106): what happens when an app's window asks for
//! attention, as a default and per app. Everything the page decides is here,
//! without GPUI: which notice it shows about the shell extension and the
//! service, which apps it lists and under which name, what each choice is
//! called and means, and what a pick in a list changes. `render` draws it and
//! `window_attention_bus` reads the shell.
//!
//! The page is its own pane item until ticket 86's System page exists, when
//! its cards move there as a "Window attention" section.

use window_attention_core::{AppId, Choice, Choices, SeenApps};

use crate::i18n::{Locale, shell};
use crate::navigation::Route;
use crate::pages::{PageModule, PageStatus};
use crate::search::SearchEntry;

/// The ids of the page's cards, which the search scrolls to.
pub(crate) mod card {
    pub(crate) const DEFAULT: &str = "attention-default";
    pub(crate) const ADD: &str = "attention-add";
}

pub(crate) const PAGE: PageModule = PageModule {
    route: Route::WindowAttention,
    name: "window-attention",
    status: PageStatus::Built,
    title: |s| s.window_attention,
    search_entries,
    attention: |_| Vec::new(),
    badge: |_| None,
};

fn search_entries(locale: Locale) -> Vec<SearchEntry> {
    let w = copy(locale);
    let page = shell(locale).window_attention;
    vec![
        SearchEntry {
            header: w.default_header,
            description: w.default_description,
            page,
            group: None,
            route: Route::WindowAttention,
            card: card::DEFAULT,
        },
        SearchEntry {
            header: w.add_header,
            description: w.apps_empty,
            page,
            group: Some(w.apps_header),
            route: Route::WindowAttention,
            card: card::ADD,
        },
    ]
}

/// The page's words.
pub(crate) struct WindowAttentionCopy {
    pub(crate) default_header: &'static str,
    pub(crate) default_description: &'static str,
    pub(crate) apps_header: &'static str,
    pub(crate) apps_empty: &'static str,
    pub(crate) add_header: &'static str,
    pub(crate) add_placeholder: &'static str,
    /// `{query}`.
    pub(crate) add_no_result: &'static str,
    /// `{choice}`.
    pub(crate) use_default: &'static str,
    pub(crate) own_choice: &'static str,
    pub(crate) follows_default: &'static str,
    pub(crate) choice_banner: &'static str,
    pub(crate) choice_mark: &'static str,
    pub(crate) choice_nothing: &'static str,
    pub(crate) choice_forward: &'static str,
    pub(crate) explain_banner: &'static str,
    pub(crate) explain_mark: &'static str,
    pub(crate) explain_nothing: &'static str,
    pub(crate) explain_forward: &'static str,
    pub(crate) checking: &'static str,
    pub(crate) turn_on_title: &'static str,
    pub(crate) turn_on_body: &'static str,
    pub(crate) turn_on_extensions_off: &'static str,
    pub(crate) turn_on: &'static str,
    pub(crate) turning_on: &'static str,
    pub(crate) sign_out_title: &'static str,
    pub(crate) sign_out_body: &'static str,
    pub(crate) not_installed_title: &'static str,
    pub(crate) not_installed_body: &'static str,
    pub(crate) failed_title: &'static str,
    pub(crate) unsupported_title: &'static str,
    pub(crate) unsupported_body: &'static str,
    pub(crate) no_shell_title: &'static str,
    pub(crate) no_shell_body: &'static str,
    pub(crate) service_off_title: &'static str,
    pub(crate) service_off_body: &'static str,
    pub(crate) service_problem_title: &'static str,
    pub(crate) start_service: &'static str,
    pub(crate) read_failed: &'static str,
    pub(crate) read_failed_advice: &'static str,
    pub(crate) save_failed: &'static str,
    pub(crate) enable_failed: &'static str,
}

static EN: WindowAttentionCopy = WindowAttentionCopy {
    default_header: "When a window asks for attention",
    default_description: "What happens when an app's window wants you to look at it. Apps below with their own choice use that instead.",
    apps_header: "Apps",
    apps_empty: "Apps appear here after they ask for attention once. You can also add one yourself.",
    add_header: "Add an app",
    add_placeholder: "Search installed apps",
    add_no_result: "No installed app matches \"{query}\".",
    use_default: "Use the default ({choice})",
    own_choice: "Its own choice",
    follows_default: "Follows the default",
    choice_banner: "Banner and taskbar mark",
    choice_mark: "Taskbar mark only",
    choice_nothing: "Nothing",
    choice_forward: "Bring the window forward",
    explain_banner: "GNOME shows \"… is ready\", keeps it in the notification list, and marks the taskbar icon.",
    explain_mark: "The app's taskbar icon is marked until you switch to it. No banner, nothing in the notification list.",
    explain_nothing: "The request is ignored. Nothing is shown.",
    explain_forward: "The window comes to the front at once, as if you had clicked it. Use this only for apps you trust.",
    checking: "Checking the Better OS shell adapter…",
    turn_on_title: "Turn on the Better OS shell adapter",
    turn_on_body: "Better Manager needs this GNOME Shell extension to change what happens when a window asks for attention. Until it is on, GNOME shows its own banner.",
    turn_on_extensions_off: "Extensions are turned off in this session. Turning this on turns extensions back on as well.",
    turn_on: "Turn on",
    turning_on: "Turning on…",
    sign_out_title: "Sign out to finish setting this up",
    sign_out_body: "The Better OS shell adapter was installed after you signed in. GNOME loads it the next time you sign in.",
    not_installed_title: "The Better OS shell adapter is not installed",
    not_installed_body: "It is the better-shell-adapter package, which the Better OS installer puts beside Better Manager. Run the installer again to add it, then sign out and back in.",
    failed_title: "The Better OS shell adapter could not start",
    unsupported_title: "This version of GNOME Shell is not supported",
    unsupported_body: "The Better OS shell adapter works with GNOME Shell 46.",
    no_shell_title: "GNOME Shell is not running",
    no_shell_body: "This setting works only in a GNOME session.",
    service_off_title: "The window attention service is not running",
    service_off_body: "Your choices are saved. Until the service runs, GNOME shows its own banner.",
    service_problem_title: "The window attention service reported a problem",
    start_service: "Start it",
    read_failed: "Your window attention choices could not be read",
    read_failed_advice: "Nothing is changed until the file can be read. Fix or delete the file named below, then open this page again.",
    save_failed: "Your choice could not be saved",
    enable_failed: "The Better OS shell adapter could not be turned on",
};

static ZH_TW: WindowAttentionCopy = WindowAttentionCopy {
    default_header: "視窗要求注意時",
    default_description: "應用程式的視窗想引起你注意時要怎麼做。下方有自己設定的應用程式會照自己的設定。",
    apps_header: "應用程式",
    apps_empty: "應用程式要求過一次注意後就會出現在這裡，也可以自己加入。",
    add_header: "加入應用程式",
    add_placeholder: "搜尋已安裝的應用程式",
    add_no_result: "沒有符合「{query}」的已安裝應用程式。",
    use_default: "使用預設（{choice}）",
    own_choice: "自己的設定",
    follows_default: "跟隨預設",
    choice_banner: "橫幅與工作列標記",
    choice_mark: "只標記工作列",
    choice_nothing: "不做任何事",
    choice_forward: "把視窗帶到最前面",
    explain_banner: "GNOME 會顯示「…視窗已就緒」、留在通知清單裡，並標記工作列圖示。",
    explain_mark: "工作列圖示會一直標記到你切換過去，不跳橫幅，也不會出現在通知清單。",
    explain_nothing: "直接忽略，什麼都不顯示。",
    explain_forward: "視窗會立刻跳到最前面，就像你點了它一樣。只建議用在你信任的應用程式。",
    checking: "正在檢查 Better OS 的 GNOME 擴充功能…",
    turn_on_title: "開啟 Better OS 的 GNOME 擴充功能",
    turn_on_body: "Better Manager 需要這個 GNOME Shell 擴充功能，才能決定視窗要求注意時要做什麼。開啟之前，GNOME 會照原本的方式顯示橫幅。",
    turn_on_extensions_off: "這個工作階段關閉了擴充功能，開啟時也會一併重新開啟擴充功能。",
    turn_on: "開啟",
    turning_on: "正在開啟…",
    sign_out_title: "登出再登入就能完成設定",
    sign_out_body: "Better OS 的 GNOME 擴充功能是在你登入之後才安裝的，GNOME 會在下次登入時載入它。",
    not_installed_title: "沒有安裝 Better OS 的 GNOME 擴充功能",
    not_installed_body: "它是 better-shell-adapter 套件，Better OS 安裝程式會和 Better Manager 一起裝上。重新執行安裝程式，再登出並重新登入就能補上。",
    failed_title: "Better OS 的 GNOME 擴充功能無法啟動",
    unsupported_title: "不支援這個版本的 GNOME Shell",
    unsupported_body: "Better OS 的 GNOME 擴充功能適用於 GNOME Shell 46。",
    no_shell_title: "GNOME Shell 沒有在執行",
    no_shell_body: "這個設定只能在 GNOME 工作階段中使用。",
    service_off_title: "視窗提醒服務沒有在執行",
    service_off_body: "你的設定已經儲存。服務執行之前，GNOME 會照原本的方式顯示橫幅。",
    service_problem_title: "視窗提醒服務回報了問題",
    start_service: "啟動服務",
    read_failed: "無法讀取你的視窗提醒設定",
    read_failed_advice: "在檔案能讀取之前不會做任何變更。請修正或刪除下方的檔案，再重新開啟這一頁。",
    save_failed: "無法儲存你的設定",
    enable_failed: "無法開啟 Better OS 的 GNOME 擴充功能",
};

pub(crate) fn copy(locale: Locale) -> &'static WindowAttentionCopy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN,
    }
}

pub(crate) fn choice_label(locale: Locale, choice: Choice) -> &'static str {
    let w = copy(locale);
    match choice {
        Choice::BannerAndMark => w.choice_banner,
        Choice::MarkOnly => w.choice_mark,
        Choice::Nothing => w.choice_nothing,
        Choice::BringForward => w.choice_forward,
    }
}

pub(crate) fn choice_explanation(locale: Locale, choice: Choice) -> &'static str {
    let w = copy(locale);
    match choice {
        Choice::BannerAndMark => w.explain_banner,
        Choice::MarkOnly => w.explain_mark,
        Choice::Nothing => w.explain_nothing,
        Choice::BringForward => w.explain_forward,
    }
}

/// The default's list: the four choices, in the ticket's order.
pub(crate) fn default_items(locale: Locale) -> Vec<&'static str> {
    Choice::ALL
        .iter()
        .map(|choice| choice_label(locale, *choice))
        .collect()
}

pub(crate) fn default_index(choice: Choice) -> usize {
    Choice::ALL
        .iter()
        .position(|candidate| *candidate == choice)
        .unwrap_or(0)
}

/// An app's list: "Use the default (…)" first, then the four choices.
pub(crate) fn app_items(locale: Locale, default: Choice) -> Vec<String> {
    std::iter::once(
        copy(locale)
            .use_default
            .replace("{choice}", choice_label(locale, default)),
    )
    .chain(
        Choice::ALL
            .iter()
            .map(|choice| choice_label(locale, *choice).to_string()),
    )
    .collect()
}

pub(crate) fn app_index(own: Option<Choice>) -> usize {
    own.map(|choice| default_index(choice) + 1).unwrap_or(0)
}

/// What a pick in an app's list sets: `Some(None)` takes the app's own choice
/// away, `Some(Some(choice))` sets one, and `None` is not an item.
pub(crate) fn app_choice_at(index: usize) -> Option<Option<Choice>> {
    match index {
        0 => Some(None),
        n => Choice::ALL.get(n - 1).map(|choice| Some(*choice)),
    }
}

/// An installed application, as the add box offers it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstalledApp {
    pub(crate) id: AppId,
    pub(crate) name: String,
    /// The desktop entry's `Icon=`: a theme name or a path.
    pub(crate) icon: Option<String>,
}

/// One app in the list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppRow {
    pub(crate) app: AppId,
    pub(crate) name: String,
    pub(crate) own: Option<Choice>,
}

/// The list: every app with its own choice and every app that has asked,
/// once each, by name. A name comes from the installed application first,
/// because that is the name the person sees in the menu, then from what was
/// recorded, then the id.
pub(crate) fn rows(choices: &Choices, seen: &SeenApps, installed: &[InstalledApp]) -> Vec<AppRow> {
    let mut ids: Vec<&AppId> = choices.apps.keys().chain(seen.apps.keys()).collect();
    ids.sort();
    ids.dedup();
    let mut rows: Vec<AppRow> = ids
        .into_iter()
        .map(|app| {
            let recorded = choices
                .apps
                .get(app)
                .map(|own| own.name.as_str())
                .filter(|name| !name.is_empty())
                .or_else(|| seen.apps.get(app).map(|seen| seen.name.as_str()))
                .filter(|name| !name.is_empty());
            let name = installed
                .iter()
                .find(|candidate| &candidate.id == app)
                .map(|candidate| candidate.name.clone())
                .or_else(|| recorded.map(str::to_string))
                .unwrap_or_else(|| app.to_string());
            AppRow {
                app: app.clone(),
                name,
                own: choices.apps.get(app).map(|own| own.choice),
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.app.cmp(&right.app))
    });
    rows
}

/// How many apps the add box suggests at once.
pub(crate) const MAX_SUGGESTIONS: usize = 8;

/// The installed apps the add box suggests for what was typed: not already in
/// the list, matching the name or the id (without `.desktop`) anywhere,
/// ignoring case.
pub(crate) fn suggestions(
    installed: &[InstalledApp],
    listed: &[AppRow],
    query: &str,
) -> Vec<InstalledApp> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<InstalledApp> = installed
        .iter()
        .filter(|candidate| !listed.iter().any(|row| row.app == candidate.id))
        .filter(|candidate| {
            candidate.name.to_lowercase().contains(&query)
                || candidate
                    .id
                    .as_str()
                    .trim_end_matches(".desktop")
                    .to_lowercase()
                    .contains(&query)
        })
        .cloned()
        .collect();
    found.sort_by(|left, right| {
        let starts = |app: &InstalledApp| !app.name.to_lowercase().starts_with(&query);
        starts(left)
            .cmp(&starts(right))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    found.dedup_by(|left, right| left.id == right.id);
    found.truncate(MAX_SUGGESTIONS);
    found
}

/// Adds an app by hand: it gets a choice of its own, starting at the default
/// so nothing about it changes until the person picks something else.
pub(crate) fn add(choices: &mut Choices, app: &InstalledApp) {
    let default = choices.default;
    choices.set(app.id.clone(), &app.name, default);
}

/// Applies a pick in an app's list. Returns whether anything changed.
pub(crate) fn pick(choices: &mut Choices, row: &AppRow, index: usize) -> bool {
    match app_choice_at(index) {
        None => false,
        Some(None) => choices.remove(&row.app),
        Some(Some(choice)) => {
            if row.own == Some(choice) {
                return false;
            }
            choices.set(row.app.clone(), &row.name, choice);
            true
        }
    }
}

/// What GNOME Shell's extension service says about the extension.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ExtensionInfo {
    /// `ExtensionState`: 1 active, 2 inactive, 3 error, 4 out of date,
    /// 6 initialized, 7 disabling, 8 enabling, 99 uninstalled.
    pub(crate) state: u32,
    pub(crate) enabled: bool,
    pub(crate) error: String,
}

/// Where the extension stands, as the page needs to know it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExtensionState {
    /// Not asked yet.
    Checking,
    Active,
    /// Installed and known to the shell, and off. `extensions_off` is set when
    /// every user extension is turned off for the session.
    Off {
        extensions_off: bool,
    },
    /// Its files are there and the shell has not loaded them: it was
    /// installed after the shell started.
    NeedsSignOut,
    NotInstalled,
    Failed(String),
    Unsupported,
    /// No GNOME Shell answered on the session bus.
    NoShell,
}

/// Reads the shell's answer. `info` is `None` when the shell does not know
/// the uuid at all, and `installed` says whether the files are on disk.
pub(crate) fn extension_state(
    info: Option<&ExtensionInfo>,
    user_extensions_enabled: bool,
    installed: bool,
) -> ExtensionState {
    let Some(info) = info else {
        return if installed {
            ExtensionState::NeedsSignOut
        } else {
            ExtensionState::NotInstalled
        };
    };
    match info.state {
        1 if info.enabled && user_extensions_enabled => ExtensionState::Active,
        3 => ExtensionState::Failed(info.error.clone()),
        4 => ExtensionState::Unsupported,
        99 => ExtensionState::NotInstalled,
        _ => ExtensionState::Off {
            extensions_off: !user_extensions_enabled,
        },
    }
}

/// A card the page shows in place of the settings, or above them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Notice {
    pub(crate) title: &'static str,
    pub(crate) body: String,
    /// The one button, when there is something the page can do about it.
    pub(crate) action: Option<NoticeAction>,
    /// Whether the settings are drawn below it.
    pub(crate) settings_shown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NoticeAction {
    TurnOn,
    StartService,
}

/// The extension notice. The settings are shown only while the extension is
/// on, as the ticket asks; otherwise the page offers to turn it on.
pub(crate) fn extension_notice(locale: Locale, state: &ExtensionState) -> Option<Notice> {
    let w = copy(locale);
    let notice = |title, body: &str, action| Notice {
        title,
        body: body.to_string(),
        action,
        settings_shown: false,
    };
    match state {
        ExtensionState::Active => None,
        ExtensionState::Checking => Some(notice(w.checking, "", None)),
        ExtensionState::Off { extensions_off } => {
            let body = if *extensions_off {
                format!("{} {}", w.turn_on_body, w.turn_on_extensions_off)
            } else {
                w.turn_on_body.to_string()
            };
            Some(notice(w.turn_on_title, &body, Some(NoticeAction::TurnOn)))
        }
        ExtensionState::NeedsSignOut => Some(notice(w.sign_out_title, w.sign_out_body, None)),
        ExtensionState::NotInstalled => {
            Some(notice(w.not_installed_title, w.not_installed_body, None))
        }
        ExtensionState::Failed(error) => Some(notice(w.failed_title, error, None)),
        ExtensionState::Unsupported => Some(notice(w.unsupported_title, w.unsupported_body, None)),
        ExtensionState::NoShell => Some(notice(w.no_shell_title, w.no_shell_body, None)),
    }
}

/// What the service said about itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ServiceState {
    Checking,
    Running {
        adapter_present: bool,
        suppressed: bool,
        problem: String,
    },
    Unreachable(String),
}

/// The service notice, drawn above the settings. A service that is not
/// running is a warning and not a stop: the choices are still saved, and the
/// page says what happens meanwhile.
pub(crate) fn service_notice(locale: Locale, state: &ServiceState) -> Option<Notice> {
    let w = copy(locale);
    match state {
        ServiceState::Checking => None,
        ServiceState::Unreachable(_) => Some(Notice {
            title: w.service_off_title,
            body: w.service_off_body.to_string(),
            action: Some(NoticeAction::StartService),
            settings_shown: true,
        }),
        ServiceState::Running {
            adapter_present,
            suppressed,
            problem,
        } => {
            if !problem.is_empty() {
                Some(Notice {
                    title: w.service_problem_title,
                    body: problem.clone(),
                    action: None,
                    settings_shown: true,
                })
            } else if *adapter_present && !*suppressed {
                Some(Notice {
                    title: w.service_off_title,
                    body: w.service_off_body.to_string(),
                    action: Some(NoticeAction::StartService),
                    settings_shown: true,
                })
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;

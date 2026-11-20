//! The Diagnostics table's details in the person's language (ticket 131).
//!
//! `touchpad-core` decides each check and words its detail in English, which
//! is what the journal and an exported report carry. The table on screen
//! shows the detail in the window's language instead, worded here from the
//! same facts the check was decided from. A detail that carries a system's own
//! message (a file that would not parse, a settings service that did not
//! answer) keeps that message as it came, after words that say what it is.

use touchpad_core::{HealthCheck, HealthState, SettingId};

use crate::i18n::Locale;
use crate::model::TouchpadModel;

/// What looking for the gesture path found: the adapter extension and the
/// gesture service, which are both needed for a hand on the touchpad to do
/// anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GestureBridge {
    /// The adapter answered and the service is recognizing gestures.
    Listening { shell_version: String },
    /// Both are there, and safe mode is on: the service runs and recognizes
    /// nothing until safe mode is turned off.
    SafeMode,
    /// Both are there, and no gesture is switched on: the service runs and
    /// waits for one.
    NothingConfigured,
    /// The service runs and recognizes gestures, and cannot watch its
    /// configuration (the person's inotify instances used up, for one): a
    /// change made here reaches it once the watch is back, which it tries
    /// every minute (ticket 131).
    NotWatching { problem: String },
    /// The adapter answered and the service is not on the bus at all.
    ServiceStopped,
    /// The adapter extension is not on the session bus.
    AdapterMissing { all_extensions_off: bool },
    /// The session bus or the adapter failed, with what it said.
    Unreachable { detail: String },
}

impl GestureBridge {
    /// Whether gestures can work: both halves are there.
    pub fn connected(&self) -> bool {
        matches!(
            self,
            Self::Listening { .. } | Self::SafeMode | Self::NothingConfigured
        )
    }

    /// The detail in English, which the report and the journal carry.
    pub fn english(&self) -> String {
        match self {
            Self::Listening { shell_version } => format!(
                "the GNOME Shell adapter answered, on shell {shell_version}, and the gesture \
                 service is listening"
            ),
            Self::SafeMode => "the gesture service is running and recognizes nothing while \
                               safe mode is on"
                .to_string(),
            Self::NothingConfigured => "the gesture service is running and waits for a \
                                        gesture to be switched on"
                .to_string(),
            Self::NotWatching { problem } => format!(
                "the gesture service is recognizing gestures but cannot watch its settings \
                 ({problem}), so a change made here reaches it only once the watch is back; \
                 it tries again every minute"
            ),
            Self::ServiceStopped => "the GNOME Shell adapter answered, but the gesture \
                                     service (better-touchpad-gestures.service) is not \
                                     running, so no gesture is recognized and GNOME keeps \
                                     its own; Check again on Better Touchpad in Better \
                                     Manager starts it"
                .to_string(),
            Self::AdapterMissing {
                all_extensions_off: true,
            } => "GNOME's switch that turns off all extensions is on, so the GNOME Shell \
                  adapter extension cannot run"
                .to_string(),
            Self::AdapterMissing {
                all_extensions_off: false,
            } => "the GNOME Shell adapter extension is not enabled on this session".to_string(),
            Self::Unreachable { detail } => detail.clone(),
        }
    }

    fn chinese(&self) -> String {
        match self {
            Self::Listening { shell_version } => {
                format!("GNOME Shell 轉接器有回應（Shell {shell_version}），手勢服務正在接收手勢")
            }
            Self::SafeMode => "手勢服務正在執行，但安全模式開啟期間不會辨識任何手勢".to_string(),
            Self::NothingConfigured => "手勢服務正在執行，等你開啟任何一個手勢".to_string(),
            Self::NotWatching { problem } => format!(
                "手勢服務正在辨識手勢，但無法追蹤設定檔（{problem}），所以這裡的變更要等它恢復追蹤後才會生效；它每分鐘會再試一次"
            ),
            Self::ServiceStopped => "GNOME Shell 轉接器有回應，但手勢服務（better-touchpad-gestures.service）沒有在執行，所以不會辨識任何手勢，GNOME 會保留自己的手勢。在 Better Manager 對 Better Touchpad 按「重新檢查」就會啟動它".to_string(),
            Self::AdapterMissing {
                all_extensions_off: true,
            } => "GNOME 的「關閉所有擴充功能」開關是開著的，所以 GNOME Shell 轉接器無法執行".to_string(),
            Self::AdapterMissing {
                all_extensions_off: false,
            } => "這個工作階段沒有啟用 GNOME Shell 轉接器".to_string(),
            Self::Unreachable { detail } => format!("連不上手勢連線：{detail}"),
        }
    }

    pub fn words(&self, locale: Locale) -> String {
        match locale.resolved() {
            Locale::ZhTw => self.chinese(),
            _ => self.english(),
        }
    }
}

/// A health check's name, from its stable key: the one place both windows
/// word it. An unknown key is shown as it is, since it names a check a newer
/// Better Touchpad added.
pub fn check_name(id: &str, locale: Locale) -> String {
    let zh = locale.resolved() == Locale::ZhTw;
    let words = match (id, zh) {
        ("touchpad.configuration", true) => "設定檔",
        ("touchpad.configuration", false) => "Settings file",
        ("touchpad.backend", true) => "桌面設定",
        ("touchpad.backend", false) => "Desktop settings",
        ("touchpad.device", true) => "觸控板",
        ("touchpad.device", false) => "Touchpad",
        ("touchpad.capabilities", true) => "可變更的項目",
        ("touchpad.capabilities", false) => "What can be changed",
        ("touchpad.capture", true) => "變更前的記錄",
        ("touchpad.capture", false) => "Values captured before changes",
        ("touchpad.safe_mode", true) => "安全模式",
        ("touchpad.safe_mode", false) => "Safe mode",
        ("touchpad.integration", true) => "Better Touchpad 整合",
        ("touchpad.integration", false) => "Better Touchpad integration",
        ("touchpad.gesture_bridge", true) => "手勢連線",
        ("touchpad.gesture_bridge", false) => "Gesture connection",
        _ => return id.to_string(),
    };
    words.to_string()
}

impl TouchpadModel {
    /// The backend's own line, in the window's language: the same words the
    /// `touchpad.backend` check's detail has.
    pub fn backend_words(&self, locale: Locale) -> String {
        match self
            .health()
            .checks
            .iter()
            .find(|check| check.id == "touchpad.backend")
        {
            Some(check) => self.check_detail(check, locale),
            None => self.backend_status().detail.clone(),
        }
    }

    /// A check's detail in the window's language.
    pub fn check_detail(&self, check: &HealthCheck, locale: Locale) -> String {
        if locale.resolved() != Locale::ZhTw {
            return check.detail.clone();
        }
        let failed = check.state == HealthState::Failed;
        match check.id.as_str() {
            "touchpad.configuration" if failed => format!("設定檔無法讀取：{}", check.detail),
            "touchpad.configuration" => "已讀取設定檔".to_string(),
            "touchpad.backend" if failed => format!("桌面設定無法使用：{}", check.detail),
            "touchpad.backend" => "桌面設定服務有回應".to_string(),
            "touchpad.device" => match (self.device_count(), self.selected_device()) {
                (0, _) => "這台電腦找不到觸控板".to_string(),
                (found, None) => format!("找到 {found} 個觸控板，但沒有選擇任何一個"),
                (found, Some(device)) => {
                    format!("找到 {found} 個觸控板，正在使用 {}", device.identity)
                }
            },
            "touchpad.capabilities" if failed => {
                format!("{} 沒有任何設定可以讀取或套用", self.backend_name())
            }
            "touchpad.capabilities" => format!(
                "{} 個設定中有 {} 個可以使用",
                SettingId::ALL.len(),
                self.available_count()
            ),
            "touchpad.capture" if self.has_backup() => "已記錄第一次變更前的設定".to_string(),
            "touchpad.capture" => "還沒有變更過任何設定，所以沒有需要還原的內容".to_string(),
            "touchpad.gesture_bridge" => self
                .gesture_bridge_fact()
                .map(|bridge| bridge.words(locale))
                .unwrap_or_else(|| check.detail.clone()),
            "touchpad.safe_mode" => {
                "安全模式已開啟：Better Touchpad 只讀取設定，不做任何變更".to_string()
            }
            "touchpad.integration" => "Better Touchpad 整合已關閉".to_string(),
            _ => check.detail.clone(),
        }
    }
}

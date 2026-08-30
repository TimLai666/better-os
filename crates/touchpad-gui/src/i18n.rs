//! Localized wording, following the pattern the manager and chooser GUIs use:
//! one struct of strings per locale, resolved from the environment unless the
//! caller passes a locale in.
//!
//! The Traditional Chinese terms Issue #3 fixes for this half — `游標靈敏度`,
//! `捲動靈敏度`, `自然捲動`, and `點按來按一下` — are used verbatim and are
//! asserted by a test, so a later rewording cannot quietly drop them.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    pub fn resolved(self) -> Self {
        match self {
            Self::System => Self::from_language(&std::env::var("LANG").unwrap_or_default()),
            locale => locale,
        }
    }

    pub fn from_language(language: &str) -> Self {
        let language = language.to_ascii_lowercase();
        if language.contains("zh_tw") || language.contains("zh-tw") || language.contains("hant") {
            Self::ZhTw
        } else {
            Self::EnUs
        }
    }

    /// The other shipped locale, for the runtime language switch.
    pub fn toggled(self) -> Self {
        match self.resolved() {
            Self::ZhTw => Self::EnUs,
            _ => Self::ZhTw,
        }
    }

    pub fn tag(self) -> &'static str {
        match self.resolved() {
            Self::ZhTw => "zh-TW",
            _ => "en-US",
        }
    }
}

pub struct Copy {
    pub brand: &'static str,
    pub application: &'static str,
    pub switch_language: &'static str,

    pub nav_overview: &'static str,
    pub nav_pointer: &'static str,
    pub nav_scrolling: &'static str,
    pub nav_clicking: &'static str,
    pub nav_devices: &'static str,
    pub nav_diagnostics: &'static str,

    pub overview_title: &'static str,
    pub overview_subtitle: &'static str,
    pub selected_touchpad: &'static str,
    pub session: &'static str,
    pub backend: &'static str,
    pub health: &'static str,
    pub pointer_summary: &'static str,
    pub scroll_summary: &'static str,
    pub pending_sign_out: &'static str,
    pub nothing_pending: &'static str,
    pub unsaved_changes: &'static str,

    pub pointer_title: &'static str,
    pub pointer_subtitle: &'static str,
    /// Issue #3 fixes this term.
    pub pointer_sensitivity: &'static str,
    pub acceleration_profile: &'static str,
    pub profile_default: &'static str,
    pub profile_adaptive: &'static str,
    pub profile_flat: &'static str,
    pub disable_while_typing: &'static str,
    pub pointer_test_title: &'static str,
    pub pointer_test_hint: &'static str,
    pub pointer_test_idle: &'static str,

    pub scrolling_title: &'static str,
    pub scrolling_subtitle: &'static str,
    /// Issue #3 fixes this term.
    pub scroll_sensitivity: &'static str,
    pub vertical_axis: &'static str,
    pub horizontal_axis: &'static str,
    pub linked_axes: &'static str,
    /// Issue #3 fixes this term.
    pub natural_scrolling: &'static str,
    pub two_finger_scrolling: &'static str,
    pub smooth_scrolling: &'static str,
    pub scroll_test_title: &'static str,
    pub scroll_test_hint: &'static str,

    pub clicking_title: &'static str,
    pub clicking_subtitle: &'static str,
    /// Issue #3 fixes this term.
    pub tap_to_click: &'static str,
    pub tap_and_drag: &'static str,
    pub drag_lock: &'static str,
    pub click_method: &'static str,
    pub method_default: &'static str,
    pub method_areas: &'static str,
    pub method_fingers: &'static str,
    pub method_none: &'static str,
    pub middle_click_emulation: &'static str,

    pub devices_title: &'static str,
    pub devices_subtitle: &'static str,
    pub device_identity: &'static str,
    pub device_capabilities: &'static str,
    pub device_scope: &'static str,
    pub scope_global: &'static str,
    pub scope_per_device: &'static str,
    pub state_connected: &'static str,
    pub state_disconnected: &'static str,
    pub state_inhibited: &'static str,
    pub contacts: &'static str,
    pub buttonpad: &'static str,
    pub separate_buttons: &'static str,
    pub no_devices: &'static str,

    pub diagnostics_title: &'static str,
    pub diagnostics_subtitle: &'static str,
    pub effective_values: &'static str,
    pub requested_value: &'static str,
    pub effective_value: &'static str,
    pub previous_value: &'static str,
    pub setting_key: &'static str,
    pub restore_actions: &'static str,
    pub captured_before: &'static str,
    pub nothing_captured: &'static str,

    pub apply: &'static str,
    pub discard: &'static str,
    pub refresh: &'static str,
    pub restore_all: &'static str,
    pub restore_section: &'static str,
    pub safe_mode_on: &'static str,
    pub safe_mode_off: &'static str,
    pub safe_mode_banner: &'static str,

    pub result_applied: &'static str,
    pub result_awaiting_sign_out: &'static str,
    pub result_partial: &'static str,
    pub result_failed: &'static str,
    pub result_nothing: &'static str,
    pub restored: &'static str,

    pub unavailable: &'static str,
    pub value_session_default: &'static str,
    pub value_unknown: &'static str,
    pub value_not_read: &'static str,
    pub value_permission_denied: &'static str,
    pub pending_badge: &'static str,
    pub drifted_badge: &'static str,
    pub sign_out_badge: &'static str,
    pub busy: &'static str,
}

pub const EN_US: Copy = Copy {
    brand: "Better OS",
    application: "Touchpad",
    switch_language: "中文",

    nav_overview: "Overview",
    nav_pointer: "Pointer",
    nav_scrolling: "Scrolling",
    nav_clicking: "Clicking",
    nav_devices: "Devices",
    nav_diagnostics: "Diagnostics",

    overview_title: "Overview",
    overview_subtitle: "What this touchpad does now, and what Better OS can change.",
    selected_touchpad: "Selected touchpad",
    session: "Session",
    backend: "Active backend",
    health: "Health",
    pointer_summary: "Pointer sensitivity",
    scroll_summary: "Scrolling sensitivity",
    pending_sign_out: "Waiting for sign-out",
    nothing_pending: "Nothing is waiting to be applied.",
    unsaved_changes: "Changes are staged but not applied yet.",

    pointer_title: "Pointer",
    pointer_subtitle: "How far the pointer moves for the same finger movement.",
    pointer_sensitivity: "Pointer sensitivity",
    acceleration_profile: "Acceleration profile",
    profile_default: "Session default",
    profile_adaptive: "Adaptive",
    profile_flat: "Flat",
    disable_while_typing: "Disable while typing",
    pointer_test_title: "Test surface",
    pointer_test_hint: "Move the pointer over this area to feel the current setting.",
    pointer_test_idle: "Move the pointer here",

    scrolling_title: "Scrolling",
    scrolling_subtitle: "How far the content moves for the same two-finger movement.",
    scroll_sensitivity: "Scrolling sensitivity",
    vertical_axis: "Vertical",
    horizontal_axis: "Horizontal",
    linked_axes: "Move both axes together",
    natural_scrolling: "Natural scrolling",
    two_finger_scrolling: "Two-finger scrolling",
    smooth_scrolling: "Smooth scrolling",
    scroll_test_title: "Test area",
    scroll_test_hint: "Scroll inside this box, sideways as well as up and down.",

    clicking_title: "Clicking",
    clicking_subtitle: "How a tap and a press are turned into buttons.",
    tap_to_click: "Tap to click",
    tap_and_drag: "Tap and drag",
    drag_lock: "Drag lock",
    click_method: "Click method",
    method_default: "Session default",
    method_areas: "By area of the pad",
    method_fingers: "By number of fingers",
    method_none: "No physical click",
    middle_click_emulation: "Middle-click emulation",

    devices_title: "Devices",
    devices_subtitle: "The touchpads this system reports, and what each one can do.",
    device_identity: "Identity",
    device_capabilities: "Capabilities",
    device_scope: "Settings apply to",
    scope_global: "Every touchpad (this session has one profile)",
    scope_per_device: "This device only",
    state_connected: "Connected",
    state_disconnected: "Not connected",
    state_inhibited: "Connected but inhibited",
    contacts: "Contacts reported",
    buttonpad: "Whole surface clicks",
    separate_buttons: "Separate hardware buttons",
    no_devices: "No touchpad was found on this system.",

    diagnostics_title: "Diagnostics",
    diagnostics_subtitle: "What was read, what was written, and how to undo it.",
    effective_values: "Effective values",
    requested_value: "Requested",
    effective_value: "Effective",
    previous_value: "Captured before the first change",
    setting_key: "Setting",
    restore_actions: "Restore",
    captured_before: "These are the values Better OS saw before it changed anything.",
    nothing_captured: "Nothing has been changed, so there is nothing to restore.",

    apply: "Apply",
    discard: "Discard changes",
    refresh: "Read again",
    restore_all: "Restore everything",
    restore_section: "Restore this section",
    safe_mode_on: "Turn on safe mode",
    safe_mode_off: "Turn off safe mode",
    safe_mode_banner: "Safe mode is on. Settings are shown but nothing is changed.",

    result_applied: "Applied.",
    result_awaiting_sign_out: "Applied, and takes effect after signing out.",
    result_partial: "Partly applied. Some settings did something else.",
    result_failed: "Failed. Nothing was left half-changed.",
    result_nothing: "Nothing needed changing.",
    restored: "Restored to the captured values.",

    unavailable: "Not available here",
    value_session_default: "Session default",
    value_unknown: "Cannot be read",
    value_not_read: "Not read yet",
    value_permission_denied: "Not allowed to read",
    pending_badge: "Staged",
    drifted_badge: "Changed elsewhere",
    sign_out_badge: "Needs sign-out",
    busy: "Working…",
};

pub const ZH_TW: Copy = Copy {
    brand: "Better OS",
    application: "觸控板",
    switch_language: "English",

    nav_overview: "總覽",
    nav_pointer: "游標",
    nav_scrolling: "捲動",
    nav_clicking: "點按",
    nav_devices: "裝置",
    nav_diagnostics: "診斷",

    overview_title: "總覽",
    overview_subtitle: "這個觸控板現在的行為，以及 Better OS 能改哪些。",
    selected_touchpad: "選定的觸控板",
    session: "工作階段",
    backend: "目前使用的後端",
    health: "健康狀態",
    pointer_summary: "游標靈敏度",
    scroll_summary: "捲動靈敏度",
    pending_sign_out: "等待登出後生效",
    nothing_pending: "目前沒有待套用的變更。",
    unsaved_changes: "已調整但還沒套用。",

    pointer_title: "游標",
    pointer_subtitle: "同樣的手指移動，游標會走多遠。",
    pointer_sensitivity: "游標靈敏度",
    acceleration_profile: "加速曲線",
    profile_default: "沿用系統設定",
    profile_adaptive: "隨速度調整",
    profile_flat: "固定不加速",
    disable_while_typing: "打字時停用觸控板",
    pointer_test_title: "測試區",
    pointer_test_hint: "把游標移到這塊區域，直接感受目前的設定。",
    pointer_test_idle: "把游標移到這裡",

    scrolling_title: "捲動",
    scrolling_subtitle: "同樣的雙指移動，內容會捲多遠。",
    scroll_sensitivity: "捲動靈敏度",
    vertical_axis: "垂直",
    horizontal_axis: "水平",
    linked_axes: "兩個方向一起調整",
    natural_scrolling: "自然捲動",
    two_finger_scrolling: "雙指捲動",
    smooth_scrolling: "平滑捲動",
    scroll_test_title: "測試區",
    scroll_test_hint: "在這個框裡捲動看看，上下和左右都可以。",

    clicking_title: "點按",
    clicking_subtitle: "輕點和按壓要怎麼變成滑鼠按鍵。",
    tap_to_click: "點按來按一下",
    tap_and_drag: "點按後拖曳",
    drag_lock: "拖曳鎖定",
    click_method: "按鍵判定方式",
    method_default: "沿用系統設定",
    method_areas: "依觸控板上的區域",
    method_fingers: "依按下的手指數",
    method_none: "沒有實體按鍵",
    middle_click_emulation: "模擬中鍵",

    devices_title: "裝置",
    devices_subtitle: "系統回報的觸控板，以及每一個能做到什麼。",
    device_identity: "識別碼",
    device_capabilities: "硬體能力",
    device_scope: "設定套用範圍",
    scope_global: "所有觸控板（這個工作階段只有一組設定）",
    scope_per_device: "只套用到這個裝置",
    state_connected: "已連接",
    state_disconnected: "未連接",
    state_inhibited: "已連接但被停用",
    contacts: "可辨識的接觸點",
    buttonpad: "整片都是按鍵",
    separate_buttons: "有獨立實體按鍵",
    no_devices: "這台電腦沒有找到觸控板。",

    diagnostics_title: "診斷",
    diagnostics_subtitle: "讀到什麼、寫了什麼，以及怎麼還原。",
    effective_values: "實際生效的值",
    requested_value: "要求的值",
    effective_value: "實際的值",
    previous_value: "第一次變更前記錄的值",
    setting_key: "設定項目",
    restore_actions: "還原",
    captured_before: "這些是 Better OS 動手之前看到的值。",
    nothing_captured: "還沒改過任何設定，所以沒有東西需要還原。",

    apply: "套用",
    discard: "捨棄變更",
    refresh: "重新讀取",
    restore_all: "全部還原",
    restore_section: "還原這一區",
    safe_mode_on: "開啟安全模式",
    safe_mode_off: "關閉安全模式",
    safe_mode_banner: "安全模式已開啟。只會顯示設定，不會做任何變更。",

    result_applied: "已套用。",
    result_awaiting_sign_out: "已寫入，登出後才會生效。",
    result_partial: "只套用了一部分，有些設定變成別的值。",
    result_failed: "沒有成功，也沒有留下改到一半的狀態。",
    result_nothing: "沒有需要變更的項目。",
    restored: "已還原成記錄下來的值。",

    unavailable: "這裡無法使用",
    value_session_default: "系統預設值",
    value_unknown: "讀不到",
    value_not_read: "尚未讀取",
    value_permission_denied: "沒有讀取權限",
    pending_badge: "待套用",
    drifted_badge: "被別的地方改過",
    sign_out_badge: "需要登出",
    busy: "處理中…",
};

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixed_traditional_chinese_terms_are_used_verbatim() {
        // Issue #3 fixes these four for the touchpad half. A rewording that
        // dropped one would be a product decision, not a copy edit.
        assert_eq!(ZH_TW.pointer_sensitivity, "游標靈敏度");
        assert_eq!(ZH_TW.scroll_sensitivity, "捲動靈敏度");
        assert_eq!(ZH_TW.natural_scrolling, "自然捲動");
        assert_eq!(ZH_TW.tap_to_click, "點按來按一下");
        assert_eq!(ZH_TW.pointer_summary, "游標靈敏度");
        assert_eq!(ZH_TW.scroll_summary, "捲動靈敏度");
    }

    #[test]
    fn both_locales_are_complete_and_actually_different() {
        let english = fields(&EN_US);
        let chinese = fields(&ZH_TW);
        assert_eq!(english.len(), chinese.len());
        for (index, (left, right)) in english.iter().zip(chinese.iter()).enumerate() {
            assert!(!left.trim().is_empty(), "English field {index} is empty");
            assert!(!right.trim().is_empty(), "Chinese field {index} is empty");
        }
        // A handful of proper nouns are the same in both; everything else has
        // actually been translated rather than copied.
        let shared = english
            .iter()
            .zip(chinese.iter())
            .filter(|(left, right)| left == right)
            .count();
        assert!(shared <= 2, "{shared} strings were never translated");
    }

    #[test]
    fn a_traditional_chinese_environment_resolves_to_the_chinese_copy() {
        assert_eq!(Locale::from_language("zh_TW.UTF-8"), Locale::ZhTw);
        assert_eq!(Locale::from_language("zh-TW"), Locale::ZhTw);
        assert_eq!(Locale::from_language("zh_Hant"), Locale::ZhTw);
        assert_eq!(Locale::from_language("en_US.UTF-8"), Locale::EnUs);
        assert_eq!(Locale::from_language(""), Locale::EnUs);
    }

    #[test]
    fn the_language_switch_goes_both_ways() {
        assert_eq!(Locale::EnUs.toggled(), Locale::ZhTw);
        assert_eq!(Locale::ZhTw.toggled(), Locale::EnUs);
        assert_eq!(Locale::EnUs.tag(), "en-US");
        assert_eq!(Locale::ZhTw.tag(), "zh-TW");
    }

    /// Every string in a `Copy`, so completeness is asserted rather than
    /// reviewed. Adding a field without translating it fails the test above.
    pub(crate) fn fields(copy: &'static Copy) -> Vec<&'static str> {
        vec![
            copy.application,
            copy.switch_language,
            copy.nav_overview,
            copy.nav_pointer,
            copy.nav_scrolling,
            copy.nav_clicking,
            copy.nav_devices,
            copy.nav_diagnostics,
            copy.overview_title,
            copy.overview_subtitle,
            copy.selected_touchpad,
            copy.session,
            copy.backend,
            copy.health,
            copy.pointer_summary,
            copy.scroll_summary,
            copy.pending_sign_out,
            copy.nothing_pending,
            copy.unsaved_changes,
            copy.pointer_title,
            copy.pointer_subtitle,
            copy.pointer_sensitivity,
            copy.acceleration_profile,
            copy.profile_default,
            copy.profile_adaptive,
            copy.profile_flat,
            copy.disable_while_typing,
            copy.pointer_test_title,
            copy.pointer_test_hint,
            copy.pointer_test_idle,
            copy.scrolling_title,
            copy.scrolling_subtitle,
            copy.scroll_sensitivity,
            copy.vertical_axis,
            copy.horizontal_axis,
            copy.linked_axes,
            copy.natural_scrolling,
            copy.two_finger_scrolling,
            copy.smooth_scrolling,
            copy.scroll_test_title,
            copy.scroll_test_hint,
            copy.clicking_title,
            copy.clicking_subtitle,
            copy.tap_to_click,
            copy.tap_and_drag,
            copy.drag_lock,
            copy.click_method,
            copy.method_default,
            copy.method_areas,
            copy.method_fingers,
            copy.method_none,
            copy.middle_click_emulation,
            copy.devices_title,
            copy.devices_subtitle,
            copy.device_identity,
            copy.device_capabilities,
            copy.device_scope,
            copy.scope_global,
            copy.scope_per_device,
            copy.state_connected,
            copy.state_disconnected,
            copy.state_inhibited,
            copy.contacts,
            copy.buttonpad,
            copy.separate_buttons,
            copy.no_devices,
            copy.diagnostics_title,
            copy.diagnostics_subtitle,
            copy.effective_values,
            copy.requested_value,
            copy.effective_value,
            copy.previous_value,
            copy.setting_key,
            copy.restore_actions,
            copy.captured_before,
            copy.nothing_captured,
            copy.apply,
            copy.discard,
            copy.refresh,
            copy.restore_all,
            copy.restore_section,
            copy.safe_mode_on,
            copy.safe_mode_off,
            copy.safe_mode_banner,
            copy.result_applied,
            copy.result_awaiting_sign_out,
            copy.result_partial,
            copy.result_failed,
            copy.result_nothing,
            copy.restored,
            copy.unavailable,
            copy.value_session_default,
            copy.value_unknown,
            copy.value_not_read,
            copy.value_permission_denied,
            copy.pending_badge,
            copy.drifted_badge,
            copy.sign_out_badge,
            copy.busy,
        ]
    }
}

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

    pub appearance: &'static str,
    pub appearance_description: &'static str,
    pub appearance_system: &'static str,
    pub appearance_light: &'static str,
    pub appearance_dark: &'static str,

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
    pub value_on: &'static str,
    pub builtin_overview: &'static str,
    pub builtin_workspace: &'static str,
    pub value_off: &'static str,
    pub value_not_read: &'static str,
    pub value_permission_denied: &'static str,
    pub pending_badge: &'static str,
    pub drifted_badge: &'static str,
    pub sign_out_badge: &'static str,
    pub busy: &'static str,

    // Ticket 30: the Gestures screen.
    pub nav_gestures: &'static str,
    pub gestures_title: &'static str,
    pub gestures_subtitle: &'static str,
    pub preset_title: &'static str,
    pub preset_description: &'static str,
    pub preset_not_applied: &'static str,
    pub preset_applied: &'static str,
    pub preset_differs: &'static str,
    pub preview_changes: &'static str,
    pub confirm_changes: &'static str,
    pub apply_preset: &'static str,
    pub cancel_preview: &'static str,
    pub changes_heading: &'static str,
    pub conflicts_heading: &'static str,
    pub conflict_badge: &'static str,
    pub unsupported_badge: &'static str,
    pub unresolved_note: &'static str,
    pub resolution_keep_built_in: &'static str,
    pub resolution_disable_built_in: &'static str,
    pub resolution_remap: &'static str,
    pub test_gestures: &'static str,
    pub test_gestures_hint: &'static str,
    pub live_testing: &'static str,
    pub live_testing_off: &'static str,
    pub test_run: &'static str,
    pub test_no_events: &'static str,
    pub recognized_events: &'static str,
    pub edit_gesture: &'static str,
    pub save_gesture: &'static str,
    pub contacts_label: &'static str,
    pub thumb_label: &'static str,
    pub shape_label: &'static str,
    pub action_heading: &'static str,
    pub activation_label: &'static str,
    pub cancellation_label: &'static str,
    pub cooldown_label: &'static str,
    pub enabled_label: &'static str,
    pub direction_heading: &'static str,
    pub profile_heading: &'static str,
    pub profile_global: &'static str,
    pub profile_this_device: &'static str,
    pub profile_follows_global: &'static str,
    pub profile_own: &'static str,
    pub profile_forget: &'static str,
    pub profile_detach: &'static str,
    pub shortcut_heading: &'static str,
    pub shortcut_modifiers: &'static str,
    pub shortcut_key: &'static str,
    pub shortcut_needs_modifier: &'static str,
    pub shortcut_conflict: &'static str,
    pub shortcut_none_recorded: &'static str,
    pub shortcut_unknown: &'static str,
    pub group_letters: &'static str,
    pub group_digits: &'static str,
    pub group_function: &'static str,
    pub group_navigation: &'static str,
    pub group_editing: &'static str,
    pub group_punctuation: &'static str,
    pub profiles_file: &'static str,
    pub export_label: &'static str,
    pub export_profiles: &'static str,
    pub import_label: &'static str,
    pub import_profiles: &'static str,
    pub import_summary: &'static str,
    pub import_uses_global: &'static str,
    pub restore_gestures: &'static str,
    pub disable_gestures: &'static str,
    pub no_gestures: &'static str,
    pub gesture_backend_none: &'static str,
    /// The adapter is there and the gesture service is not (ticket 131).
    pub gesture_service_stopped: &'static str,
    /// A gesture switched on that collides with one of GNOME's three-finger
    /// swipes: GNOME 46 does not let it go reliably (ticket 131, ADR 0035).
    pub gesture_collision_unreliable: &'static str,
    /// What the compositor cannot see. Shown wherever a thumb-and-three gesture
    /// is, because the gesture works and the reason it works is not what the
    /// row appears to say.
    pub gesture_thumb_best_effort: &'static str,
    pub gesture_launcher: &'static str,
    pub gesture_show_desktop: &'static str,
    pub gesture_overview: &'static str,
    pub gesture_current_app_windows: &'static str,
    pub gesture_workspace_next: &'static str,
    pub gesture_workspace_previous: &'static str,
    pub gesture_app_back: &'static str,
    pub gesture_app_forward: &'static str,
    pub gesture_app_zoom: &'static str,
    pub gesture_app_rotate: &'static str,
    pub action_launcher_open: &'static str,
    pub action_launcher_close: &'static str,
    pub action_show_desktop: &'static str,
    pub action_overview: &'static str,
    pub action_current_app_windows: &'static str,
    pub action_workspace_next: &'static str,
    pub action_workspace_previous: &'static str,
    pub action_app_next: &'static str,
    pub action_app_previous: &'static str,
    pub action_app_back: &'static str,
    pub action_app_forward: &'static str,
    pub action_app_zoom: &'static str,
    pub action_app_rotate: &'static str,
    pub action_media_play_pause: &'static str,
    pub action_volume_up: &'static str,
    pub action_volume_down: &'static str,
    pub action_volume_mute: &'static str,
    pub action_shortcut: &'static str,
    pub action_none: &'static str,
    pub shape_swipe: &'static str,
    pub shape_pinch: &'static str,
    pub shape_spread: &'static str,
    pub shape_hold: &'static str,
    pub shape_tap: &'static str,
    pub shape_rotate: &'static str,
    pub direction_up: &'static str,
    pub direction_down: &'static str,
    pub direction_left: &'static str,
    pub direction_right: &'static str,
    pub direction_clockwise: &'static str,
    pub direction_counter_clockwise: &'static str,
    pub verification_verified: &'static str,
    pub verification_failed: &'static str,
    pub verification_not_run: &'static str,
    pub verification_unsupported: &'static str,
    pub changes_none: &'static str,
}

pub const EN_US: Copy = Copy {
    brand: "Better OS",
    application: "Touchpad",
    switch_language: "繁體中文",

    appearance: "Appearance",
    appearance_description: "Follows your desktop's light or dark mode unless you pick one.",
    appearance_system: "Use system setting",
    appearance_light: "Light",
    appearance_dark: "Dark",

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
    scrolling_subtitle: "How far the content moves for the same two-finger movement. The speed applies to every touchpad on this computer.",
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
    no_devices: "No touchpad was found on this PC.",

    diagnostics_title: "Diagnostics",
    diagnostics_subtitle: "What was read, what was written, and how to undo it.",
    effective_values: "Effective values",
    requested_value: "You chose",
    effective_value: "In effect",
    previous_value: "Captured before the first change",
    setting_key: "Setting",
    restore_actions: "Restore",
    captured_before: "These are the values Better OS saw before it changed anything.",
    nothing_captured: "Nothing has been changed, so there is nothing to restore.",

    apply: "Apply",
    discard: "Discard",
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
    value_on: "On",
    builtin_overview: "GNOME's overview and app grid",
    builtin_workspace: "GNOME's workspace switching",
    value_off: "Off",
    value_not_read: "Not read yet",
    value_permission_denied: "Not allowed to read",
    pending_badge: "Not applied yet",
    drifted_badge: "Changed elsewhere",
    sign_out_badge: "Needs sign-out",
    busy: "Working…",

    nav_gestures: "Gestures",
    gestures_title: "Gestures",
    gestures_subtitle: "Multi-finger gestures, what they do, and what the desktop already claims.",
    preset_title: "Mac-style gestures",
    preset_description: "The recommended set: a thumb and three fingers for the launcher and the desktop, four fingers for workspaces.",
    preset_not_applied: "Not applied yet",
    preset_applied: "Applied",
    preset_differs: "Applied, then changed here",
    preview_changes: "Preview changes",
    confirm_changes: "I have read the changes above",
    apply_preset: "Apply the preset",
    cancel_preview: "Cancel",
    changes_heading: "What would change",
    conflicts_heading: "What the desktop already does with these fingers",
    conflict_badge: "Conflicts with the desktop",
    unsupported_badge: "No backend can do this yet",
    unresolved_note: "Every conflict needs a decision before anything is applied.",
    resolution_keep_built_in: "Keep the desktop gesture",
    resolution_disable_built_in: "Use the Better OS gesture",
    resolution_remap: "Move mine to five fingers",
    test_gestures: "Test gestures",
    test_gestures_hint: "Replays the gesture and shows what the recognizer saw. Nothing on the system moves.",
    live_testing: "Perform the action for real while testing",
    live_testing_off: "Live testing is off, so testing performs no action.",
    test_run: "Test",
    test_no_events: "Nothing recognized yet.",
    recognized_events: "Recognized events",
    edit_gesture: "Edit",
    save_gesture: "Save",
    contacts_label: "Contact points",
    thumb_label: "Thumb required",
    shape_label: "Gesture",
    action_heading: "Action",
    activation_label: "Activation threshold",
    cancellation_label: "Cancellation threshold",
    cooldown_label: "Cooldown",
    enabled_label: "Enabled",
    direction_heading: "Direction",
    profile_heading: "Gesture profile",
    profile_global: "All touchpads",
    profile_this_device: "This touchpad only",
    profile_follows_global: "This touchpad follows the profile shared by all touchpads.",
    profile_own: "This touchpad has a gesture profile of its own.",
    profile_forget: "Share the profile again",
    profile_detach: "Give this pad its own gestures",
    shortcut_heading: "Custom keyboard shortcut",
    shortcut_modifiers: "Modifier keys",
    shortcut_key: "Key",
    shortcut_needs_modifier: "A shortcut needs at least one modifier key.",
    shortcut_conflict: "A keyboard shortcut you have already set uses this combination:",
    shortcut_none_recorded: "No keyboard shortcut you have changed uses this combination. GNOME's own defaults cannot be read, so this is not a promise that nothing uses it.",
    shortcut_unknown: "The recorded keyboard shortcuts could not be read, so this combination was not checked.",
    group_letters: "Letters",
    group_digits: "Digits",
    group_function: "Function keys",
    group_navigation: "Navigation",
    group_editing: "Editing",
    group_punctuation: "Punctuation",
    profiles_file: "Move this setup between machines",
    export_label: "Export to",
    export_profiles: "Export",
    import_label: "Import from",
    import_profiles: "Preview the import",
    import_summary: "Profiles in this file",
    import_uses_global: "This file has no profile for the selected touchpad, so its shared profile is what would be applied.",
    restore_gestures: "Restore the captured gestures",
    disable_gestures: "Turn gestures off",
    no_gestures: "No gesture is configured yet.",
    gesture_backend_none: "The GNOME Shell adapter is not enabled on this session, so no gesture reaches the desktop yet.",
    gesture_service_stopped: "The gesture service isn't running, so no gesture is recognized and GNOME keeps its own gestures. Choose Check again on Better Touchpad in Better Manager's Apps to start it.",
    gesture_collision_unreliable: "A gesture you switched on uses three fingers in a direction GNOME uses too. GNOME turns its own back on after every animated workspace switch and keeps its overview gestures, so both can happen at once. Four or five fingers avoid it.",
    gesture_thumb_best_effort: "The desktop reports how many fingers a gesture has and not which one is the thumb, so a thumb and three fingers is matched as four contacts.",
    gesture_launcher: "Open the launcher",
    gesture_show_desktop: "Show the desktop",
    gesture_overview: "Workspace overview",
    gesture_current_app_windows: "Windows of this app",
    gesture_workspace_next: "Next workspace",
    gesture_workspace_previous: "Previous workspace",
    gesture_app_back: "Back in the app",
    gesture_app_forward: "Forward in the app",
    gesture_app_zoom: "Zoom in the app",
    gesture_app_rotate: "Rotate in the app",
    action_launcher_open: "Open Better Launcher",
    action_launcher_close: "Close Better Launcher",
    action_show_desktop: "Show Desktop",
    action_overview: "Workspace overview",
    action_current_app_windows: "Show all windows of this app",
    action_workspace_next: "Switch to the next workspace",
    action_workspace_previous: "Switch to the previous workspace",
    action_app_next: "Next application",
    action_app_previous: "Previous application",
    action_app_back: "Back",
    action_app_forward: "Forward",
    action_app_zoom: "Zoom",
    action_app_rotate: "Rotate",
    action_media_play_pause: "Play or pause",
    action_volume_up: "Volume up",
    action_volume_down: "Volume down",
    action_volume_mute: "Mute",
    action_shortcut: "Keyboard shortcut",
    action_none: "Do nothing",
    shape_swipe: "Swipe",
    shape_pinch: "Pinch",
    shape_spread: "Spread",
    shape_hold: "Hold",
    shape_tap: "Tap",
    shape_rotate: "Rotate",
    direction_up: "Up",
    direction_down: "Down",
    direction_left: "Left",
    direction_right: "Right",
    direction_clockwise: "Clockwise",
    direction_counter_clockwise: "Counter-clockwise",
    verification_verified: "Verified",
    verification_failed: "Verification failed",
    verification_not_run: "Not applied yet",
    verification_unsupported: "Not available here",
    changes_none: "Nothing would change.",
};

pub const ZH_TW: Copy = Copy {
    brand: "Better OS",
    application: "觸控板",
    switch_language: "English",

    appearance: "外觀",
    appearance_description: "預設跟著桌面切換淺色或深色，也可以固定用其中一種。",
    appearance_system: "跟隨系統",
    appearance_light: "淺色",
    appearance_dark: "深色",

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
    scrolling_subtitle: "同樣的雙指移動，內容會捲多遠。速度會套用到這台電腦的每個觸控板。",
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
    tap_to_click: "輕點即按一下",
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
    requested_value: "你選的",
    effective_value: "目前生效",
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
    value_on: "開啟",
    builtin_overview: "GNOME 的工作區總覽和應用程式格",
    builtin_workspace: "GNOME 的切換工作區",
    value_off: "關閉",
    value_not_read: "尚未讀取",
    value_permission_denied: "沒有讀取權限",
    pending_badge: "尚未套用",
    drifted_badge: "被別的地方改過",
    sign_out_badge: "需要登出",
    busy: "處理中…",

    nav_gestures: "手勢",
    gestures_title: "手勢",
    gestures_subtitle: "多指手勢的用途，以及系統原本就佔用了哪些。",
    preset_title: "Mac 風格手勢",
    preset_description: "建議的組合：拇指加三指開啟啟動器和顯示桌面，四指切換工作區。",
    preset_not_applied: "尚未套用",
    preset_applied: "已套用",
    preset_differs: "套用後又改過",
    preview_changes: "預覽變更",
    confirm_changes: "我已看過上面的變更",
    apply_preset: "套用這組手勢",
    cancel_preview: "取消",
    changes_heading: "會變更的項目",
    conflicts_heading: "系統已經佔用的手勢",
    conflict_badge: "與系統手勢衝突",
    unsupported_badge: "目前沒有後端做得到",
    unresolved_note: "每一個衝突都要先決定，才能套用。",
    resolution_keep_built_in: "保留系統手勢",
    resolution_disable_built_in: "改用 Better OS 手勢",
    resolution_remap: "把我的改成五指",
    test_gestures: "測試手勢",
    test_gestures_hint: "重播這個手勢並顯示辨識過程，不會真的改變系統。",
    live_testing: "測試時真的執行動作",
    live_testing_off: "即時測試已關閉，測試不會執行任何動作。",
    test_run: "測試",
    test_no_events: "還沒有辨識結果。",
    recognized_events: "辨識到的事件",
    edit_gesture: "編輯",
    save_gesture: "儲存",
    contacts_label: "接觸點數量",
    thumb_label: "需要拇指",
    shape_label: "手勢動作",
    action_heading: "動作",
    activation_label: "觸發門檻",
    cancellation_label: "取消門檻",
    cooldown_label: "冷卻時間",
    enabled_label: "啟用",
    direction_heading: "方向",
    profile_heading: "手勢設定檔",
    profile_global: "所有觸控板",
    profile_this_device: "只有這個觸控板",
    profile_follows_global: "這個觸控板目前使用所有觸控板共用的設定檔。",
    profile_own: "這個觸控板有自己的手勢設定檔。",
    profile_forget: "改回使用共用設定檔",
    profile_detach: "讓這個觸控板有自己的手勢",
    shortcut_heading: "自訂鍵盤快速鍵",
    shortcut_modifiers: "輔助按鍵",
    shortcut_key: "按鍵",
    shortcut_needs_modifier: "快速鍵至少要有一個輔助按鍵。",
    shortcut_conflict: "你設定過的鍵盤快速鍵已經在用這個組合：",
    shortcut_none_recorded: "你改過的鍵盤快速鍵都沒有用這個組合。GNOME 內建的預設值讀不到，所以這不代表沒有其他功能在用。",
    shortcut_unknown: "讀不到目前記錄的鍵盤快速鍵，所以沒有檢查這個組合。",
    group_letters: "字母",
    group_digits: "數字",
    group_function: "功能鍵",
    group_navigation: "移動",
    group_editing: "編輯",
    group_punctuation: "符號",
    profiles_file: "把這套設定搬到另一台機器",
    export_label: "匯出到",
    export_profiles: "匯出",
    import_label: "匯入自",
    import_profiles: "預覽匯入內容",
    import_summary: "這個檔案裡的設定檔",
    import_uses_global: "這個檔案沒有目前觸控板的設定檔，所以會套用它的共用設定檔。",
    restore_gestures: "還原記錄下來的手勢",
    disable_gestures: "關閉手勢",
    no_gestures: "還沒有設定任何手勢。",
    gesture_backend_none: "這個工作階段沒有啟用 GNOME Shell 轉接器，所以手勢還無法真的影響桌面。",
    gesture_service_stopped: "手勢服務沒有在執行，所以不會辨識任何手勢，GNOME 會保留自己的手勢。在 Better Manager 的「應用程式」對 Better Touchpad 按「重新檢查」就會啟動它。",
    gesture_collision_unreliable: "你開啟的手勢用了三指，方向和 GNOME 自己的手勢一樣。GNOME 每次以動畫切換工作區後會把自己的手勢再打開，概覽裡的手勢也不會讓出來，所以兩邊可能同時動作。改用四指或五指就不會撞到。",
    gesture_thumb_best_effort: "桌面只會回報手勢用了幾根手指，不會說哪一根是拇指，所以拇指加三指會以四點接觸來判斷。",
    gesture_launcher: "開啟啟動器",
    gesture_show_desktop: "顯示桌面",
    gesture_overview: "工作區總覽",
    gesture_current_app_windows: "這個 App 的視窗",
    gesture_workspace_next: "下一個工作區",
    gesture_workspace_previous: "上一個工作區",
    gesture_app_back: "在 App 中返回",
    gesture_app_forward: "在 App 中前進",
    gesture_app_zoom: "在 App 中縮放",
    gesture_app_rotate: "在 App 中旋轉",
    action_launcher_open: "開啟 Better Launcher",
    action_launcher_close: "關閉 Better Launcher",
    action_show_desktop: "顯示桌面",
    action_overview: "工作區總覽",
    action_current_app_windows: "顯示所有 App",
    action_workspace_next: "切換到下一個工作區",
    action_workspace_previous: "切換到上一個工作區",
    action_app_next: "下一個應用程式",
    action_app_previous: "上一個應用程式",
    action_app_back: "返回",
    action_app_forward: "前進",
    action_app_zoom: "縮放",
    action_app_rotate: "旋轉",
    action_media_play_pause: "播放或暫停",
    action_volume_up: "調高音量",
    action_volume_down: "調低音量",
    action_volume_mute: "靜音",
    action_shortcut: "鍵盤快速鍵",
    action_none: "不做任何事",
    shape_swipe: "滑動",
    shape_pinch: "捏合",
    shape_spread: "張開",
    shape_hold: "按住",
    shape_tap: "輕點",
    shape_rotate: "旋轉",
    direction_up: "向上",
    direction_down: "向下",
    direction_left: "向左",
    direction_right: "向右",
    direction_clockwise: "順時針",
    direction_counter_clockwise: "逆時針",
    verification_verified: "已驗證",
    verification_failed: "驗證失敗",
    verification_not_run: "尚未套用",
    verification_unsupported: "這裡無法使用",
    changes_none: "沒有任何項目需要變更。",
};

/// A health state in words (audit T4: the overview printed the enum name,
/// in English, inside the Chinese window).
pub fn health_words(state: touchpad_core::HealthState, locale: Locale) -> &'static str {
    use touchpad_core::HealthState::*;
    match (state, locale.resolved()) {
        (Ok, Locale::ZhTw) => "運作正常",
        (Degraded, Locale::ZhTw) => "部分功能無法使用",
        (Failed, Locale::ZhTw) => "無法運作",
        (Ok, _) => "Working",
        (Degraded, _) => "Partly unavailable",
        (Failed, _) => "Not working",
    }
}

/// Why a control is unavailable, in the person's words, from the stable
/// reason key the backend or the hardware gave. The backend's own sentence
/// is English and technical; it stays in the details, not on the card. A key
/// this table does not know returns `None` and the caller falls back to the
/// generic "not available here", never to the English sentence.
pub fn reason_sentence(reason: &str, locale: Locale) -> Option<&'static str> {
    let zh = locale.resolved() == Locale::ZhTw;
    let pick = |en: &'static str, zh_tw: &'static str| Some(if zh { zh_tw } else { en });
    match reason {
        "gnome.no_scroll_factor_key" => pick(
            "GNOME has no setting for this, so scrolling follows the pointer speed.",
            "GNOME 沒有這個設定，捲動距離會跟著游標速度。",
        ),
        "gnome.no_smooth_scroll_key" => pick(
            "GNOME has no setting for this; the desktop and each app decide it.",
            "GNOME 沒有這個設定，由桌面和各個 App 自己決定。",
        ),
        "gnome.no_key" | "gnome.key_absent" => pick(
            "This version of GNOME has no setting for this.",
            "這個版本的 GNOME 沒有這項設定。",
        ),
        "gnome.read_only_backend" => pick(
            "Settings are only being read right now, not changed.",
            "目前只讀取設定，不會變更。",
        ),
        "gnome.dconf_service_unreachable" | "gnome.no_bus" => pick(
            "The desktop's settings service isn't answering, so this can't be changed right now.",
            "桌面的設定服務沒有回應，暫時無法變更。",
        ),
        "gnome.database_not_readable" | "gnome.database_unreadable" => pick(
            "The desktop's settings can't be read right now.",
            "目前讀不到桌面的設定。",
        ),
        "gnome.value_kind_does_not_fit_the_key" => pick(
            "This GNOME stores this setting in a form Better OS doesn't change.",
            "這個 GNOME 用 Better OS 不會變更的格式儲存這項設定。",
        ),
        "touchpad.pad_reports_one_contact" => pick(
            "This touchpad only senses one finger at a time.",
            "這個觸控板一次只能感應一根手指。",
        ),
        "touchpad.pad_has_a_middle_button" => pick(
            "This touchpad has a real middle button.",
            "這個觸控板有實體中鍵。",
        ),
        "touchpad.pad_has_separate_buttons" => pick(
            "This touchpad has separate buttons, so there's only one way to click.",
            "這個觸控板有獨立按鍵，只有一種按鍵判定方式。",
        ),
        "touchpad.backend_owns_nothing" | "touchpad.backend_declares_no_support" => pick(
            "Nothing can be read or changed in this session.",
            "這個工作階段無法讀取或變更任何設定。",
        ),
        "touchpad.safe_mode" => pick(
            "Safe mode is on, so nothing is changed.",
            "安全模式已開啟，不會做任何變更。",
        ),
        "touchpad.integration_disabled" => {
            pick("Better Touchpad is turned off.", "Better Touchpad 已關閉。")
        }
        "gnome.database_beneath_unreadable" => pick(
            "One of the settings databases this session reads from can't be read, so this value is unknown.",
            "這個工作階段讀取的其中一個設定資料庫讀不到，所以不知道這個值。",
        ),
        // Which dconf database the session reads, and why it could not be
        // decided: the sentences Better Defaults shows too.
        other => touchpad_platform::dconf_profile::sentence(other)
            .map(|(en, zh_tw)| if zh { zh_tw } else { en }),
    }
}

/// A gesture problem in the person's words. The gesture half reports its
/// problems as stable keys, sometimes followed by `:` and a technical
/// detail (`gestures.import.no_such_file: /tmp/x.json`); the key decides the
/// sentence and the whole string is kept for the details. An unknown key
/// gets the generic sentence, never the key itself (audit T2).
pub fn problem_sentence(problem: &str, locale: Locale) -> &'static str {
    let zh = locale.resolved() == Locale::ZhTw;
    let pick = |en: &'static str, zh_tw: &'static str| if zh { zh_tw } else { en };
    let key = problem.split([':', ' ']).next().unwrap_or_default().trim();
    match key {
        "gestures.adapter_disabled_after_failures" => pick(
            "Three gestures in a row didn't work, so they were turned off.",
            "連續三次手勢沒有成功，所以先關閉了。",
        ),
        "gestures.export.no_path" => pick(
            "Choose where to save the file first.",
            "請先選擇要存到哪裡。",
        ),
        "gestures.import.no_such_file" => pick("That file isn't there.", "找不到這個檔案。"),
        "gestures.config.not_json"
        | "gestures.config.no_schema_version"
        | "gestures.config.unknown_schema"
        | "gestures.config.malformed_device_identity"
        | "gestures.config.unknown_active_device" => pick(
            "That file isn't a gesture settings file Better OS can read.",
            "這不是 Better OS 讀得懂的手勢設定檔。",
        ),
        "gestures.config.newer_schema" => pick(
            "That file was made by a newer Better OS.",
            "這個檔案是較新版本的 Better OS 做的。",
        ),
        "gestures.config.too_many_devices" | "gestures.config.too_many_gestures" => pick(
            "That file holds more than Better OS accepts.",
            "這個檔案的內容超過 Better OS 能接受的數量。",
        ),
        "gestures.plan.not_confirmed" => {
            pick("Confirm the changes above first.", "請先確認上面的變更。")
        }
        "gestures.plan.unresolved_conflict" => {
            pick("Decide every conflict first.", "每一個衝突都要先決定。")
        }
        "gestures.plan.nothing_to_do" => {
            pick("There's nothing to change.", "沒有任何項目需要變更。")
        }
        _ => pick("That didn't work.", "沒有成功。"),
    }
}

pub fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN_US,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_problem_key_is_never_the_sentence() {
        use super::{Locale, problem_sentence, reason_sentence};
        assert_eq!(
            problem_sentence("gestures.adapter_disabled_after_failures:3", Locale::ZhTw),
            "連續三次手勢沒有成功，所以先關閉了。"
        );
        assert_eq!(
            problem_sentence("gestures.import.no_such_file: /tmp/x", Locale::EnUs),
            "That file isn't there."
        );
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let unknown = problem_sentence("something.new", locale);
            assert!(!unknown.contains('.') || unknown.ends_with('.') || unknown.ends_with('。'));
            assert!(!unknown.contains("something"));
        }
        assert_eq!(
            reason_sentence("gnome.no_scroll_factor_key", Locale::ZhTw),
            Some("GNOME 沒有這個設定，捲動距離會跟著游標速度。")
        );
        assert_eq!(reason_sentence("mock.no_key", Locale::EnUs), None);
    }

    // Review finding 4: a session whose dconf database could not be decided
    // says why and what to do, in both languages, rather than "not available".
    #[test]
    fn every_dconf_refusal_has_a_sentence_in_both_languages() {
        for key in [
            "dconf.profile_missing",
            "dconf.profile_unreadable",
            "dconf.profile_not_writable",
            "dconf.profile_invalid_database_name",
            "dconf.session_profile_not_in_environment",
            "dconf.profile_nfs_dconf_version_unknown",
            "gnome.database_beneath_unreadable",
        ] {
            let en = reason_sentence(key, Locale::EnUs).unwrap_or_else(|| panic!("{key}"));
            let zh = reason_sentence(key, Locale::ZhTw).unwrap_or_else(|| panic!("{key}"));
            assert_ne!(en, zh, "{key}");
        }
        assert!(
            reason_sentence("dconf.session_profile_not_in_environment", Locale::EnUs)
                .unwrap()
                .contains("sign out")
        );
    }

    use super::*;

    #[test]
    fn the_scrolling_page_says_the_speed_is_for_every_touchpad() {
        // The scroll pass-through sets the same factors on every touchpad
        // (ADR 0030); the page must not let a person think it is per pad.
        assert!(EN_US.scrolling_subtitle.contains("every touchpad"));
        assert!(ZH_TW.scrolling_subtitle.contains("每個觸控板"));
    }

    #[test]
    fn the_fixed_traditional_chinese_terms_are_used_verbatim() {
        // Issue #3 fixes these four for the touchpad half. A rewording that
        // dropped one would be a product decision, not a copy edit.
        assert_eq!(ZH_TW.pointer_sensitivity, "游標靈敏度");
        assert_eq!(ZH_TW.scroll_sensitivity, "捲動靈敏度");
        assert_eq!(ZH_TW.natural_scrolling, "自然捲動");
        assert_eq!(ZH_TW.tap_to_click, "輕點即按一下");
        assert_eq!(ZH_TW.pointer_summary, "游標靈敏度");
        assert_eq!(ZH_TW.scroll_summary, "捲動靈敏度");
    }

    #[test]
    fn the_fixed_gesture_terms_are_used_verbatim() {
        // Issue #3 fixes these four for the gesture half.
        assert_eq!(ZH_TW.nav_gestures, "手勢");
        assert_eq!(ZH_TW.gestures_title, "手勢");
        assert_eq!(ZH_TW.action_show_desktop, "顯示桌面");
        assert_eq!(ZH_TW.action_current_app_windows, "顯示所有 App");
        assert_eq!(ZH_TW.action_overview, "工作區總覽");
        assert_eq!(ZH_TW.gesture_show_desktop, "顯示桌面");
        assert_eq!(ZH_TW.gesture_overview, "工作區總覽");
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
            copy.appearance,
            copy.appearance_description,
            copy.appearance_system,
            copy.appearance_light,
            copy.appearance_dark,
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
            copy.nav_gestures,
            copy.gestures_title,
            copy.gestures_subtitle,
            copy.preset_title,
            copy.preset_description,
            copy.preset_not_applied,
            copy.preset_applied,
            copy.preset_differs,
            copy.preview_changes,
            copy.confirm_changes,
            copy.apply_preset,
            copy.cancel_preview,
            copy.changes_heading,
            copy.conflicts_heading,
            copy.conflict_badge,
            copy.unsupported_badge,
            copy.unresolved_note,
            copy.resolution_keep_built_in,
            copy.resolution_disable_built_in,
            copy.resolution_remap,
            copy.test_gestures,
            copy.test_gestures_hint,
            copy.live_testing,
            copy.live_testing_off,
            copy.test_run,
            copy.test_no_events,
            copy.recognized_events,
            copy.edit_gesture,
            copy.save_gesture,
            copy.contacts_label,
            copy.thumb_label,
            copy.shape_label,
            copy.action_heading,
            copy.activation_label,
            copy.cancellation_label,
            copy.cooldown_label,
            copy.enabled_label,
            copy.direction_heading,
            copy.profile_heading,
            copy.profile_global,
            copy.profile_this_device,
            copy.profile_follows_global,
            copy.profile_own,
            copy.profile_forget,
            copy.profile_detach,
            copy.shortcut_heading,
            copy.shortcut_modifiers,
            copy.shortcut_key,
            copy.shortcut_needs_modifier,
            copy.shortcut_conflict,
            copy.shortcut_none_recorded,
            copy.shortcut_unknown,
            copy.group_letters,
            copy.group_digits,
            copy.group_function,
            copy.group_navigation,
            copy.group_editing,
            copy.group_punctuation,
            copy.profiles_file,
            copy.export_label,
            copy.export_profiles,
            copy.import_label,
            copy.import_profiles,
            copy.import_summary,
            copy.import_uses_global,
            copy.restore_gestures,
            copy.disable_gestures,
            copy.no_gestures,
            copy.gesture_backend_none,
            copy.gesture_service_stopped,
            copy.gesture_collision_unreliable,
            copy.gesture_thumb_best_effort,
            copy.gesture_launcher,
            copy.gesture_show_desktop,
            copy.gesture_overview,
            copy.gesture_current_app_windows,
            copy.gesture_workspace_next,
            copy.gesture_workspace_previous,
            copy.gesture_app_back,
            copy.gesture_app_forward,
            copy.gesture_app_zoom,
            copy.gesture_app_rotate,
            copy.action_launcher_open,
            copy.action_launcher_close,
            copy.action_show_desktop,
            copy.action_overview,
            copy.action_current_app_windows,
            copy.action_workspace_next,
            copy.action_workspace_previous,
            copy.action_app_next,
            copy.action_app_previous,
            copy.action_app_back,
            copy.action_app_forward,
            copy.action_app_zoom,
            copy.action_app_rotate,
            copy.action_media_play_pause,
            copy.action_volume_up,
            copy.action_volume_down,
            copy.action_volume_mute,
            copy.action_shortcut,
            copy.action_none,
            copy.shape_swipe,
            copy.shape_pinch,
            copy.shape_spread,
            copy.shape_hold,
            copy.shape_tap,
            copy.shape_rotate,
            copy.direction_up,
            copy.direction_down,
            copy.direction_left,
            copy.direction_right,
            copy.direction_clockwise,
            copy.direction_counter_clockwise,
            copy.verification_verified,
            copy.verification_failed,
            copy.verification_not_run,
            copy.verification_unsupported,
            copy.changes_none,
        ]
    }
}

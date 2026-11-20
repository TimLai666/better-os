//! The System page's words (spec sections 2 and 4), in both languages.
//! Each line is the spec's, word for word, except the few the spec leaves
//! to the page, which say so.

use crate::i18n::Locale;

pub(crate) struct Copy {
    pub(crate) empty: &'static str,

    pub(crate) group_power: &'static str,
    pub(crate) group_display: &'static str,
    pub(crate) group_cooling: &'static str,
    pub(crate) group_about: &'static str,

    pub(crate) power_mode: &'static str,
    pub(crate) power_mode_description: &'static str,
    pub(crate) power_saver: &'static str,
    pub(crate) balanced: &'static str,
    pub(crate) performance: &'static str,
    pub(crate) degraded_lap: &'static str,
    pub(crate) degraded_hot: &'static str,
    /// Not in the spec: a reason this version does not know. `{reason}`.
    pub(crate) degraded_other: &'static str,
    /// `{application}` and `{mode}`.
    pub(crate) held: &'static str,

    // The charger switch (owner, 2026-11-13).
    pub(crate) follow_header: &'static str,
    /// Not in the spec: what the two targets are called.
    pub(crate) follow_description: &'static str,
    pub(crate) follow_plugged_in: &'static str,
    pub(crate) follow_on_battery: &'static str,
    pub(crate) follow_unreadable: &'static str,

    pub(crate) charge_limit: &'static str,
    pub(crate) charge_limit_description: &'static str,
    pub(crate) charge_100: &'static str,
    pub(crate) charge_80: &'static str,
    pub(crate) charge_60: &'static str,
    /// `{n}`.
    pub(crate) charge_other: &'static str,
    pub(crate) charge_auth_note: &'static str,
    /// Not the spec's "resets to 100%" line, which nobody has observed
    /// (ticket 86): a neutral one, true whether or not the firmware keeps it.
    pub(crate) charge_at_startup: &'static str,
    /// What the battery is doing now, on the charge limit card (ticket 170).
    pub(crate) charge_now: &'static str,
    /// What GNOME's own battery icon shows while the battery is held at a
    /// limit under 100%.
    pub(crate) charge_desktop_icon: &'static str,

    pub(crate) battery: &'static str,
    /// `{percent}`.
    pub(crate) battery_health: &'static str,
    /// `{n}`.
    pub(crate) battery_cycles: &'static str,
    /// `{model}`.
    pub(crate) battery_model: &'static str,

    pub(crate) brightness: &'static str,
    pub(crate) brightness_description: &'static str,
    pub(crate) keyboard: &'static str,
    /// `{levels}`.
    pub(crate) keyboard_description: &'static str,
    pub(crate) keyboard_off: &'static str,
    /// `{n}`.
    pub(crate) keyboard_level: &'static str,

    pub(crate) fan_mode: &'static str,
    pub(crate) fan_mode_description: &'static str,
    pub(crate) fan_auto: &'static str,
    pub(crate) fan_full: &'static str,
    pub(crate) fan_after_restart: &'static str,

    pub(crate) sensors: &'static str,
    /// `{hottest}` (label and degrees) and `{rpm}`.
    pub(crate) sensors_description: &'static str,
    /// The description when there is no temperature. `{rpm}`.
    pub(crate) sensors_fan_only: &'static str,
    pub(crate) sensors_unreadable: &'static str,

    pub(crate) processor: &'static str,
    pub(crate) graphics: &'static str,
    pub(crate) ssd: &'static str,
    pub(crate) motherboard: &'static str,
    pub(crate) wifi: &'static str,
    pub(crate) cpu_fan: &'static str,

    pub(crate) about: &'static str,
    // Not in the spec: the rows' names.
    pub(crate) about_device: &'static str,
    pub(crate) about_system: &'static str,
    pub(crate) about_architecture: &'static str,
    pub(crate) about_kernel: &'static str,
    pub(crate) about_desktop: &'static str,
    pub(crate) about_unknown: &'static str,
    pub(crate) copy: &'static str,
    pub(crate) copied: &'static str,

    // The {reason} of "That didn't change. {reason}" (not in the spec).
    pub(crate) reason_no_service: &'static str,
    pub(crate) reason_absent: &'static str,
    pub(crate) reason_write: &'static str,

    // Home.
    pub(crate) home_worn_header: &'static str,
    /// `{percent}`.
    pub(crate) home_worn_description: &'static str,
    pub(crate) view: &'static str,
}

static EN: Copy = Copy {
    empty: "This PC has no hardware settings Better OS can change.",
    group_power: "Power",
    group_display: "Display",
    group_cooling: "Cooling",
    group_about: "About this PC",
    power_mode: "Power mode",
    power_mode_description: "Balance battery life against performance.",
    power_saver: "Best power efficiency",
    balanced: "Balanced",
    performance: "Best performance",
    degraded_lap: "Performance is reduced while the PC is on your lap, to keep it cool.",
    degraded_hot: "It's running hot, so performance is reduced for now.",
    degraded_other: "Performance is reduced for now ({reason}).",
    held: "{application} is asking for {mode}.",
    follow_header: "Switch power mode with the charger",
    follow_description: "Uses one mode when plugged in and another on battery. A mode you pick by hand stays until the next time you plug in or unplug.",
    follow_plugged_in: "When plugged in",
    follow_on_battery: "On battery",
    follow_unreadable: "This setting couldn't be read, so it's shown as off.",
    charge_limit: "Battery charge limit",
    charge_limit_description: "Stopping short of full extends battery life when you're mostly plugged in.",
    charge_100: "Charge to 100% (no limit)",
    charge_80: "Charge to 80% (best for mostly plugged in)",
    charge_60: "Charge to 60% (longest battery life)",
    charge_other: "Charge to {n}% (set elsewhere)",
    charge_auth_note: "You'll be asked for an administrator password.",
    charge_at_startup: "Better OS sets it again each time the PC starts.",
    charge_now: "Now: {state}.",
    charge_desktop_icon: "At the limit, the desktop's own battery icon looks the same as on battery, with no charging mark: GNOME draws it that way, and nothing outside GNOME can change it.",
    battery: "Battery",
    battery_health: "Battery health: holds {percent}% of its original capacity",
    battery_cycles: "Charge cycles: {n}",
    battery_model: "Model: {model}",
    brightness: "Brightness",
    brightness_description: "Change the brightness of the built-in display.",
    keyboard: "Keyboard backlight",
    keyboard_description: "{levels} levels; 0 is off.",
    keyboard_off: "Off",
    keyboard_level: "Level {n}",
    fan_mode: "Fan mode",
    fan_mode_description: "Automatic follows the temperature. Full speed is for short bursts and is loud.",
    fan_auto: "Automatic",
    fan_full: "Full speed",
    fan_after_restart: "It always goes back to Automatic when the PC restarts.",
    sensors: "Temperatures and fans",
    sensors_description: "{hottest} · Fan {rpm}",
    sensors_fan_only: "Fan {rpm}",
    sensors_unreadable: "Can't be read right now",
    processor: "Processor",
    graphics: "Graphics",
    ssd: "SSD",
    motherboard: "Motherboard",
    wifi: "Wi-Fi",
    cpu_fan: "CPU fan",
    about: "Device specifications",
    about_device: "Device name",
    about_system: "Operating system",
    about_architecture: "System type",
    about_kernel: "Kernel",
    about_desktop: "Desktop",
    about_unknown: "Couldn't be read",
    copy: "Copy",
    copied: "Copied",
    reason_no_service: "The Better OS service that makes this change didn't answer.",
    reason_absent: "This PC's hardware didn't offer this setting when it was changed.",
    reason_write: "The hardware didn't accept the new value.",
    home_worn_header: "Battery is wearing out",
    home_worn_description: "It now holds {percent}% of its original capacity.",
    view: "View",
};

static ZH_TW: Copy = Copy {
    empty: "這台電腦沒有 Better OS 能調整的硬體設定。",
    group_power: "電源",
    group_display: "顯示",
    group_cooling: "散熱",
    group_about: "關於這台電腦",
    power_mode: "電源模式",
    power_mode_description: "在電池續航力和效能之間取捨。",
    power_saver: "最佳電源效率",
    balanced: "平衡",
    performance: "最佳效能",
    degraded_lap: "電腦放在腿上時，效能模式會降低以免過熱。",
    degraded_hot: "溫度太高，效能模式暫時降低。",
    degraded_other: "效能模式暫時降低（{reason}）。",
    held: "{application} 正在要求「{mode}」模式。",
    follow_header: "依充電狀態自動切換電源模式",
    follow_description: "接上電源和使用電池時各用一種模式。你自己選的模式會維持到下次插拔電源。",
    follow_plugged_in: "接上電源時",
    follow_on_battery: "使用電池時",
    follow_unreadable: "讀不到這項設定，所以先顯示為關閉。",
    charge_limit: "電池充電上限",
    charge_limit_description: "長時間接著電源時，充到較低的電量可以延長電池壽命。",
    charge_100: "充到 100%（不限制）",
    charge_80: "充到 80%（建議長時間接電使用）",
    charge_60: "充到 60%（電池壽命最長）",
    charge_other: "充到 {n}%（在其他地方設定的）",
    charge_auth_note: "變更時會要求輸入管理者密碼。",
    charge_at_startup: "每次開機時，Better OS 會重新套用這個上限。",
    charge_now: "目前：{state}。",
    charge_desktop_icon: "電池停在上限時，桌面本身的電池圖示看起來會跟使用電池時一樣，沒有充電符號：這是 GNOME 自己的畫法，GNOME 以外的程式改不了。",
    battery: "電池",
    battery_health: "電池健康度：目前可充到出廠容量的 {percent}%",
    battery_cycles: "充電次數：{n}",
    battery_model: "型號：{model}",
    brightness: "螢幕亮度",
    brightness_description: "調整內建螢幕的亮度。",
    keyboard: "鍵盤背光",
    keyboard_description: "{levels} 段亮度，0 是關閉。",
    keyboard_off: "關閉",
    keyboard_level: "第 {n} 段",
    fan_mode: "風扇模式",
    fan_mode_description: "自動會依溫度調整。全速只適合短時間散熱，會比較吵。",
    fan_auto: "自動",
    fan_full: "全速",
    fan_after_restart: "重新開機後一律回到自動。",
    sensors: "溫度與風扇",
    sensors_description: "{hottest} · 風扇 {rpm}",
    sensors_fan_only: "風扇 {rpm}",
    sensors_unreadable: "目前讀不到",
    processor: "處理器",
    graphics: "顯示晶片",
    ssd: "固態硬碟",
    motherboard: "主機板",
    wifi: "無線網路",
    cpu_fan: "處理器風扇",
    about: "裝置規格",
    about_device: "裝置名稱",
    about_system: "作業系統",
    about_architecture: "系統類型",
    about_kernel: "核心版本",
    about_desktop: "桌面環境",
    about_unknown: "讀不到",
    copy: "複製",
    copied: "已複製",
    reason_no_service: "負責變更這項設定的 Better OS 服務沒有回應。",
    reason_absent: "變更時，這台電腦的硬體沒有提供這項設定。",
    reason_write: "硬體沒有接受新的值。",
    home_worn_header: "電池老化",
    home_worn_description: "目前最多只能充到出廠容量的 {percent}%。",
    view: "查看",
};

pub(crate) fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        Locale::EnUs | Locale::System => &EN,
    }
}

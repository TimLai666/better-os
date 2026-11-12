# Better Manager as the control center

What the control center contains, how each page behaves in every state, the
exact copy in both languages, where every function of today's four windows
goes, and how hardware is detected and written. The decision behind it is
[ADR 0017](../docs/decisions/0017-one-control-center.md); the work is cut into
tickets 84 to 91. The look follows `DESIGN.md`, and every control named here
is a `better_ui::fluent` control (ticket 80). The mockups in
`design/control-center/` draw Home, System, and Keep awake in both appearances.

Where this file and a ticket differ, the ticket decides behaviour and this file
decides layout and copy.

## Sources this design was checked against

- Today's windows, read in full on 2026-11-12: `crates/manager-gui` (including
  its Defaults pages, `pages_defaults.rs`, `defaults_model.rs`,
  `defaults_app.rs`), `crates/awake-gui`, `crates/touchpad-gui`, and the
  Better Awake tray `crates/awake-tray`, plus their copy in each `i18n.rs`.
- The UI audit of 2026-11-12 (findings M1–M14, A1–A8, T1–T10, and the ten
  cross-window problems), whose screenshots are in the session scratchpad.
- The Fluent kit's public API in `crates/better-ui/src/fluent/` and the
  gallery, run in a headless sway session.
- The owner's machine, an ASUS Vivobook S 14 M5406WA on Zorin OS 18.1,
  GNOME Shell 46.0, kernel 7.0.0-42, read without writing anything. Every
  hardware fact below marked "read on the machine" comes from that read.

## 1. The window

One window, `io.betteros.Manager`, opened by `better-manager`. Minimum size
stays 960 × 600.

```
┌───────────────────────────────────────────────────────────────────────┐
│ ←  Better Manager                                            –  □  ×  │ 32  titlebar
├──────────────────────┬────────────────────────────────────────────────┤
│ [🔍 尋找設定       ] │  系統                                           │ Title 28/36
│                      │                                                │
│ ▌⌂ 首頁              │  ┌ 電源模式 ───────────────────── [平衡   ▾] ┐ │ settings cards
│  ▣ 系統              │  ┌ 電池充電上限 ────────────────── [80%   ▾] ┐ │ 4px apart
│  ▢ 觸控板            │  ┌ 螢幕亮度 ──────────────── ───●──────  55 ┐ │
│  ☀ 保持清醒          │  ...                                           │
│  ▤ 應用程式      (2) │                                                │
│  ☆ 預設應用程式      │                                                │
│  ⚠ 修正              │                                                │
│                      │                                                │
│  ⚙ 設定              │                                                │
└──────────────────────┴────────────────────────────────────────────────┘
        320                              flexible, content max 1000
```

- **Titlebar** through `better_ui::window_chrome`, title `Better Manager`
  in both languages (the product name is not translated, which settles audit
  M12). A back button (←) sits at its leading edge whenever the page history
  is not empty, as in Windows 11 Settings; Alt+Left does the same.
- **Navigation pane**: `fluent::navigation::NavigationView`, 320px, with the
  search box above the items. Below 1000px logical width the pane goes to
  48px behind ☰ and the search becomes an icon button that opens the pane
  (audit M5). It never disappears.
- **Content**: page title in Title, then settings cards in groups. Each group
  may have a Body Strong group header 8px above it. Content is at most 1000px
  wide and starts 24px from the pane, as in Windows 11 Settings.
- **Sub-pages** (a card with a chevron opens one) put a `BreadcrumbBar` in
  place of the title: `應用程式 › Better Files`. Clicking an earlier crumb
  goes back to it.

### Navigation items

The order is the order a person reaches for them. Icons are gpui-component's
Lucide set, the only set the kit draws; there is no touchpad or house glyph in
it, so the two nearest are used and listed in section 13 as a kit gap.

| Item | zh-TW | English | Icon | Badge |
| --- | --- | --- | --- | --- |
| Home | 首頁 | Home | `LayoutDashboard` | none |
| System | 系統 | System | `Cpu` | none |
| Touchpad | 觸控板 | Touchpad | `Frame` | none |
| Keep awake | 保持清醒 | Keep awake | `Sun` | a dot while a session holds the machine awake |
| Apps | 應用程式 | Apps | `Inbox` | the number of updates, when above zero |
| Default apps | 預設應用程式 | Default apps | `Star` | none |
| Fixes | 修正 | Fixes | `TriangleAlert` | the number of faults found, when above zero |
| Settings (footer) | 設定 | Settings | `Settings` | none |

An item is never hidden because its component is missing: Touchpad and Keep
awake open their page and the page says the component is not installed, with
an Install button. Hiding them would leave a person who reads "Better Awake"
in a release note with nowhere to find it.

### Search ("Find a setting")

| | zh-TW | English |
| --- | --- | --- |
| Placeholder | 尋找設定 | Find a setting |
| No result | 找不到符合「{query}」的設定。 | No settings match "{query}". |

Search matches the header and description of every settings card on every
page, in the current language, by prefix of any word, case-insensitively for
Latin text, and shows at most eight results in the box's suggestion list,
each as `{card header}` over `{page} › {group}` in Caption. Enter or a click
opens the page and scrolls the card into view with keyboard focus on it. The
index is a plain list built from the same copy tables the pages draw from, in
a module with no GPUI dependency.

### Opening a page from outside

`better-manager --page <page>` opens the window on that page. `<page>` is one
of `home`, `system`, `touchpad`, `keep-awake`, `apps`, `updates`,
`default-apps`, `fixes`, `settings`, plus `keep-awake/until`, which opens Keep awake with the "Until a time" dialog (the tray's entry). A second invocation while a window is
open asks the running window to show that page and raise itself, over the
session bus name `io.betteros.Manager` (method `ShowPage(s)`), then exits 0;
there is never a second window. An unknown page is refused with the same
two-language message pattern `better-manager` uses today for an unknown
argument. Every other argument is still refused (ticket 60).

The tray's "Open Better Awake…" and "Change session…" run
`better-manager --page keep-awake`, and "Until a time…" runs
`better-manager --page keep-awake/until`; the Touchpad and Keep awake desktop
entries run the same command with their page (ADR 0017).

## 2. Shared patterns

### States every page and card uses

Each state is drawn with a kit control, never a bespoke box.

| State | How it is drawn | zh-TW | English |
| --- | --- | --- | --- |
| Loading (first read, under 300 ms) | nothing; the previous content or an empty page stays | — | — |
| Loading (longer) | `ProgressRing` 32px centred under the title, Caption beside it | 正在讀取… | Reading… |
| Component not installed | One `SettingsCard` with the component icon, accent `Button` | 需要安裝 {component} 才能使用這一頁。 / 安裝 | {component} needs to be installed to use this page. / Install |
| Component too old for this page | One `SettingsCard`, accent `Button` | 這一頁需要較新版本的 {component}。目前是 {installed}，需要 {required} 以上。 / 更新 | This page needs a newer {component}. You have {installed}; it needs {required} or later. / Update |
| Its service is not running | `InfoBar` Warning, cards below disabled, action `Button` | {component} 的背景服務沒有在執行，這一頁暫時無法讀取或變更。 / 啟動服務 | {component}'s background service isn't running, so this page can't read or change anything right now. / Start service |
| Hardware not present | the card is not drawn at all; if a whole group is empty, the group header is not drawn either | — | — |
| Hardware present, value unreadable | the card is drawn disabled with the reason as its description | 這台電腦有這項功能，但目前讀不到它的狀態。 | This PC has this, but its state can't be read right now. |
| No permission (person cancelled or failed authentication) | `InfoBar` Warning above the card group, no close button until the next action | 沒有變更：身分驗證沒有完成。 | Nothing changed: authentication wasn't completed. |
| Not allowed (polkit says no for this session) | `InfoBar` Error | 這個帳號沒有權限變更這項設定。 | This account isn't allowed to change this setting. |
| A write failed | `ErrorBar` (one sentence, expandable copyable details) | 沒有變更成功。{reason} | That didn't change. {reason} |
| A write read back different | `InfoBar` Warning | 已寫入，但讀回來的值是 {value}。 | It was written, but it reads back as {value}. |
| Empty list | Body sentence in secondary text, at most one standard `Button` | (per page) | (per page) |

"Start service" asks `manager-core` to start the component's user units
through `UserUnitController` (AGENTS.md: a window never starts a unit
itself). It is offered only for an installed, enabled component.

### Errors

Every error uses `ErrorReport`: the summary is one sentence in the person's
words, the details are the technical chain (D-Bus error name, errno, path),
collapsed by default, with a Copy button. No D-Bus error string is ever the
summary (audit A2).

### Units and words

All on/off words, states, units, and percentages come from
`better_ui::fluent::words` (ticket 80). Temperatures are whole degrees Celsius
(`52 °C`), fan speed is `4,800 RPM` in English and `4,800 RPM` in zh-TW,
percentages have no space (`80%`).

## 3. Home

The page a person lands on. It answers one question: does anything here need
me? Windows 11 Settings' Home is the model: a device banner, then cards.

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 首頁 | Home |
| Device banner, line 1 (Subtitle) | {product name from DMI} | {product name from DMI} |
| Device banner, line 2 (Caption) | {distribution} · Better OS {manager version} | {distribution} · Better OS {manager version} |
| Attention group header | 需要你處理 | Needs your attention |
| Nothing needs attention (one card, success icon) | 一切正常，沒有需要處理的事。 | Everything's fine. Nothing needs your attention. |
| Quick settings group header | 常用設定 | Quick settings |
| Recent activity group header | 最近活動 | Recent activity |
| Recent activity link | 查看全部活動 | See all activity |

The device banner shows `/sys/class/dmi/id/product_name` with the
manufacturer's model suffix removed when it repeats (`ASUS Vivobook S 14
M5406WA_M5406WA` reads as `ASUS Vivobook S 14 M5406WA`; the vendor comes from
`sys_vendor`, `ASUSTeK COMPUTER INC.`, and is shown as the first word of the
product name already). A battery icon and charge sit at its trailing edge when
a battery exists: `🔋 87%` with `充電中` / `Charging`, `使用電池` / `On
battery`, or `已充滿` / `Full`.

### Attention cards

Each card is a `SettingsCard` with an icon in the status colour, a Body Strong
header, a description, and one button that goes where the fix is. They are
ordered by severity, then by this table's order. Each page module supplies its
own items through one function with no GPUI dependency, so Home never
duplicates a page's rules.

| Source | When | Header (zh-TW / en) | Description (zh-TW / en) | Button (zh-TW / en) → target |
| --- | --- | --- | --- | --- |
| Apps | dpkg left an install unfinished | 套件安裝中途被中斷 / A package installation was interrupted | 完成之前，任何元件都無法安裝、更新或移除。 / Nothing can be installed, updated, or removed until it's finished. | 修復 / Repair → Apps › Health |
| Apps | a component failed or is degraded | {component} 需要處理 / {component} needs attention | 它沒有通過檢查。 / It didn't pass its check. | 查看 / View → its details |
| Apps | record and machine disagree | {component} 和紀錄對不上 / {component} doesn't match its record | 對齊之前無法安裝、更新或還原。 / It can't be installed, updated, or restored until they match. | 查看 / View → its details |
| Apps | updates available | 有 {n} 個更新 / {n} updates available | 確認後才會安裝。 / Nothing is installed until you confirm. | 查看更新 / See updates → Apps › Updates |
| Apps | catalog may be outdated | 元件清單可能不是最新的 / The component list may be out of date | (the catalog's degraded sentence, unchanged) | 更新清單 / Refresh list → refresh in place |
| Keep awake | service installed but not running | 保持清醒的背景服務沒有在執行 / The keep-awake service isn't running | 規則不會觸發，也無法開始工作階段。 / Rules won't fire and no session can start. | 啟動服務 / Start service → in place |
| Keep awake | a session is active | 這台電腦正被保持清醒 / This PC is being kept awake | {first reason}，剩餘 {time} / {first reason}, {time} left | 查看 / View → Keep awake |
| Keep awake | inhibitor backend unavailable or attention required | 保持清醒無法擋下睡眠 / Keep awake can't stop sleep | (the service's reason, translated) | 查看 / View → Keep awake › Diagnostics |
| Touchpad | safe mode is on | 觸控板安全模式已開啟 / Touchpad safe mode is on | Better OS 不會變更觸控板設定，也不會處理手勢。 / Better OS won't change touchpad settings or handle gestures. | 關閉安全模式 / Turn off safe mode → in place |
| Touchpad | integration turned itself off after three failed gestures | 手勢已自動關閉 / Gestures turned themselves off | 連續三次手勢沒有成功，所以先關閉了。 / Three gestures in a row didn't work, so they were turned off. | 查看 / View → Touchpad › Gestures |
| Default apps | a default changed elsewhere | 有預設值被其他程式變更 / A default was changed by another app | {n} 項設定和 Better OS 套用的不同。 / {n} settings no longer match what Better OS set. | 查看 / View → Default apps |
| Fixes | a known fault was found | 找到 {n} 個已知問題 / {n} known problems found | {first fault's one-line title} / {first fault's one-line title} | 查看 / View → Fixes |
| System | battery health under 80% of design | 電池老化 / Battery is wearing out | 目前最多只能充到出廠容量的 {percent}%。 / It now holds {percent}% of its original capacity. | 查看 / View → System › Battery |

Info-level items (a session is active) are shown after warnings and never on
their own make the "Everything's fine" card disappear; that card is replaced
only by warning or error items.

### Quick settings

Three cards that repeat the most-used controls, live, each with the control
itself (not a link): Power mode (combo box), Keep awake (a toggle that starts
the default quick session or ends the manual one), Appearance (combo box).
A card whose hardware or component is missing is not drawn.

| Card | zh-TW | English |
| --- | --- | --- |
| Keep awake toggle header | 保持清醒 | Keep awake |
| …description when off | 依照電源設定睡眠 | Sleeps by your power settings |
| …description when on | {reason}，剩餘 {time} | {reason}, {time} left |

### Recent activity

The five latest entries of Apps › Activity, as list rows (36px, icon in
status colour, Body Strong summary, Caption time). Empty: `還沒有活動紀錄。` /
`No activity yet.`

## 4. System

Hardware controls. Every card here exists only when the machine has the thing
(section 11 says how each is detected); a machine with none of them shows the
empty sentence.

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 系統 | System |
| Empty | 這台電腦沒有 Better OS 能調整的硬體設定。 | This PC has no hardware settings Better OS can change. |

### Group: 電源 / Power

**Power mode** — `ComboBox`.

| | zh-TW | English |
| --- | --- | --- |
| Header | 電源模式 | Power mode |
| Description | 在電池續航力和效能之間取捨。 | Balance battery life against performance. |
| power-saver | 最佳電源效率 | Best power efficiency |
| balanced | 平衡 | Balanced |
| performance | 最佳效能 | Best performance |
| Degraded (`PerformanceDegraded` = `lap-detected`) | 電腦放在腿上時，效能模式會降低以免過熱。 | Performance is reduced while the PC is on your lap, to keep it cool. |
| Degraded (`high-operating-temperature`) | 溫度太高，效能模式暫時降低。 | It's running hot, so performance is reduced for now. |
| Held by an app (`ActiveProfileHolds` not empty) | {application} 正在要求「{mode}」模式。 | {application} is asking for {mode}. |
| Not available (no power-profiles-daemon) | card not drawn | — |

Only the profiles the daemon's `Profiles` property lists are offered. The
holds line and the degraded line are a second description line in the
caution colour with a caution icon.

**Battery charge limit** — `ComboBox`, privileged.

| | zh-TW | English |
| --- | --- | --- |
| Header | 電池充電上限 | Battery charge limit |
| Description | 長時間接著電源時，充到較低的電量可以延長電池壽命。 | Stopping short of full extends battery life when you're mostly plugged in. |
| 100 | 充到 100%（不限制） | Charge to 100% (no limit) |
| 80 | 充到 80%（建議長時間接電使用） | Charge to 80% (best for mostly plugged in) |
| 60 | 充到 60%（電池壽命最長） | Charge to 60% (longest battery life) |
| Other value found | 充到 {n}%（在其他地方設定的） | Charge to {n}% (set elsewhere) |
| Authentication note (Caption, shown once the combo box opens) | 變更時會要求輸入管理者密碼。 | You'll be asked for an administrator password. |
| Not persistent (see 11.2) | 重新開機後會回到 100%，Better OS 會在開機時重新套用。 | This resets to 100% on restart; Better OS sets it again at startup. |

**Battery** — a `SettingsExpander`, read only.

| Row | zh-TW | English | Source |
| --- | --- | --- | --- |
| Header | 電池 | Battery | |
| Description | {capacity}% · {status} | {capacity}% · {status} | `capacity`, `status` |
| Health | 電池健康度：目前可充到出廠容量的 {percent}% | Battery health: holds {percent}% of its original capacity | `charge_full / charge_full_design` (or `energy_*`) |
| Cycles | 充電次數：{n} | Charge cycles: {n} | `cycle_count`, row hidden when it reads 0 (the machine reads 0) |
| Model | 型號：{model} | Model: {model} | `model_name` |

Status words: `Charging` 充電中 / `Discharging` 使用電池 / `Full` 已充滿 /
`Not charging` 已接電源，沒有充電 / anything else 無法判讀 (Unknown).

### Group: 顯示 / Display

**Brightness** — `Slider` 0–100 with the value at its trailing edge.

| | zh-TW | English |
| --- | --- | --- |
| Header | 螢幕亮度 | Brightness |
| Description | 調整內建螢幕的亮度。 | Change the brightness of the built-in display. |

Changes are sent while dragging, at most every 50 ms, and the slider follows
`PropertiesChanged` so a brightness key moves it.

**Keyboard backlight** — `Slider` with one stop per level.

| | zh-TW | English |
| --- | --- | --- |
| Header | 鍵盤背光 | Keyboard backlight |
| Description | {levels} 段亮度，0 是關閉。 | {levels} levels; 0 is off. |
| Value | 關閉 / 第 {n} 段 | Off / Level {n} |

### Group: 散熱 / Cooling

**Fan mode** — `RadioButtons`, horizontal, privileged.

| | zh-TW | English |
| --- | --- | --- |
| Header | 風扇模式 | Fan mode |
| Description | 自動會依溫度調整。全速只適合短時間散熱，會比較吵。 | Automatic follows the temperature. Full speed is for short bursts and is loud. |
| auto | 自動 | Automatic |
| full | 全速 | Full speed |
| After restart note | 重新開機後一律回到自動。 | It always goes back to Automatic when the PC restarts. |

**Temperatures and fans** — a `SettingsExpander`, read only, refreshed every
2 seconds only while the page is visible.

| | zh-TW | English |
| --- | --- | --- |
| Header | 溫度與風扇 | Temperatures and fans |
| Description | {hottest label} {°C} · 風扇 {rpm} | {hottest label} {°C} · Fan {rpm} |
| Row | {label}　{value} | {label}　{value} |

Labels (the kernel names on the left are read on the machine):

| hwmon name / label | zh-TW | English |
| --- | --- | --- |
| `k10temp` / `Tctl`, `coretemp` / `Package id 0` | 處理器 | Processor |
| `amdgpu` / `edge` | 顯示晶片 | Graphics |
| `nvme` / `Composite` | 固態硬碟 | SSD |
| `acpitz` | 主機板 | Motherboard |
| a wireless driver (`mt7921_phy0`, `iwlwifi_1`) | 無線網路 | Wi-Fi |
| `asus` / `cpu_fan`, any `fanN_label` | 處理器風扇 | CPU fan |
| anything else | the hwmon name, unchanged | the hwmon name, unchanged |

A fan reading appears once per physical fan: when a platform driver reports a
labelled fan (`asus` `cpu_fan`), the unlabelled `acpi_fan` reading of the same
machine is not shown (both read about 4,800 RPM on the machine; they are the
same fan). Fan curves are not offered: the machine exposes no `pwmN` or
`pwmN_auto_point*` file, only `pwm1_enable`.

### Group: 關於這台電腦 / About this PC

One `SettingsExpander` that keeps today's About facts: product name, the
distribution line (`Zorin OS 18 · Ubuntu 24.04 基礎` stays in today's words),
architecture, kernel version, desktop and session (`GNOME 46 · Wayland`).
Each row has a Copy icon button. Header `裝置規格` / `Device specifications`.

## 5. Touchpad

All of today's Better Touchpad, as one page with a selector bar instead of
seven sidebar entries. The pages are the same and keep every behaviour,
including the capture-before-first-change and per-section restore.

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 觸控板 | Touchpad |
| Selector bar | 游標 · 捲動 · 點按 · 手勢 · 裝置 · 診斷 | Pointer · Scrolling · Clicking · Gestures · Devices · Diagnostics |
| No touchpad | 這台電腦沒有找到觸控板。 | No touchpad was found on this PC. |
| Safe mode on (`InfoBar` Warning, action) | 安全模式已開啟。只會顯示設定，不會做任何變更。 / 關閉安全模式 | Safe mode is on. Settings are shown but nothing is changed. / Turn off safe mode |
| Not GNOME (`InfoBar` Informational) | 這些設定目前只支援 GNOME 桌面。 | These settings currently work only on the GNOME desktop. |

Today's Overview page is gone as a page: its facts become the first card
(`SettingsExpander` "這個觸控板" / "This touchpad": selected device, session,
backend, health in words) and its pending-changes line becomes the Apply bar
below.

**Apply model stays.** Today a change is staged and then applied with
`Apply`, verified by reading back, and each section restores on its own. That
stays, because Better Touchpad's acceptance (ticket 29) depends on the
capture taken before the first write and on verifying each value. A sticky
bar appears at the bottom of the content only while something is staged:

| | zh-TW | English |
| --- | --- | --- |
| Bar text | 有 {n} 項變更還沒套用。 | {n} changes haven't been applied. |
| Primary | 套用 | Apply |
| Secondary | 捨棄變更 | Discard |

Copy for the controls is today's `touchpad-gui` zh-TW and en-US tables
(`crates/touchpad-gui/src/i18n.rs`), kept word for word except where the audit
found a fault:

| Key | Today (zh-TW) | Now (zh-TW / en) | Why |
| --- | --- | --- | --- |
| `requested_value` / `effective_value` | 要求的值 / 實際的值 | 你選的 / 目前生效 · You chose / In effect | audit T3 |
| `tap_to_click` | 點按來按一下 | 輕點即按一下 · Tap to click | reads as a sentence |
| `pending_badge` | 待套用 | 尚未套用 · Not applied yet | consistent with the bar |
| health words | English `Degraded` in zh-TW | 部分功能無法使用 · Partly unavailable | audit T4 |
| device identity | shown on Overview | only under Devices › 詳細資料 · Details | audit T4 |
| `switch_language` button | in the page header | removed: language is in Settings | one language setting |
| mock notice | `mock · Not applied yet` on screen | never shown outside tests | audit T1 |

Each unavailable control is drawn disabled with its reason as the card
description (unchanged behaviour), never as a working switch.

Gestures sub-page keeps: the Mac-style preset with preview, conflict
resolution per gesture (keep GNOME's / use Better OS's / move mine to five
fingers), per-gesture enable, edit (contacts, thumb, shape, direction,
action, thresholds, cooldown, custom keyboard shortcut with conflict check),
replay test with optional live testing, per-device or shared profile, export
and import with preview, restore recorded gestures, turn gestures off. The
export and import path fields become a standard `Button` that opens the
desktop's file chooser portal; until ticket 88 lands that portal call, the
text field stays.

Diagnostics sub-page keeps: effective values table (`DetailsTable`), the
first-captured values, Restore section, Restore all, safe mode on and off.

## 6. Keep awake

All of today's Better Awake window, plus the two things its tray promises
that the window cannot do today (section 12, rows marked ✱).

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 保持清醒 | Keep awake |
| Selector bar | 狀態 · 規則 · 紀錄 · 診斷 | Status · Rules · History · Diagnostics |

### Status (the default sub-page)

Top card: a `SettingsCard` with a large state line.

| State | Header | Description | Control |
| --- | --- | --- | --- |
| Nothing holds the PC | 沒有任何東西在保持這台電腦清醒 / Nothing is keeping this PC awake | 電腦依照自己的電源設定運作。 / The PC follows its own power settings. | `SplitButton` accent: 保持清醒 / Keep awake, menu: the quick lengths, 直到指定時間… / Until a time… |
| A manual session | 這台電腦正被保持清醒 / This PC is being kept awake | 由你開始，剩餘 {time} / Started by you, {time} left (or 直到你結束 / until you end it) | standard 延長 ▾ / Extend ▾ (15, 30, 60 min), standard 結束 / End |
| Rule sessions only | 這台電腦正被保持清醒 / This PC is being kept awake | 由規則「{rule}」開始 / Started by the rule "{rule}" | 暫停規則 ▾ / Pause rules ▾ |
| Rules paused | 自動規則已暫停到 {time} / Automatic rules are paused until {time} | (or 直到手動恢復 / until you resume them) | 恢復規則 / Resume rules |
| All rules overridden | 所有自動規則都被覆寫停用 / All automatic rules are overridden | | 恢復規則 / Resume rules |
| Attention required | 有工作階段需要處理 / A session needs attention | the reason | 查看診斷 / See diagnostics |

**Until a time…** opens a `ContentDialog` with a time picker (new kit
control, section 13): title 保持清醒到… / Keep awake until…, primary 開始 /
Start, secondary 取消 / Cancel. A time earlier than now means tomorrow, and
the dialog says so in Caption: 明天 {time} / Tomorrow at {time}.

**Group: 目前的原因 / Active reasons** — one card per reason, with the
origin, start time, remaining time, and an overflow menu (`…`): 延長 15 分鐘,
延長 30 分鐘, 延長 1 小時, 修改…, 結束這個原因 / Extend 15 minutes, Extend 30
minutes, Extend 1 hour, Modify…, End this reason. "End" first says what remains
(today's `ending_leaves` sentence) in the card's second line, not in a dialog.
This replaces today's five buttons per row (audit A4).

**Group: 實際生效的原則 / What's in effect** — four rows: 系統睡眠, 閒置處理,
螢幕休眠, 自動鎖定 / System sleep, Idle handling, Display sleep, Automatic lock,
each 已擋下 / Prevented, 維持原本行為 / Unchanged, or 有要求，但後端無法提供 /
Requested, but the backend can't do it (caution colour). Then 衝突是怎麼決定的
/ How conflicts were decided, collapsed.

**Group: 快速工作階段 / Quick sessions** — the lengths the tray and the split
button offer.

| | zh-TW | English |
| --- | --- | --- |
| Expander header | 快速工作階段長度 | Quick session lengths |
| Description | 系統匣選單和「保持清醒」按鈕提供的長度。 | The lengths the tray menu and the Keep awake button offer. |
| Row | {length}　[設為預設] [↑] [↓] [移除] | {length}　[Make default] [↑] [↓] [Remove] |
| Add | 新增長度 | Add a length |
| Reset | 回復預設值 | Restore defaults |

These lengths are stored by the Better Awake service (ticket 87) so the tray
reads the same list. Today they live in the window's own preferences file and
the tray never reads them (section 12, ✱).

**Group: 新工作階段的預設值 / Defaults for new sessions** — four
`ToggleSwitch` cards: 擋下系統睡眠, 擋下閒置處理, 擋下螢幕休眠, 擋下自動鎖定 /
Prevent system sleep, Prevent idle handling, Prevent display sleep, Prevent
automatic lock. The last two show today's `reduced_security_warning` as a
caution line when on. Then the battery stop:

| | zh-TW | English |
| --- | --- | --- |
| Header | 電量低於這個值就停止 | Stop below this battery level |
| Control | `ToggleSwitch` 「依電量停止」 plus a `NumberBox` 1–99 % (today's stepper range), the box disabled while the switch is off | `ToggleSwitch` "Stop on battery level" plus a `NumberBox` 1–99 %, disabled while the switch is off |
| No battery | card not drawn (today's `no_battery` explanation is dropped with it) | — |

**Group: 電池與安全 / Battery and safety** — today's Battery & Safety page:
current charge and source in one line, the on-battery rule sentence, and the
quit warning as an `InfoBar` Informational.

### Rules

| Element | zh-TW | English |
| --- | --- | --- |
| Command bar | 新增規則 · 暫停 15 分鐘 · 暫停 1 小時 · 覆寫所有規則 | New rule · Pause 15 minutes · Pause 1 hour · Override all rules |
| Summary line | {total} 條規則中有 {enabled} 條啟用 | {enabled} of {total} rules on |
| Empty | 還沒有自動規則。規則會盯著某個條件，條件成立時開始工作階段。 | No automatic rules yet. A rule watches a condition and starts a session when it's true. |

Each rule is a `SettingsExpander`: header = name, description = its
conditions in one sentence plus 目前符合 / Matching now or 目前不符合 / Not
matching now, a `ToggleSwitch` (enable) at the trailing edge (audit A5). The
expanded part has priority (`NumberBox` 0–255), Test with its result, and an
overflow menu: 編輯, 複製, 上移, 下移, 刪除 / Edit, Duplicate, Move up, Move
down, Delete. Delete asks in a `ContentDialog`: 刪除「{rule}」？ / Delete
"{rule}"? with 刪除 / Delete and 取消 / Cancel.

The rule editor opens as a full sub-page (breadcrumb `保持清醒 › 編輯規則`),
keeping today's editor: name, groups combined by AND or OR, conditions of the
eleven providers with their operands, weekdays and times for a schedule (now
with the time picker instead of ± buttons), Save and Cancel. An unavailable
provider is listed disabled with its reason.

### History

A `DetailsTable`: 開始時間, 結束時間, 來源, 原因, 停止原因 / Started, Ended,
Origin, Reason, Stopped because. Sortable by start and end. The retention note
in Caption below. Empty: 還沒有工作階段紀錄。 / No sessions yet.

### Diagnostics

Today's diagnostics page as settings cards: inhibitor backend and adapter,
what the backend can prevent (four rows), each condition provider with its
availability, polling cadence, and reason (`DetailsTable`), the tray host
line, protocol version, and the two verified statements. Copy unchanged.

## 7. Apps

Today's Better Manager pages, as one page with sub-pages.

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 應用程式 | Apps |
| Card → sub-page | 更新 · 有 {n} 個更新 / 已是最新 | Updates · {n} updates / Up to date |
| Card → sub-page | 系統狀態 · 檢查元件服務、相依套件與復原選項 | Health · Check services, dependencies, and recovery options |
| Card → sub-page | 活動紀錄 · 安裝、更新與檢查的結果 | Activity · Results of installs, updates, and checks |
| Group header | Better OS 元件 | Better OS components |
| Filter (`SelectorBar`) | 全部 · 已安裝 · 可更新 · 已停用 · 需要處理 | All · Installed · Updates · Turned off · Needs attention |
| Search placeholder | 搜尋元件 | Search components |
| Catalog line (Caption) | {source} · {age} · [更新清單] | {source} · {age} · [Refresh list] |

Each component is a `SettingsCard` with its icon, display name (Body Strong),
summary (description, in the current language, from the shipped translation
for a first-party component — audit M3), and at its trailing edge exactly one
thing: the state word in secondary text when nothing is to be done
(`已安裝 0.2.9` / `Installed 0.2.9`), or one standard button for the one
action (`安裝` / `Install`, `更新` / `Update`, `查看` / `View` for a fault),
then a chevron to its details. Kind, health pills, and restart scope move to
the details page (audit M2).

**Component details** (breadcrumb `應用程式 › {component}`): a
`SelectorBar` 總覽 · 版本 / Overview · Versions (audit M8), the drift card,
last-failure card, unorderable-version explanation, what it does, replaces and
enhances, restart requirement, installed and available version, files
touched, health checks, release notes, and the action row: 安裝 / 更新 / 停用
/ 啟用 / 重新檢查 / 還原上一個版本 / 重新安裝 / 移除 / 採用系統版本, each
present only when the planning API offers it. Better Manager's own remove is
replaced by today's `cannot_remove_self` sentence.

**Updates** keeps the review → installing → finished / restore → restored
flow and all of today's copy (`review_changes`, `installing_title`, the
stage names, `restore_title`, …), drawn with `ProgressBar`, settings cards,
and one accent button per screen. The accent button only exists when there is
something to install (audit M4). The self-update notice stays.

**Health** keeps the doctor (run the full check, results list), the
interrupted-package card with Repair, view recovery, and all evidence
sentences.

**Activity** keeps the filters (All, Successful, Warnings, Failures), the
list, and Clear history (now behind a `ContentDialog`: 清除所有活動紀錄？ /
Clear all activity? · 清除 / Clear · 取消 / Cancel).

**First run.** Today's welcome page said the same thing four times and called
the host check "simulated" (audit M6). The control center has no separate
first-run page: Home shows one `InfoBar` Informational the first time,
`歡迎使用 Better OS。這裡可以安裝與更新 Better OS 元件，以及調整這台電腦。` /
`Welcome to Better OS. Install and update Better OS components and adjust
this PC here.` with a close button, and the compatibility facts move to
System › About this PC. `onboarding_complete` is set when it is closed.

**Demo mode.** `BETTER_MANAGER_DEMO` keeps today's banner text
(`demo_mode_banner`) as an `InfoBar` Warning on every page.

## 8. Default apps

Today's Defaults page, unchanged in behaviour (review before any write,
capture before the change, verify after, restore with the captured value).

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 預設應用程式 | Default apps |
| Description under title | 讓 Better OS 元件成為開啟檔案、連結與功能的預設程式，並可隨時恢復。 | Make Better OS components the default for files, links, and features, and put things back any time. |
| Summary line | {are_default} 個已是預設 · {can_change} 個可以變更 · {changed} 個被其他程式變更 | {are_default} are defaults · {can_change} can change · {changed} changed elsewhere |
| Command bar | 套用 Better OS 預設值 · 恢復先前的預設值 · 重新檢查 | Use Better OS defaults · Restore previous defaults · Check again |

One `SettingsExpander` per component: header the component, description its
aggregate state (today's eight state words), expanded rows one per
integration (current owner → Better OS target, last verified, saved previous
value). Per-component actions in its overflow menu: 設為預設, 恢復, 重新檢查 /
Make default, Restore, Check again. The review screen is a sub-page with a
`CheckBox` per entry, the overwrite confirmation per entry, the missing-app
alternatives, and the bottom summary; the results screen follows it. All of
today's Defaults copy is kept.

## 9. Fixes

Better Patch, as a page. It lists known faults this PC has, says what is
wrong in plain words, fixes it, checks the fix, and undoes it (ADR 0017
decides it lives here and how entries are written).

| Element | zh-TW | English |
| --- | --- | --- |
| Title | 修正 | Fixes |
| Description | Better OS 會檢查這台電腦有沒有已知的問題。每一項修正都可以復原。 | Better OS checks this PC for known problems. Every fix can be undone. |
| Command bar | 重新檢查 | Check again |
| Last checked (Caption) | 上次檢查：{time} | Last checked: {time} |
| Nothing found | 沒有找到已知的問題。 | No known problems found. |
| Group: found | 找到的問題 | Problems found |
| Group: applied | 已套用的修正 | Fixes applied |
| Group: all | 所有已知問題 | All known problems |

Each fault is a `SettingsExpander`: header the symptom, description the cause,
expanded the exact change it makes, with 修正 / Fix (accent) or 復原 / Undo.
After Fix the page runs the entry's check and says 已修正，檢查通過。 / Fixed
and checked. or 已套用，但問題還在。 / Applied, but the problem is still
there. with Undo offered.

The two entries ticket 76 records, each reproduced and fixed on the owner's
machine:

| Entry | Header (zh-TW / en) | Description (zh-TW / en) | Change |
| --- | --- | --- | --- |
| fontconfig cache | 有些應用程式的文字是空白的 / Text is blank in some apps | 字型快取損壞了，每次查字型都拿到同一個檔案。 / The font cache is damaged and answers every font query with the same file. | rebuild the user's font cache; undo is not needed and the card says 這項修正不需要復原。 / This fix doesn't need undoing. |
| Electron cursor | {app} 的游標只有一半大 / The pointer is half size over {app} | 這個應用程式在 Wayland 下不會放大游標，改用 XWayland 執行就正常。 / This app doesn't scale its pointer on Wayland; running it through XWayland fixes it. | a per-user Flatpak override for that app; undo removes exactly those keys |

## 10. Settings

| Card | Control | zh-TW | English |
| --- | --- | --- | --- |
| Appearance | `ComboBox` | 外觀 · 深色 / 淺色 / 跟隨系統 · Better OS 預設為深色。 | Appearance · Dark / Light / Use system setting · Better OS starts dark. |
| Language | `ComboBox` | 語言 · 跟隨系統 / English / 繁體中文 · 立即生效。 | Language · Use system setting / English / 繁體中文 · Takes effect right away. |
| Check for updates when opened | `ToggleSwitch` | 開啟時檢查更新 | Check for updates when opened |
| Update channel | read-only value | 更新頻道 · 穩定版 · 目前只有穩定版。 | Update channel · Stable · Stable is the only channel right now. |
| About (expander) | rows | 關於 · Better Manager {version} · 專案原始碼 · 第三方授權 | About · Better Manager {version} · Source code · Third-party licenses |
| Privacy (description line under About) | — | 只在本機保存狀態與紀錄，不會上傳任何資料。 | State and history stay on this PC. Nothing is uploaded. |

Appearance uses `theme::Appearance` (dark, light, system, followed live).
One language setting and one appearance setting now cover every page that used
to be its own window. The language names are written the same way on every
page (`繁體中文`, audit A7).

Dropped, because nothing reads them (checked with `grep` on 2026-11-12: each
field is written by `manager-gui/src/app.rs` and read by no other code):
`auto_download` (自動下載更新), `diagnostic_logs` (保留失敗操作的診斷紀錄),
and the Preview choice of `release_channel`. `check_updates` was also unread
— `refresh_catalog_at_launch` (`app.rs:363`) always refreshes — and ticket 85
makes it true rather than dropping it. The development note at the bottom of
today's Settings (`mock_lifecycle_detail`, audit M7) is not carried.

## 11. Hardware: detection and writing

### 11.1 What each control reads and writes

| Control | Present when | Read from | Write through | Privilege |
| --- | --- | --- | --- | --- |
| Power mode | `org.freedesktop.UPower.PowerProfiles` (or the older `net.hadess.PowerProfiles`) answers on the system bus | `ActiveProfile`, `Profiles`, `PerformanceDegraded`, `ActiveProfileHolds` | set `ActiveProfile` | none: polkit `org.freedesktop.UPower.PowerProfiles.switch-profile` is `allow_active=yes` on the machine |
| Battery charge limit | a `power_supply` of `type` `Battery` and `scope` not `Device` has `charge_control_end_threshold` | that file (mode 0644, readable) | `manager-daemon` `org.betteros.Hardware1.SetChargeLimit(y)` | root, polkit `org.betteros.manager.set-charge-limit`, `auth_admin_keep` |
| Battery facts | same battery | `capacity`, `status`, `charge_full`, `charge_full_design` (or `energy_*`), `cycle_count`, `model_name` | — | none |
| Brightness | `org.gnome.SettingsDaemon.Power` on the session bus exposes `org.gnome.SettingsDaemon.Power.Screen` | its `Brightness` (0–100) | set `Brightness` | none (the desktop's own path) |
| Keyboard backlight | `org.gnome.SettingsDaemon.Power.Keyboard` (GNOME) or `org.freedesktop.UPower` `/org/freedesktop/UPower/KbdBacklight` | `Brightness` and `Steps`, or `GetBrightness` and `GetMaxBrightness` | set `Brightness`, or `SetBrightness(i)` | none |
| Fan mode | a hwmon directory has `pwm1_enable` and no writable `pwm1` | `pwm1_enable` | `manager-daemon` `org.betteros.Hardware1.SetFanMode(s)` with `auto` or `full` | root, polkit `org.betteros.manager.set-fan-mode`, `auth_admin_keep` |
| Temperatures, fan speeds | any hwmon `tempN_input`, `fanN_input` | those files | — | none |

On the machine, read on 2026-11-12: power-profiles-daemon 0.21 owns both bus
names and lists `power-saver`, `balanced`, `performance` with
`PlatformDriver` `platform_profile`; `/sys/firmware/acpi/platform_profile`
offers `quiet balanced performance` and is driven by that daemon, so writing
it directly would fight the daemon. `BAT1/charge_control_end_threshold`
reads `100`, is `root:root 0644`, and there is no start threshold. UPower
1.90.3 has no charge-threshold property on the battery. `asus::kbd_backlight`
has `max_brightness` 3 and is `root:root 0644`, but gsd-power exposes
`Power.Keyboard` with `Brightness` read-write and `Steps` 4, and UPower exposes
`KbdBacklight`; so the keyboard backlight needs no root, which corrects the
ticket's "(root)". `amdgpu_bl1` is the display backlight, `root:root 0644`,
written by GNOME through gsd-power's `Power.Screen.Brightness`. hwmon `asus`
(device `asus-nb-wmi`) has `fan1_input`, `fan1_label` `cpu_fan`, and
`pwm1_enable` reading `2`, with no `pwm1`.

Not yet verified, each owned by ticket 86: that `pwm1_enable` accepts `0`
for full speed and `2` for automatic on this driver (recalled from the
`asus-wmi` driver, not read from its source here); whether
`charge_control_end_threshold` survives a reboot on this machine; and
whether UPower's `KbdBacklight.SetBrightness` is unrestricted or polkit-gated
on Ubuntu 24.04.

### 11.2 The daemon side

The daemon gains a second interface, `org.betteros.Hardware1`, on its existing
bus name and object path, with one method per control and one polkit action
per method. Nothing crosses as free text or a path:

- `SetChargeLimit(percent: y)` — accepts 60, 80, or 100; the daemon finds the
  battery itself, writes the file, reads it back, and returns the value read.
- `SetFanMode(mode: s)` — accepts exactly `auto` or `full`; the daemon finds
  the hwmon directory whose `pwm1_enable` exists under a platform device it
  recognises (`asus-nb-wmi` first), writes, reads back, and returns it.
- No read method: reading is unprivileged and the window reads sysfs itself
  (AGENTS.md allows read-only host queries).

The unit keeps `ProtectKernelTunables=yes`, which makes `/sys` read-only, and
adds `ReadWritePaths=` for the two attribute files' real paths under
`/sys/devices` at the narrowest directory that holds them. Whether systemd's
more specific mount wins over `ProtectKernelTunables` is to be shown in a VM
by ticket 86 before the unit change merges; if it does not, the ticket stops
and asks rather than dropping the protection.

If the charge limit does not survive a reboot, the daemon stores the chosen
value in `/var/lib/better-os/hardware.json` and a oneshot system unit,
`better-hardware-restore.service`, enabled at boot, runs
`/usr/libexec/better-manager-daemon --restore-hardware`, which writes the
stored limit and nothing else. Fan mode is never restored: it always starts on
automatic.

## 12. Where every function goes

Every function of the four windows and of the tray's window entry points,
with its new place. "Same" means behaviour and copy are kept.

### Better Manager (`crates/manager-gui`)

| Today | New place | Note |
| --- | --- | --- |
| Sidebar: Overview | Home | metric cards become attention cards |
| Overview: system state, installed / updates / healthy counts | Home attention cards; Apps card subtitles | |
| Overview: component summary, View | Apps | |
| Overview: recent activity, View all | Home › Recent activity → Apps › Activity | |
| Sidebar: Components; search; filters (All, Installed, Updates, Disabled, Needs attention) | Apps; Apps search; Apps `SelectorBar` | |
| Catalog status row, Refresh list, degraded and rejected sentences | Apps catalog line; Home attention card | same copy |
| Check for updates, result line | Apps › Updates card and Updates sub-page | |
| Component row actions: Install, Update, Enable, View recovery, Details, More (Details, Disable, Enable, Retry check, Restore previous, Remove) | Apps card's one action + details page action row | audit M2 |
| Component details: Overview / Versions tabs, what it does, replaces, enhances, restart, versions, installed outside manager, files touched, health checks, release notes, no-privileged-actions note | Apps › {component} | same |
| Drift card, Adopt host state with confirmation | Apps › {component} | same |
| Last install failed, Try install again | Apps › {component} | same |
| Unorderable version explanation | Apps › {component}, Health results | same |
| Cannot remove self | Apps › Better Manager | same |
| Self-update notice | Apps › Updates review | same |
| Sidebar: Updates (count), Update all, Review changes | Apps › Updates; nav badge | |
| Review changes: affected components, download size, disk space, restore available, dependencies, files touched, before / after | Apps › Updates › Review | same |
| Installing: stages, progress, details, Cancel installation | Apps › Updates › Installing | same |
| Finished, Back to overview | Apps › Updates › Done → Home | |
| Restore page, Restore previous version, Retry check, Restored page | Apps › Updates › Restore | same |
| Sidebar: System health; Run full check; doctor results; Run again; Back | Apps › Health | |
| Interrupted package system card, Repair | Apps › Health; Home attention card | same |
| Sidebar: Activity; filters; Clear history | Apps › Activity | Clear now confirms |
| Sidebar: Settings; Language | Settings › Language | |
| Appearance | Settings › Appearance | gains "Use system setting" |
| Update channel (Stable / Preview) | Settings › Update channel, read only | Preview never did anything |
| Check for updates when opened | Settings | made true by ticket 85 |
| Download updates automatically | dropped | nothing reads it |
| Keep diagnostic logs | dropped | nothing reads it |
| Privacy note | Settings › About description | |
| About: version, this computer, source code | Settings › About; System › About this PC | |
| Sidebar header: brand and version | Settings › About | |
| Sidebar footer: host line, architecture | System › About this PC | |
| First-run: welcome, check system, load catalog, compatibility facts, Continue | Home first-run info bar; System › About this PC | audit M6 |
| Unsupported host and storage error banners | `ErrorBar` on every page | same copy |
| Demo mode banner | `InfoBar` on every page | same copy |
| Sidebar: Defaults (all of it) | Default apps | see below |
| `--help`, `--version`, refusing other arguments | same, plus `--page` | |

### Better Defaults (the Defaults pages inside `manager-gui`)

| Today | New place |
| --- | --- |
| Summary (are default, can change, changed externally) | Default apps summary line; Home attention card for "changed elsewhere" |
| Use Better OS defaults, Restore previous defaults, Verify again (all) | Default apps command bar |
| Per component: state, details, make default, restore, verify | Default apps expander and its overflow menu |
| Per integration: current owner, target, last verified, saved value, kind, session effect | Default apps expander rows |
| Empty and read-failed states | Default apps, same copy |
| Review: per-component checkboxes, overwrite confirmations, missing-application alternatives (restore instead, clear default), snapshot line, elevated-excluded line, damaged snapshots, nothing to change | Default apps › Review |
| Results: per-entry outcome, partial | Default apps › Results |
| Link from a component's details to its defaults | Apps › {component} card "預設應用程式" → Default apps, filtered |

### Better Awake (`crates/awake-gui`, and the tray's window entry points)

| Today | New place | Note |
| --- | --- | --- |
| Status: summary, effective policy, active reasons, end, extend by 15/30/60, modify, ending-leaves sentence, inhibitor health, interrupted previous session, conflicts | Keep awake › Status | per-reason overflow menu |
| Quick sessions: presets list, start, set default, move up/down, remove, add, restore defaults, default policy choice | Keep awake › Status › Quick sessions | ✱ stored by the service so the tray uses them |
| Session defaults: four policy toggles, security warning, default battery threshold | Keep awake › Status › Defaults for new sessions | |
| Battery & safety: current charge, AC or battery, on-battery rule, quit warning, no-battery state | Keep awake › Status › Battery and safety | |
| Rules: list, enable, edit, duplicate, delete, move up/down, priority, test and result, pause 15 min / 1 h, resume, override all, refused-rules line, paused and overridden states | Keep awake › Rules | toggle instead of text button |
| Rule editor: name, groups AND/OR, add/remove group and condition, eleven providers, operands, schedule days and times, unavailable providers, save, cancel, invalid input | Keep awake › Rules › Edit rule | time picker for schedules |
| History: list, origin, reasons, policy, stop cause, retention note, showing N of M | Keep awake › History | |
| Diagnostics: backend, adapter, capabilities, providers table, verification statements, protocol version, tray host unavailable | Keep awake › Diagnostics | |
| Settings: language, appearance, storage-failed note | Settings | one setting for all pages |
| Service unreachable error (D-Bus string) | "service not running" state with Start service | audit A2, A3 |
| Ctrl+1 … Ctrl+8 section shortcuts | Ctrl+1 … Ctrl+4 for the four sub-pages | audit A8 writes them `Ctrl+1` |
| Tray: Open Better Awake… | `better-manager --page keep-awake` | |
| Tray: Until a time… | Keep awake › Status, time dialog opened | ✱ the window has no until-time start today |
| Tray: Change session… | Keep awake › Status, Modify | |
| Tray: everything else (start, extend, end, pause, resume, override, quick options, quit) | stays in the tray | |

### Better Touchpad (`crates/touchpad-gui`)

| Today | New place |
| --- | --- |
| Overview: selected touchpad, session, backend, health, pointer and scroll summary, pending line | Touchpad › "This touchpad" card and Apply bar |
| Pointer: sensitivity, acceleration profile, disable while typing, test area | Touchpad › Pointer |
| Scrolling: vertical and horizontal sensitivity, link axes, natural, two-finger, smooth, test area | Touchpad › Scrolling |
| Clicking: tap to click, tap and drag, drag lock, click method, middle-click emulation | Touchpad › Clicking |
| Gestures: everything listed in section 5 | Touchpad › Gestures |
| Devices: identity, capabilities, contacts, buttonpad, scope, connection state | Touchpad › Devices |
| Diagnostics: effective values, captured values, restore section, restore all, safe mode on/off | Touchpad › Diagnostics |
| Top bar: switch language, refresh, discard, apply | language → Settings; refresh → command bar; discard and apply → Apply bar |
| `--page` | `better-manager --page touchpad` (sub-page not addressable; see Notes in ticket 88) |
| `--lang` | dropped: the control center has one language setting |
| `--offline` | kept only for tests inside the page module |
| `--safe-mode`, `--normal-mode` | kept, in the `better-touchpad` command that stays in the package (ADR 0017) |

## 13. Gaps in the Fluent kit this design needs

Each is added to `better_ui::fluent` with tests by the ticket named, never
drawn inside the window.

| Gap | Needed by | Ticket |
| --- | --- | --- |
| NavigationView: a search box slot above the items with a suggestion list, and compact behaviour for it | the pane | 84 |
| NavigationView: an info badge (count or dot) on an item | Apps, Keep awake, Fixes | 84 |
| Titlebar back button with history (WinUI `NavigationView` back button, which the kit cut) | sub-pages | 84 |
| A sticky bottom bar for staged changes (a content-area footer, not a dialog) | Touchpad Apply bar | 88 |
| Time picker (WinUI `TimePicker`: hour and minute columns, 24-hour in zh-TW and en) | Until a time, schedule conditions | 87 |
| Icons with no Lucide equivalent in gpui-component's set: house, touchpad | nav | not fixed; `LayoutDashboard` and `Frame` are used, owner to decide |

## 14. Faults found while reading, carried into the tickets

| Fault | Where | Ticket |
| --- | --- | --- |
| The Quick sessions list says it sets the tray's menu, but the tray hard-codes 15, 30, 60, 120 minutes and never reads the window's preferences file | `awake-gui/src/settings.rs`, `awake-tray/src/menu.rs:330-333` | 87 |
| The tray's "Until a time…" opens a window that has no way to start a session that ends at a time | `awake-tray/src/controller.rs:254-258`; no `UntilUnixSeconds` in `awake-gui` | 87 |
| Four Settings choices are stored and never read | `manager-core/src/lib.rs:129-149`, `manager-gui/src/app.rs:969-989` | 85 |
| The awake manifest promises a choice to delete rules and history at uninstall; no code offers it | `components/manifests/better-awake.yaml` release notes | not in scope; listed in ticket 91's Notes |

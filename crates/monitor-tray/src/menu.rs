//! What the taskbar item knows and says: the readings, the person's choice
//! of what the icon shows, the words in both languages, and the right-click
//! menu built from them. None of it needs a bus, so all of it is tested here.

use std::path::{Path, PathBuf};

use hardware_core::Presence;
use hardware_core::power::{ModeShown, PowerProfilesFacts, Profile, mode_shown, unreadable_words};
use monitor_core::{CollectorReport, EntityKind, MetricId, MetricScalar, Observation};

/// What power-profiles-daemon says, as `hardware_platform::bus::power_profiles`
/// reads it for the System page: absent on a machine without it.
pub type PowerFacts = Presence<PowerProfilesFacts>;

/// A reading the icon can show.
///
/// The battery is not one (owner, 2026-11-22, ticket 295): the panel already
/// has the desktop's own battery icon, and a second beside it is a
/// duplicate. The battery stays in the menu and the tooltip as words
/// ([`battery_line`]).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Reading {
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Temperature,
}

impl Reading {
    pub const ALL: [Reading; 6] = [
        Reading::Cpu,
        Reading::Memory,
        Reading::Gpu,
        Reading::Disk,
        Reading::Network,
        Reading::Temperature,
    ];

    /// The word stored in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Reading::Cpu => "cpu",
            Reading::Memory => "memory",
            Reading::Gpu => "gpu",
            Reading::Disk => "disk",
            Reading::Network => "network",
            Reading::Temperature => "temperature",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reading| reading.key() == key.trim())
    }
}

/// What the battery icon rule reads from the battery collector's round: the
/// battery's status, charge and limit, and whether any power source is
/// online.
fn charge_readings(
    report: &monitor_core::CollectorReport,
) -> hardware_core::charge_state::ChargeReadings {
    use hardware_core::charge_state::{BatteryPart, ChargeLimit, ChargeReadings, charger_online};
    let percent = |ratio: f64| (ratio * 100.0).round().clamp(0.0, 100.0) as u8;
    let sources: Vec<(String, Option<bool>)> = report
        .entities_of(EntityKind::PowerSource)
        .map(|source| {
            let kind = source
                .metrics
                .get(&id("power_source.kind"))
                .and_then(Observation::as_text)
                .unwrap_or("")
                .to_string();
            let online = match source.metrics.get(&id("power_source.online")) {
                Some(Observation::Value(MetricScalar::Bool(online))) => Some(*online),
                _ => None,
            };
            (kind, online)
        })
        .collect();
    ChargeReadings {
        charger_online: charger_online(sources.iter().map(|(k, o)| (k.as_str(), *o))),
        batteries: report
            .entities_of(EntityKind::Battery)
            .filter(|e| !is_peripheral(e))
            .map(|battery| {
                let m = &battery.metrics;
                BatteryPart {
                    status: m
                        .get(&id("battery.status"))
                        .and_then(Observation::as_text)
                        .and_then(hardware_core::battery::BatteryStatus::from_key),
                    percent: number(m, "battery.charge").map(percent),
                    energy_now: number(m, "battery.energy.now"),
                    energy_full: number(m, "battery.energy.full"),
                    limit: match (
                        m.get(&id("battery.charge.limit")),
                        number(m, "battery.charge.limit"),
                    ) {
                        (_, Some(end)) => ChargeLimit::Percent {
                            end: percent(end),
                            start: number(m, "battery.charge.start").map(percent),
                        },
                        (Some(Observation::Unsupported(_)), None) => ChargeLimit::Unreadable,
                        _ => ChargeLimit::NotSupported,
                    },
                }
            })
            .collect(),
    }
}

/// A mouse's or a keyboard's battery rather than this machine's.
fn is_peripheral(entity: &monitor_core::Entity) -> bool {
    matches!(
        entity.metrics.get(&id("battery.peripheral")),
        Some(Observation::Value(MetricScalar::Bool(true)))
    )
}

/// The handful of facts the item shows, read from one round.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrayReadings {
    /// Fractions `0.0..=1.0`.
    pub cpu: Option<f64>,
    pub cpu_hz: Option<f64>,
    pub memory_used: Option<f64>,
    pub memory_total: Option<f64>,
    pub gpu: Option<f64>,
    pub gpu_temperature: Option<f64>,
    pub disk_read: Option<f64>,
    pub disk_write: Option<f64>,
    pub network_receive: Option<f64>,
    pub network_send: Option<f64>,
    pub battery: Option<f64>,
    /// What the battery is doing, for its words (`hardware_core::charge_state`).
    pub battery_state: Option<hardware_core::charge_state::ChargeReadings>,
    pub battery_seconds_left: Option<f64>,
    pub battery_seconds_to_full: Option<f64>,
    /// The hottest temperature any sensor reports, and the chip it came from.
    pub hottest: Option<f64>,
    pub hottest_chip: Option<String>,
    pub fan_rpm: Option<f64>,
}

fn id(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a tray metric id is well formed")
}

fn number(metrics: &monitor_core::MetricSet, raw: &str) -> Option<f64> {
    metrics
        .get(&id(raw))
        .and_then(Observation::as_f64)
        .filter(|v| v.is_finite())
}

fn add(total: Option<f64>, value: Option<f64>) -> Option<f64> {
    match (total, value) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    }
}

impl TrayReadings {
    /// Read one round of collector reports.
    pub fn from_round(round: &[CollectorReport]) -> Self {
        let mut readings = TrayReadings::default();
        let find = |name: &str| round.iter().find(|r| r.collector.as_str() == name);
        if let Some(cpu) = find("linux.cpu") {
            readings.cpu = number(&cpu.metrics, "cpu.utilization.busy");
            let speeds: Vec<f64> = cpu
                .entities_of(EntityKind::LogicalCpu)
                .filter_map(|e| number(&e.metrics, "cpu.frequency.current"))
                .collect();
            if !speeds.is_empty() {
                readings.cpu_hz = Some(speeds.iter().sum::<f64>() / speeds.len() as f64);
            }
            readings.hottest = number(&cpu.metrics, "cpu.package.temperature");
        }
        if let Some(memory) = find("linux.memory") {
            let total = number(&memory.metrics, "memory.total");
            let available = number(&memory.metrics, "memory.available");
            readings.memory_total = total;
            readings.memory_used = total.zip(available).map(|(t, a)| (t - a).max(0.0));
        }
        if let Some(gpu) = find("linux.gpu") {
            let first = gpu.entities_of(EntityKind::Gpu).next();
            readings.gpu = first.and_then(|e| number(&e.metrics, "gpu.utilization"));
            readings.gpu_temperature = first.and_then(|e| number(&e.metrics, "gpu.temperature"));
        }
        if let Some(storage) = find("linux.storage") {
            for disk in storage.entities_of(EntityKind::BlockDevice) {
                readings.disk_read = add(
                    readings.disk_read,
                    number(&disk.metrics, "storage.read.bytes.rate"),
                );
                readings.disk_write = add(
                    readings.disk_write,
                    number(&disk.metrics, "storage.write.bytes.rate"),
                );
            }
        }
        if let Some(network) = find("linux.network") {
            for adapter in network
                .entities_of(EntityKind::NetworkInterface)
                .filter(|e| e.id.key != "lo")
            {
                readings.network_receive = add(
                    readings.network_receive,
                    number(&adapter.metrics, "network.rx.bytes.rate"),
                );
                readings.network_send = add(
                    readings.network_send,
                    number(&adapter.metrics, "network.tx.bytes.rate"),
                );
            }
        }
        if let Some(battery) = find("linux.battery") {
            let system = battery
                .entities_of(EntityKind::Battery)
                .find(|e| !is_peripheral(e));
            if let Some(entity) = system {
                let state = charge_readings(battery);
                // Every battery's charge together (one battery's own).
                readings.battery = state.percent().map(|p| f64::from(p) / 100.0);
                readings.battery_state = Some(state);
                readings.battery_seconds_left = number(&entity.metrics, "battery.time.remaining");
                readings.battery_seconds_to_full = number(&entity.metrics, "battery.time.to_full");
            }
        }
        if let Some(sensors) = find("linux.sensors") {
            for sensor in sensors.entities_of(EntityKind::Sensor) {
                if let Some(t) = number(&sensor.metrics, "sensor.temperature")
                    && readings.hottest.is_none_or(|hottest| t > hottest)
                {
                    readings.hottest = Some(t);
                    readings.hottest_chip = sensor
                        .metrics
                        .get(&id("sensor.chip"))
                        .and_then(Observation::as_text)
                        .map(str::to_string);
                }
                if let Some(rpm) = number(&sensor.metrics, "sensor.fan.speed") {
                    readings.fan_rpm = Some(readings.fan_rpm.map_or(rpm, |r: f64| r.max(rpm)));
                }
            }
        }
        readings
    }

    pub fn memory_fraction(&self) -> Option<f64> {
        let total = self.memory_total.filter(|t| *t > 0.0)?;
        Some(self.memory_used? / total)
    }

    pub fn disk_total(&self) -> Option<f64> {
        add(self.disk_read, self.disk_write)
    }

    pub fn network_total(&self) -> Option<f64> {
        add(self.network_receive, self.network_send)
    }

    /// Whether the machine has something to show for a reading.
    pub fn has(&self, reading: Reading) -> bool {
        match reading {
            Reading::Cpu => self.cpu.is_some(),
            Reading::Memory => self.memory_total.is_some(),
            Reading::Gpu => self.gpu.is_some(),
            Reading::Disk => self.disk_total().is_some(),
            Reading::Network => self.network_total().is_some(),
            Reading::Temperature => self.hottest.is_some(),
        }
    }
}

/// The language the item speaks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    English,
    TraditionalChinese,
}

impl Locale {
    /// The window's own language choice when it made one, then the
    /// environment, so the item and the window always agree.
    pub fn resolve(stored: Option<&str>, lang: Option<&str>) -> Self {
        match stored.map(str::trim) {
            Some("zh-TW") => return Locale::TraditionalChinese,
            Some("en") => return Locale::English,
            _ => {}
        }
        match lang {
            Some(lang) if lang.starts_with("zh") => Locale::TraditionalChinese,
            _ => Locale::English,
        }
    }

    fn zh(self) -> bool {
        self == Locale::TraditionalChinese
    }

    fn pick(self, en: &'static str, zh: &'static str) -> &'static str {
        if self.zh() { zh } else { en }
    }

    pub fn reading_name(self, reading: Reading) -> &'static str {
        match reading {
            Reading::Cpu => self.pick("Processor", "處理器"),
            Reading::Memory => self.pick("Memory", "記憶體"),
            Reading::Gpu => self.pick("Graphics card", "顯示卡"),
            Reading::Disk => self.pick("Disk", "磁碟"),
            Reading::Network => self.pick("Network", "網路"),
            Reading::Temperature => self.pick("Temperature", "溫度"),
        }
    }

    pub fn app_name(self) -> &'static str {
        self.pick("Better Monitor", "系統監控")
    }

    /// The battery rule's and the power modes' language.
    pub fn language(self) -> hardware_core::charge_state::Language {
        if self.zh() {
            hardware_core::charge_state::Language::TraditionalChinese
        } else {
            hardware_core::charge_state::Language::English
        }
    }

    pub fn power_mode(self) -> &'static str {
        self.pick("Power mode", "電源模式")
    }
}

/// The power mode in force, when the machine offers modes and the daemon's
/// active one is among them (`hardware_core::power::mode_in_force`).
pub fn power_mode(power: &PowerFacts) -> Option<Profile> {
    shown(power, Locale::English).map(|shown| shown.mode)
}

/// The mode to show, by `hardware_core::power::mode_shown`, the one rule the
/// popup follows too.
fn shown(power: &PowerFacts, locale: Locale) -> Option<ModeShown> {
    match power {
        Presence::Present(facts) => mode_shown(facts, locale.language()),
        _ => None,
    }
}

/// The menu's and the tooltip's power mode line: the mode's name, or that it
/// could not be read. `None` when there is no mode to show, so nothing is
/// guessed.
pub fn power_line(power: &PowerFacts, locale: Locale) -> Option<String> {
    let text = match power {
        Presence::Unreadable(_) => unreadable_words(locale.language()).to_string(),
        _ => shown(power, locale)?.name.to_string(),
    };
    Some(if locale.zh() {
        format!("{}：{text}", locale.power_mode())
    } else {
        format!("{}: {text}", locale.power_mode())
    })
}

fn bytes(value: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = value.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

fn percent(fraction: f64) -> String {
    let p = (fraction * 100.0).max(0.0);
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

fn hours_minutes(seconds: f64, locale: Locale) -> String {
    let minutes = (seconds / 60.0).round().max(0.0) as u64;
    match (minutes / 60, locale.zh()) {
        (0, false) => format!("{} min", minutes % 60),
        (0, true) => format!("{} 分鐘", minutes % 60),
        (h, false) => format!("{h} h {} min", minutes % 60),
        (h, true) => format!("{h} 小時 {} 分鐘", minutes % 60),
    }
}

/// One line of text for a reading, as the menu and the tooltip write it.
/// `None` when the machine has nothing for it.
pub fn reading_line(reading: Reading, r: &TrayReadings, locale: Locale) -> Option<String> {
    let name = locale.reading_name(reading);
    let text = match reading {
        Reading::Cpu => {
            let mut text = percent(r.cpu?);
            if let Some(hz) = r.cpu_hz {
                text = format!("{text} · {:.2} GHz", hz / 1e9);
            }
            text
        }
        Reading::Memory => {
            let (used, total) = (r.memory_used?, r.memory_total?);
            if locale.zh() {
                format!(
                    "已用 {} / {}（{}）",
                    bytes(used),
                    bytes(total),
                    percent(used / total.max(1.0))
                )
            } else {
                format!(
                    "{} of {} ({})",
                    bytes(used),
                    bytes(total),
                    percent(used / total.max(1.0))
                )
            }
        }
        Reading::Gpu => {
            let mut text = percent(r.gpu?);
            if let Some(t) = r.gpu_temperature {
                text = format!("{text} · {t:.0} °C");
            }
            text
        }
        Reading::Disk => {
            r.disk_total()?;
            let read = bytes(r.disk_read.unwrap_or(0.0));
            let write = bytes(r.disk_write.unwrap_or(0.0));
            if locale.zh() {
                format!("讀取 {read}/s · 寫入 {write}/s")
            } else {
                format!("read {read}/s · write {write}/s")
            }
        }
        Reading::Network => {
            r.network_total()?;
            format!(
                "↓ {}/s ↑ {}/s",
                bytes(r.network_receive.unwrap_or(0.0)),
                bytes(r.network_send.unwrap_or(0.0))
            )
        }
        Reading::Temperature => format!("{:.0} °C", r.hottest?),
    };
    Some(if locale.zh() {
        format!("{name}：{text}")
    } else {
        format!("{name}: {text}")
    })
}

/// The battery's line: its charge and its state in the words every battery
/// line in Better OS uses (`battery_look`, ticket 170), and the time left
/// while the charge moves. `None` on a machine without a battery.
pub fn battery_line(r: &TrayReadings, locale: Locale) -> Option<String> {
    let text = {
        use hardware_core::charge_state::{ChargeState, Language, battery_look};
        let charge = percent(r.battery?);
        // The words every battery icon in Better OS uses (ticket 170).
        let look = r.battery_state.as_ref().map(|readings| {
            battery_look(
                readings,
                if locale.zh() {
                    Language::TraditionalChinese
                } else {
                    Language::English
                },
            )
        });
        let time = match (
            look.as_ref().map(|look| look.state),
            r.battery_seconds_left,
            r.battery_seconds_to_full,
        ) {
            (Some(state), Some(left), _) if state.on_battery() == Some(true) => {
                Some(if locale.zh() {
                    format!("剩下約 {}", hours_minutes(left, locale))
                } else {
                    format!("about {} left", hours_minutes(left, locale))
                })
            }
            (Some(ChargeState::Charging), _, Some(full)) => Some(if locale.zh() {
                format!("約 {}後充滿", hours_minutes(full, locale))
            } else {
                format!("full in about {}", hours_minutes(full, locale))
            }),
            _ => None,
        };
        let state = match look {
            Some(look) => format!("{charge} · {}", look.words),
            None => charge,
        };
        match time {
            Some(time) if locale.zh() => format!("{state}，{time}"),
            Some(time) => format!("{state}, {time}"),
            None => state,
        }
    };
    let name = locale.pick("Battery", "電池");
    Some(if locale.zh() {
        format!("{name}：{text}")
    } else {
        format!("{name}: {text}")
    })
}

/// The fan line, which is not a reading the icon can show.
pub fn fan_line(r: &TrayReadings, locale: Locale) -> Option<String> {
    let rpm = r.fan_rpm?;
    Some(if locale.zh() {
        format!("風扇：{rpm:.0} RPM")
    } else {
        format!("Fan: {rpm:.0} RPM")
    })
}

/// What the person chose for the icon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconChoice {
    pub shown: Vec<Reading>,
    pub style: crate::icon::IconStyle,
    /// Whether the power mode is shown beside the icon (ticket 295).
    pub power_mode: bool,
}

impl Default for IconChoice {
    /// Processor and memory as bars, like Stats out of the box.
    fn default() -> Self {
        Self {
            shown: vec![Reading::Cpu, Reading::Memory],
            style: crate::icon::IconStyle::Bars,
            power_mode: true,
        }
    }
}

impl IconChoice {
    /// The settings file: `$XDG_CONFIG_HOME/better-os/monitor/tray`, beside
    /// the window's own appearance and language.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("better-os").join("monitor").join("tray"))
    }

    pub fn parse(text: &str) -> Self {
        let mut choice = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "show" => {
                    let shown: Vec<Reading> = value.split(',').filter_map(Reading::parse).collect();
                    if !shown.is_empty() {
                        choice.shown = shown;
                    }
                }
                "style" => {
                    choice.style = match value.trim() {
                        "numbers" => crate::icon::IconStyle::Numbers,
                        _ => crate::icon::IconStyle::Bars,
                    }
                }
                "power_mode" => choice.power_mode = value.trim() != "hidden",
                _ => {}
            }
        }
        choice
    }

    pub fn render(&self) -> String {
        let shown: Vec<&str> = self.shown.iter().map(|r| r.key()).collect();
        let style = match self.style {
            crate::icon::IconStyle::Bars => "bars",
            crate::icon::IconStyle::Numbers => "numbers",
        };
        let power_mode = if self.power_mode { "shown" } else { "hidden" };
        format!(
            "show={}\nstyle={style}\npower_mode={power_mode}\n",
            shown.join(",")
        )
    }

    /// The choice stored at `path`, or the default. A choice stored before
    /// ticket 295 may name the battery, which the icon no longer shows: it
    /// is dropped (a choice of the battery alone falls back to the default
    /// readings, so the icon never goes empty) and the file is written again
    /// without it, so nothing stale is left behind.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let choice = Self::parse(&text);
        if names_the_battery(&text)
            && let Err(error) = choice.save(path)
        {
            eprintln!(
                "better-monitor-tray: the icon choice could not be rewritten without the \
                 battery: {error}"
            );
        }
        choice
    }

    /// Written to a temporary file and renamed, so a crash never leaves half.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, self.render())?;
        std::fs::rename(temporary, path)
    }

    /// Show or hide a reading, keeping at least one, in the fixed order.
    pub fn toggle(&mut self, reading: Reading) {
        if let Some(index) = self.shown.iter().position(|r| *r == reading) {
            if self.shown.len() > 1 {
                self.shown.remove(index);
            }
        } else {
            self.shown.push(reading);
            self.shown.sort();
        }
    }
}

/// Whether a stored choice still shows the battery, as one stored before
/// ticket 295 may.
fn names_the_battery(text: &str) -> bool {
    text.lines().any(|line| {
        line.split_once('=').is_some_and(|(key, value)| {
            key.trim() == "show" && value.split(',').any(|word| word.trim() == "battery")
        })
    })
}

/// What a menu item does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    OpenWindow,
    OpenPopup,
    Toggle(Reading),
    Style(crate::icon::IconStyle),
    /// Show or hide the power mode beside the icon.
    TogglePowerMode,
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Standard,
    Separator,
    Checkmark {
        checked: bool,
    },
    Radio {
        checked: bool,
    },
    /// A line that states something. Never activatable.
    Info,
    Submenu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub kind: ItemKind,
    pub enabled: bool,
    pub action: Option<MenuAction>,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    fn new(label: impl Into<String>, kind: ItemKind, action: Option<MenuAction>) -> Self {
        Self {
            id: 0,
            label: label.into(),
            enabled: !matches!(kind, ItemKind::Info | ItemKind::Separator),
            kind,
            action,
            children: Vec::new(),
        }
    }

    fn separator() -> Self {
        Self::new("", ItemKind::Separator, None)
    }

    fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }

    /// This item's id and its children's, in order: two menus with the same
    /// skeleton differ in their properties only.
    fn skeleton(&self, out: &mut Vec<(i32, usize)>) {
        out.push((self.id, self.children.len()));
        for child in &self.children {
            child.skeleton(out);
        }
    }

    fn walk_mut(&mut self, visit: &mut impl FnMut(&mut MenuItem)) {
        visit(self);
        for child in &mut self.children {
            child.walk_mut(visit);
        }
    }

    fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a MenuItem)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Menu {
    pub items: Vec<MenuItem>,
}

impl Menu {
    pub fn find(&self, id: i32) -> Option<&MenuItem> {
        let mut found = None;
        for item in &self.items {
            item.walk(&mut |candidate| {
                if candidate.id == id {
                    found = Some(candidate);
                }
            });
        }
        found
    }

    pub fn find_by_label(&self, label: &str) -> Option<&MenuItem> {
        let mut found = None;
        for item in &self.items {
            item.walk(&mut |candidate| {
                if found.is_none() && candidate.label == label {
                    found = Some(candidate);
                }
            });
        }
        found
    }

    /// Every id in order, each with how many children it has. Equal
    /// skeletons mean the same items in the same places, so moving from one
    /// menu to the other changes properties and never the layout.
    pub fn skeleton(&self) -> Vec<(i32, usize)> {
        let mut out = Vec::new();
        for item in &self.items {
            item.skeleton(&mut out);
        }
        out
    }

    /// This menu's items in this menu's places, each showing what `newer`
    /// says about the same id. An item `newer` no longer has keeps what it
    /// said last. This is what a host with a submenu open is shown while a
    /// change of layout waits for the submenu to close.
    pub fn showing(&self, newer: &Menu) -> Menu {
        let mut merged = self.clone();
        for item in &mut merged.items {
            item.walk_mut(&mut |item| {
                if let Some(update) = newer.find(item.id)
                    && std::mem::discriminant(&update.kind) == std::mem::discriminant(&item.kind)
                {
                    item.label.clone_from(&update.label);
                    item.kind = update.kind.clone();
                    item.enabled = update.enabled;
                    item.action = update.action;
                }
            });
        }
        merged
    }

    pub fn labels(&self) -> Vec<&str> {
        let mut labels = Vec::new();
        for item in &self.items {
            item.walk(&mut |item| {
                if item.kind != ItemKind::Separator {
                    labels.push(item.label.as_str());
                }
            });
        }
        labels
    }
}

/// Whether the service is answering.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ServiceState {
    #[default]
    Answering,
    /// Nothing answers on the bus.
    Unavailable,
    /// The service speaks an older or newer protocol: Better Monitor was
    /// updated and the service has not restarted yet.
    Outdated,
}

impl ServiceState {
    /// The sentence the menu and the tooltip show when there are no readings.
    pub fn sentence(self, locale: Locale) -> &'static str {
        match self {
            ServiceState::Outdated => locale.pick(
                "Better Monitor was updated. Its service restarts at your next sign-in.",
                "系統監控已更新，服務會在下次登入時重新啟動。",
            ),
            _ => locale.pick(
                "Better Monitor's service is not answering",
                "系統監控服務沒有回應",
            ),
        }
    }
}

/// The id each item of the menu always has, whatever else the menu holds.
///
/// A panel caches items by id, and the GNOME AppIndicator extension rebuilds
/// every row it already knows when the layout changes, which closes an open
/// submenu. So an item keeps its id when a reading line above it appears or
/// goes away, and a new reading is a change of properties only.
mod ids {
    /// The reading lines, one per [`super::Reading`] in its fixed order.
    pub const READING: i32 = 1;
    /// The battery's line, after the readings' (ticket 295: no longer a
    /// reading the icon shows, so no longer among them).
    pub const BATTERY: i32 = 7;
    pub const FAN: i32 = 8;
    pub const SERVICE: i32 = 9;
    pub const AFTER_READINGS: i32 = 10;
    pub const SHOW_ALL: i32 = 11;
    pub const OPEN_WINDOW: i32 = 12;
    pub const SHOW: i32 = 13;
    /// The check boxes under [`SHOW`], one per reading in the same order.
    pub const SHOW_READING: i32 = 14;
    pub const AFTER_SHOW_READINGS: i32 = 21;
    pub const STYLE_BARS: i32 = 22;
    pub const STYLE_NUMBERS: i32 = 23;
    pub const BEFORE_QUIT: i32 = 24;
    pub const QUIT: i32 = 25;
    /// The power mode line, after the reading lines (ticket 295).
    pub const POWER: i32 = 26;
    /// Its check box under [`SHOW`], after the readings'.
    pub const SHOW_POWER: i32 = 27;
}

fn reading_offset(reading: Reading) -> i32 {
    Reading::ALL
        .iter()
        .position(|candidate| *candidate == reading)
        .and_then(|index| i32::try_from(index).ok())
        .expect("every reading is in Reading::ALL")
}

/// The right-click menu: the readings as text, then what the icon shows,
/// then opening the window and quitting. Every item has the id of its role
/// (see [`ids`]), so a reading that comes or goes moves no other item.
pub fn build(
    readings: Option<&TrayReadings>,
    power: &PowerFacts,
    choice: &IconChoice,
    locale: Locale,
    service: ServiceState,
) -> Menu {
    let mut items = Vec::new();
    match readings {
        Some(r) => {
            for reading in Reading::ALL {
                if let Some(line) = reading_line(reading, r, locale) {
                    items.push(
                        MenuItem::new(line, ItemKind::Info, None)
                            .with_id(ids::READING + reading_offset(reading)),
                    );
                }
            }
            if let Some(line) = battery_line(r, locale) {
                items.push(MenuItem::new(line, ItemKind::Info, None).with_id(ids::BATTERY));
            }
            if let Some(line) = fan_line(r, locale) {
                items.push(MenuItem::new(line, ItemKind::Info, None).with_id(ids::FAN));
            }
        }
        None => items.push(
            MenuItem::new(service.sentence(locale), ItemKind::Info, None).with_id(ids::SERVICE),
        ),
    }
    // The power mode comes from power-profiles-daemon, not the service, so
    // it is said whether or not the service answers.
    if let Some(line) = power_line(power, locale) {
        items.push(MenuItem::new(line, ItemKind::Info, None).with_id(ids::POWER));
    }
    items.push(MenuItem::separator().with_id(ids::AFTER_READINGS));
    items.push(
        MenuItem::new(
            locale.pick("Show all readings", "顯示所有讀數"),
            ItemKind::Standard,
            Some(MenuAction::OpenPopup),
        )
        .with_id(ids::SHOW_ALL),
    );
    items.push(
        MenuItem::new(
            locale.pick("Open Better Monitor", "開啟系統監控"),
            ItemKind::Standard,
            Some(MenuAction::OpenWindow),
        )
        .with_id(ids::OPEN_WINDOW),
    );
    let mut show = MenuItem::new(
        locale.pick("Show in the taskbar", "在工作列顯示"),
        ItemKind::Submenu,
        None,
    )
    .with_id(ids::SHOW);
    for reading in Reading::ALL {
        let checked = choice.shown.contains(&reading);
        let mut item = MenuItem::new(
            locale.reading_name(reading),
            ItemKind::Checkmark { checked },
            Some(MenuAction::Toggle(reading)),
        )
        .with_id(ids::SHOW_READING + reading_offset(reading));
        // The last reading shown cannot be hidden: an empty icon is a
        // missing icon.
        item.enabled =
            !(checked && choice.shown.len() == 1) && readings.is_none_or(|r| r.has(reading));
        show.children.push(item);
    }
    let mut power_item = MenuItem::new(
        locale.power_mode(),
        ItemKind::Checkmark {
            checked: choice.power_mode,
        },
        Some(MenuAction::TogglePowerMode),
    )
    .with_id(ids::SHOW_POWER);
    // Nothing to show on a machine without modes.
    power_item.enabled = power_mode(power).is_some();
    show.children.push(power_item);
    show.children
        .push(MenuItem::separator().with_id(ids::AFTER_SHOW_READINGS));
    for (style, label, id) in [
        (
            crate::icon::IconStyle::Bars,
            locale.pick("As bars", "以長條顯示"),
            ids::STYLE_BARS,
        ),
        (
            crate::icon::IconStyle::Numbers,
            locale.pick("As numbers (the first two)", "以數字顯示（前兩項）"),
            ids::STYLE_NUMBERS,
        ),
    ] {
        show.children.push(
            MenuItem::new(
                label,
                ItemKind::Radio {
                    checked: choice.style == style,
                },
                Some(MenuAction::Style(style)),
            )
            .with_id(id),
        );
    }
    items.push(show);
    items.push(MenuItem::separator().with_id(ids::BEFORE_QUIT));
    items.push(
        MenuItem::new(
            locale.pick("Quit taskbar item", "結束工作列圖示"),
            ItemKind::Standard,
            Some(MenuAction::Quit),
        )
        .with_id(ids::QUIT),
    );
    Menu { items }
}

/// The tooltip: the app's name, and every reading on its own line.
pub fn tooltip(
    readings: Option<&TrayReadings>,
    power: &PowerFacts,
    locale: Locale,
    service: ServiceState,
) -> (String, String) {
    let mut lines: Vec<String> = match readings {
        Some(r) => Reading::ALL
            .into_iter()
            .filter_map(|reading| reading_line(reading, r, locale))
            .chain(battery_line(r, locale))
            .collect(),
        None => vec![service.sentence(locale).to_string()],
    };
    lines.extend(power_line(power, locale));
    // A mode held by an application or reduced by the daemon says so, as
    // the System page does.
    lines.extend(
        shown(power, locale)
            .into_iter()
            .flat_map(|shown| shown.cautions),
    );
    (locale.app_name().to_string(), lines.join("\n"))
}

/// The mode whose mark the picture shows (ticket 295): the mode to show,
/// unless the person unticked "Power mode" under "Show in the taskbar".
pub fn mark_mode(power: &PowerFacts, choice: &IconChoice) -> Option<Profile> {
    power_mode(power).filter(|_| choice.power_mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::IconStyle;

    fn readings() -> TrayReadings {
        TrayReadings {
            cpu: Some(0.123),
            cpu_hz: Some(2.01e9),
            memory_used: Some(10.0 * 1024.0 * 1024.0 * 1024.0),
            memory_total: Some(30.5 * 1024.0 * 1024.0 * 1024.0),
            battery: Some(0.83),
            battery_state: Some(hardware_core::charge_state::ChargeReadings::single(
                Some(true),
                Some(hardware_core::battery::BatteryStatus::Charging),
                Some(83),
                hardware_core::charge_state::ChargeLimit::NotSupported,
            )),
            battery_seconds_to_full: Some(14.0 * 60.0),
            hottest: Some(73.0),
            fan_rpm: Some(3450.0),
            ..TrayReadings::default()
        }
    }

    /// The owner's machine on 2026-11-21, as the service reports it
    /// (ticket 170).
    #[test]
    fn a_battery_held_at_its_limit_says_so_in_the_menu() {
        use monitor_core::{CollectorId, Entity, EntityId, MetricSet, Timestamp};
        let set = |pairs: &[(&str, Observation)]| -> MetricSet {
            pairs.iter().map(|(n, o)| (id(n), o.clone())).collect()
        };
        let mut report = CollectorReport::new(
            CollectorId::new("linux.battery").unwrap(),
            Timestamp {
                unix_ms: 1,
                monotonic_ns: 1,
            },
        );
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::Battery, "BAT1"),
            set(&[
                ("battery.charge", Observation::float(0.8)),
                ("battery.status", Observation::text("not_charging")),
                ("battery.charge.limit", Observation::float(0.8)),
                ("battery.peripheral", Observation::boolean(false)),
            ]),
        ));
        report.entities.push(Entity::new(
            EntityId::new(EntityKind::PowerSource, "ACAD"),
            set(&[
                ("power_source.kind", Observation::text("mains")),
                ("power_source.online", Observation::boolean(true)),
            ]),
        ));
        let readings = TrayReadings::from_round(&[report]);
        for (locale, line) in [
            (Locale::English, "Battery: 80% · Plugged in, holding at 80%"),
            (
                Locale::TraditionalChinese,
                "電池：80% · 已接上電源，維持在 80%",
            ),
        ] {
            let menu = build(
                Some(&readings),
                &Presence::Absent,
                &IconChoice::default(),
                locale,
                ServiceState::Answering,
            );
            assert!(menu.labels().contains(&line), "{:?}", menu.labels());
        }
    }

    #[test]
    fn the_menu_lists_the_readings_the_machine_has() {
        let menu = build(
            Some(&readings()),
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        let labels = menu.labels();
        assert!(labels.contains(&"Processor: 12% · 2.01 GHz"), "{labels:?}");
        assert!(
            labels.contains(&"Memory: 10.0 GB of 30.5 GB (33%)"),
            "{labels:?}"
        );
        assert!(
            labels.contains(&"Battery: 83% · Charging, full in about 14 min"),
            "{labels:?}"
        );
        assert!(labels.contains(&"Temperature: 73 °C"));
        assert!(labels.contains(&"Fan: 3450 RPM"));
        assert!(
            !labels.iter().any(|l| l.starts_with("Graphics card:")),
            "no card read"
        );
        // Readings are statements, not buttons.
        assert!(!menu.find_by_label("Temperature: 73 °C").unwrap().enabled);
    }

    #[test]
    fn an_outdated_service_is_told_apart_from_a_missing_one() {
        let menu = build(
            None,
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Outdated,
        );
        assert!(
            menu.labels()[0].contains("was updated"),
            "{:?}",
            menu.labels()
        );
        let menu = build(
            None,
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Unavailable,
        );
        assert_eq!(
            menu.labels()[0],
            "Better Monitor's service is not answering"
        );
        let (_, body) = tooltip(
            None,
            &Presence::Absent,
            Locale::English,
            ServiceState::Outdated,
        );
        assert!(body.contains("was updated"));
    }

    #[test]
    fn the_last_reading_on_the_icon_cannot_be_hidden() {
        let mut choice = IconChoice {
            shown: vec![Reading::Cpu],
            style: IconStyle::Bars,
            power_mode: true,
        };
        choice.toggle(Reading::Cpu);
        assert_eq!(choice.shown, vec![Reading::Cpu]);
        let menu = build(
            Some(&readings()),
            &Presence::Absent,
            &choice,
            Locale::English,
            ServiceState::Answering,
        );
        let processor = &menu.find_by_label("Show in the taskbar").unwrap().children[0];
        assert_eq!(processor.kind, ItemKind::Checkmark { checked: true });
        assert!(!processor.enabled);
        choice.toggle(Reading::Temperature);
        choice.toggle(Reading::Memory);
        assert_eq!(
            choice.shown,
            vec![Reading::Cpu, Reading::Memory, Reading::Temperature]
        );
    }

    #[test]
    fn the_taskbar_offers_no_battery_and_the_menu_keeps_its_words() {
        let menu = build(
            Some(&readings()),
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        let show = menu.find_by_label("Show in the taskbar").unwrap();
        assert!(
            !show.children.iter().any(|item| item.label == "Battery"),
            "{:?}",
            show.children
        );
        assert!(
            menu.labels()
                .contains(&"Battery: 83% · Charging, full in about 14 min")
        );
        let (_, body) = tooltip(
            Some(&readings()),
            &Presence::Absent,
            Locale::TraditionalChinese,
            ServiceState::Answering,
        );
        assert!(body.contains("電池：83% · 充電中"), "{body}");
    }

    #[test]
    fn a_choice_that_showed_the_battery_is_migrated_in_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tray");
        std::fs::write(&path, "show=cpu,battery\nstyle=numbers\n").unwrap();
        let choice = IconChoice::load(&path);
        assert_eq!(choice.shown, vec![Reading::Cpu]);
        assert_eq!(choice.style, IconStyle::Numbers);
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(!stored.contains("battery"), "{stored}");
        assert_eq!(IconChoice::load(&path), choice);

        // The battery alone: the icon falls back to the default readings
        // rather than going empty, and keeps the person's style.
        std::fs::write(&path, "show=battery\nstyle=numbers\n").unwrap();
        let choice = IconChoice::load(&path);
        assert_eq!(choice.shown, IconChoice::default().shown);
        assert_eq!(choice.style, IconStyle::Numbers);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("battery"));

        // A choice that never named it is not written.
        std::fs::write(&path, "show=gpu\n").unwrap();
        IconChoice::load(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "show=gpu\n");
    }

    #[test]
    fn the_choice_survives_a_round_trip_through_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tray");
        assert_eq!(IconChoice::load(&path), IconChoice::default());
        let choice = IconChoice {
            shown: vec![Reading::Cpu, Reading::Temperature],
            style: IconStyle::Numbers,
            power_mode: false,
        };
        choice.save(&path).unwrap();
        assert_eq!(IconChoice::load(&path), choice);
        assert_eq!(
            IconChoice::parse("show=nonsense\nstyle=weird"),
            IconChoice::default()
        );
    }

    #[test]
    fn the_language_follows_the_window_then_the_environment() {
        assert_eq!(
            Locale::resolve(Some("zh-TW\n"), Some("en_US.UTF-8")),
            Locale::TraditionalChinese
        );
        assert_eq!(
            Locale::resolve(Some("system"), Some("zh_TW.UTF-8")),
            Locale::TraditionalChinese
        );
        assert_eq!(Locale::resolve(None, None), Locale::English);
        let menu = build(
            Some(&readings()),
            &Presence::Absent,
            &IconChoice::default(),
            Locale::TraditionalChinese,
            ServiceState::Answering,
        );
        assert!(menu.labels().contains(&"處理器：12% · 2.01 GHz"));
    }

    #[test]
    fn an_item_keeps_its_id_when_a_reading_above_it_comes_or_goes() {
        let without = build(
            Some(&TrayReadings {
                battery: None,
                ..readings()
            }),
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        let with = build(
            Some(&readings()),
            &Presence::Absent,
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        assert_ne!(without.skeleton(), with.skeleton());
        for label in ["Show in the taskbar", "Quit taskbar item", "As bars"] {
            assert_eq!(
                without.find_by_label(label).unwrap().id,
                with.find_by_label(label).unwrap().id,
                "{label}"
            );
        }
    }

    fn power(profiles: &[&str], active: &str) -> PowerFacts {
        Presence::Present(PowerProfilesFacts {
            active: active.into(),
            profiles: profiles.iter().map(|p| p.to_string()).collect(),
            ..PowerProfilesFacts::default()
        })
    }

    const ALL_MODES: [&str; 3] = ["power-saver", "balanced", "performance"];

    #[test]
    fn each_power_mode_is_named_in_the_menu_the_tooltip_and_beside_the_icon() {
        for (active, english, chinese, icon) in [
            (
                "power-saver",
                "Power mode: Best power efficiency",
                "電源模式：最佳電源效率",
                "power-profile-power-saver-symbolic",
            ),
            (
                "balanced",
                "Power mode: Balanced",
                "電源模式：平衡",
                "power-profile-balanced-symbolic",
            ),
            (
                "performance",
                "Power mode: Best performance",
                "電源模式：最佳效能",
                "power-profile-performance-symbolic",
            ),
        ] {
            let facts = power(&ALL_MODES, active);
            for (locale, line) in [
                (Locale::English, english),
                (Locale::TraditionalChinese, chinese),
            ] {
                let menu = build(
                    Some(&readings()),
                    &facts,
                    &IconChoice::default(),
                    locale,
                    ServiceState::Answering,
                );
                assert!(menu.labels().contains(&line), "{:?}", menu.labels());
                let (_, body) = tooltip(Some(&readings()), &facts, locale, ServiceState::Answering);
                assert!(body.ends_with(line), "{body}");
            }
            let mode = mark_mode(&facts, &IconChoice::default()).unwrap();
            assert_eq!(crate::picture::icon_name(mode), icon);
        }
    }

    #[test]
    fn a_machine_without_power_modes_shows_nothing_for_them() {
        for facts in [Presence::Absent, power(&[], ""), power(&[], "balanced")] {
            let menu = build(
                Some(&readings()),
                &facts,
                &IconChoice::default(),
                Locale::English,
                ServiceState::Answering,
            );
            assert!(
                !menu.labels().iter().any(|l| l.starts_with("Power mode:")),
                "{:?}",
                menu.labels()
            );
            let (_, body) = tooltip(
                Some(&readings()),
                &facts,
                Locale::English,
                ServiceState::Answering,
            );
            assert!(!body.contains("Power mode"), "{body}");
            assert_eq!(mark_mode(&facts, &IconChoice::default()), None);
            // Its check box has nothing to show.
            assert!(!menu.find_by_label("Power mode").unwrap().enabled);
        }
    }

    #[test]
    fn a_mode_the_daemon_does_not_list_is_never_drawn_as_one_it_does() {
        // An active mode this version does not know is not shown at all,
        // never as the daemon's English word and never as a known mode.
        let facts = power(&["balanced", "turbo"], "turbo");
        let menu = build(
            Some(&readings()),
            &facts,
            &IconChoice::default(),
            Locale::TraditionalChinese,
            ServiceState::Answering,
        );
        assert!(
            !menu.labels().iter().any(|l| l.starts_with("電源模式：")),
            "{:?}",
            menu.labels()
        );
        assert_eq!(mark_mode(&facts, &IconChoice::default()), None);
        let unreadable: PowerFacts = Presence::Unreadable("the daemon failed".into());
        let menu = build(
            Some(&readings()),
            &unreadable,
            &IconChoice::default(),
            Locale::TraditionalChinese,
            ServiceState::Answering,
        );
        assert!(
            menu.labels().contains(&"電源模式：無法讀取"),
            "{:?}",
            menu.labels()
        );
        assert_eq!(mark_mode(&unreadable, &IconChoice::default()), None);
    }

    #[test]
    fn a_held_or_reduced_mode_says_so_in_the_tooltips() {
        let mut facts = PowerProfilesFacts {
            active: "performance".into(),
            profiles: ALL_MODES.iter().map(|p| p.to_string()).collect(),
            ..PowerProfilesFacts::default()
        };
        facts.holds = vec![hardware_core::power::Hold {
            application: "GNOME Boxes".into(),
            profile: "performance".into(),
        }];
        let facts = Presence::Present(facts);
        let (_, body) = tooltip(
            Some(&readings()),
            &facts,
            Locale::English,
            ServiceState::Answering,
        );
        assert!(
            body.ends_with(
                "Power mode: Best performance\nGNOME Boxes is asking for Best performance."
            ),
            "{body}"
        );
        let (_, body) = tooltip(
            Some(&readings()),
            &facts,
            Locale::TraditionalChinese,
            ServiceState::Answering,
        );
        assert!(
            body.ends_with("電源模式：最佳效能\nGNOME Boxes 正在要求「最佳效能」模式。"),
            "{body}"
        );
    }

    #[test]
    fn the_power_mode_is_said_even_while_the_service_is_not_answering() {
        let menu = build(
            None,
            &power(&ALL_MODES, "performance"),
            &IconChoice::default(),
            Locale::English,
            ServiceState::Unavailable,
        );
        assert!(menu.labels().contains(&"Power mode: Best performance"));
    }

    #[test]
    fn the_power_mode_item_can_be_hidden_and_the_choice_is_kept() {
        let facts = power(&ALL_MODES, "balanced");
        let mut choice = IconChoice::default();
        assert!(choice.power_mode);
        choice.power_mode = false;
        assert_eq!(mark_mode(&facts, &choice), None);
        let menu = build(
            Some(&readings()),
            &facts,
            &choice,
            Locale::English,
            ServiceState::Answering,
        );
        let check = menu.find_by_label("Power mode").unwrap();
        assert_eq!(check.kind, ItemKind::Checkmark { checked: false });
        assert!(check.enabled);
        assert_eq!(check.action, Some(MenuAction::TogglePowerMode));
        // The line in the menu stays: unticking it takes only the mark away.
        assert!(menu.labels().contains(&"Power mode: Balanced"));
        assert_eq!(IconChoice::parse(&choice.render()), choice);
        assert!(IconChoice::parse("show=cpu\nstyle=bars\n").power_mode);
    }

    #[test]
    fn ids_are_unique_and_stable() {
        let menu = build(
            Some(&readings()),
            &power(&ALL_MODES, "balanced"),
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        let mut ids = Vec::new();
        for item in &menu.items {
            item.walk(&mut |item| ids.push(item.id));
        }
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
        assert_eq!(
            menu,
            build(
                Some(&readings()),
                &power(&ALL_MODES, "balanced"),
                &IconChoice::default(),
                Locale::English,
                ServiceState::Answering
            )
        );
    }
}

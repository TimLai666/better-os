//! What the taskbar item knows and says: the readings, the person's choice
//! of what the icon shows, the words in both languages, and the right-click
//! menu built from them. None of it needs a bus, so all of it is tested here.

use std::path::{Path, PathBuf};

use monitor_core::{CollectorReport, EntityKind, MetricId, MetricScalar, Observation};

/// A reading the icon can show.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Reading {
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Battery,
    Temperature,
}

impl Reading {
    pub const ALL: [Reading; 7] = [
        Reading::Cpu,
        Reading::Memory,
        Reading::Gpu,
        Reading::Disk,
        Reading::Network,
        Reading::Battery,
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
            Reading::Battery => "battery",
            Reading::Temperature => "temperature",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reading| reading.key() == key.trim())
    }
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
    /// `charging`, `discharging`, `full`, `not_charging`.
    pub battery_status: Option<String>,
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
            let system = battery.entities_of(EntityKind::Battery).find(|e| {
                !matches!(
                    e.metrics.get(&id("battery.peripheral")),
                    Some(Observation::Value(MetricScalar::Bool(true)))
                )
            });
            if let Some(entity) = system {
                readings.battery = number(&entity.metrics, "battery.charge");
                readings.battery_status = entity
                    .metrics
                    .get(&id("battery.status"))
                    .and_then(Observation::as_text)
                    .map(str::to_string);
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
            Reading::Battery => self.battery.is_some(),
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
            Reading::Battery => self.pick("Battery", "電池"),
            Reading::Temperature => self.pick("Temperature", "溫度"),
        }
    }

    pub fn app_name(self) -> &'static str {
        self.pick("Better Monitor", "系統監控")
    }
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
        Reading::Battery => {
            let charge = percent(r.battery?);
            let state = match r.battery_status.as_deref() {
                Some("charging") => locale.pick("charging", "充電中"),
                Some("discharging") => locale.pick("on battery", "使用電池中"),
                Some("full") => locale.pick("fully charged", "已充飽"),
                Some("not_charging") => locale.pick("plugged in", "已接電源"),
                _ => "",
            };
            let time = match (
                r.battery_status.as_deref(),
                r.battery_seconds_left,
                r.battery_seconds_to_full,
            ) {
                (Some("discharging"), Some(left), _) => Some(if locale.zh() {
                    format!("剩下約 {}", hours_minutes(left, locale))
                } else {
                    format!("about {} left", hours_minutes(left, locale))
                }),
                (Some("charging"), _, Some(full)) => Some(if locale.zh() {
                    format!("約 {}後充滿", hours_minutes(full, locale))
                } else {
                    format!("full in about {}", hours_minutes(full, locale))
                }),
                _ => None,
            };
            [
                Some(charge),
                (!state.is_empty()).then(|| state.to_string()),
                time,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(if locale.zh() { "，" } else { ", " })
        }
        Reading::Temperature => format!("{:.0} °C", r.hottest?),
    };
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
}

impl Default for IconChoice {
    /// Processor and memory as bars, like Stats out of the box.
    fn default() -> Self {
        Self {
            shown: vec![Reading::Cpu, Reading::Memory],
            style: crate::icon::IconStyle::Bars,
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
        format!("show={}\nstyle={style}\n", shown.join(","))
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
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

/// What a menu item does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    OpenWindow,
    OpenPopup,
    Toggle(Reading),
    Style(crate::icon::IconStyle),
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
            if let Some(line) = fan_line(r, locale) {
                items.push(MenuItem::new(line, ItemKind::Info, None).with_id(ids::FAN));
            }
        }
        None => items.push(
            MenuItem::new(service.sentence(locale), ItemKind::Info, None).with_id(ids::SERVICE),
        ),
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
    locale: Locale,
    service: ServiceState,
) -> (String, String) {
    let body = match readings {
        Some(r) => Reading::ALL
            .into_iter()
            .filter_map(|reading| reading_line(reading, r, locale))
            .collect::<Vec<_>>()
            .join("\n"),
        None => service.sentence(locale).to_string(),
    };
    (locale.app_name().to_string(), body)
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
            battery_status: Some("charging".into()),
            battery_seconds_to_full: Some(14.0 * 60.0),
            hottest: Some(73.0),
            fan_rpm: Some(3450.0),
            ..TrayReadings::default()
        }
    }

    #[test]
    fn the_menu_lists_the_readings_the_machine_has() {
        let menu = build(
            Some(&readings()),
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
            labels.contains(&"Battery: 83%, charging, full in about 14 min"),
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
            &IconChoice::default(),
            Locale::English,
            ServiceState::Unavailable,
        );
        assert_eq!(
            menu.labels()[0],
            "Better Monitor's service is not answering"
        );
        let (_, body) = tooltip(None, Locale::English, ServiceState::Outdated);
        assert!(body.contains("was updated"));
    }

    #[test]
    fn the_last_reading_on_the_icon_cannot_be_hidden() {
        let mut choice = IconChoice {
            shown: vec![Reading::Cpu],
            style: IconStyle::Bars,
        };
        choice.toggle(Reading::Cpu);
        assert_eq!(choice.shown, vec![Reading::Cpu]);
        let menu = build(
            Some(&readings()),
            &choice,
            Locale::English,
            ServiceState::Answering,
        );
        let processor = &menu.find_by_label("Show in the taskbar").unwrap().children[0];
        assert_eq!(processor.kind, ItemKind::Checkmark { checked: true });
        assert!(!processor.enabled);
        choice.toggle(Reading::Battery);
        choice.toggle(Reading::Memory);
        assert_eq!(
            choice.shown,
            vec![Reading::Cpu, Reading::Memory, Reading::Battery]
        );
    }

    #[test]
    fn the_choice_survives_a_round_trip_through_its_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tray");
        assert_eq!(IconChoice::load(&path), IconChoice::default());
        let choice = IconChoice {
            shown: vec![Reading::Cpu, Reading::Temperature],
            style: IconStyle::Numbers,
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
            &IconChoice::default(),
            Locale::English,
            ServiceState::Answering,
        );
        let with = build(
            Some(&readings()),
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

    #[test]
    fn ids_are_unique_and_stable() {
        let menu = build(
            Some(&readings()),
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
                &IconChoice::default(),
                Locale::English,
                ServiceState::Answering
            )
        );
    }
}

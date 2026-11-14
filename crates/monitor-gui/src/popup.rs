//! The taskbar popup: `better-monitor --popup`.
//!
//! The taskbar item (`better-monitor-tray`) starts this when it is clicked. It
//! is the same program as the window, so it draws with the same kit, reads
//! from the same service, and speaks the same words; what differs is that it
//! is small, shows every resource at once, and closes as soon as it loses
//! focus, the way the macOS menu-bar app Stats does.
//!
//! GNOME gives an application no way to place a window next to its tray icon
//! (and Wayland gives no application a way to place any window at all), so
//! the popup asks for the top-right corner of the monitor the pointer was on
//! when the icon was clicked. Where the compositor ignores that, it goes
//! wherever the compositor puts new windows.
//!
//! The sparklines are the service's stored history of the last five minutes,
//! so they are full the moment the popup opens rather than starting empty.
//! The figures are the live round, once a second.
//!
//! What is shown, in what order, and how a history sample becomes a point is
//! decided in the GPUI-free half of this module and tested directly.

use better_ui::fluent::button::Button;
use better_ui::fluent::chart::{ChartHue, ChartSeries, LineChart};
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, label, palette, radius, spacing};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName};
use monitor_core::{EntityKind, MetricId};
use monitor_store::Sample;
use monitor_views::ProcessFacts;

use crate::charts::Readings;
use crate::i18n::{self, Locale, copy};
use crate::link::{self, LinkRequest, LinkUpdate};
use crate::tables::{bytes, rate, share, watts};

/// How much history the sparklines show.
pub(crate) const POPUP_HISTORY_SECONDS: u64 = 300;

/// How many processes the popup names per resource.
pub(crate) const POPUP_TOP: usize = 3;

/// The popup's window title.
pub(crate) const POPUP_TITLE: &str = "Better Monitor (taskbar)";

/// The popup's size.
pub(crate) const POPUP_WIDTH: f32 = 400.0;
pub(crate) const POPUP_HEIGHT: f32 = 760.0;

/// One block of the popup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Section {
    Cpu,
    Memory,
    Gpu,
    Disk,
    Network,
    Battery,
    Temperatures,
    Fans,
}

impl Section {
    const ORDER: [Section; 8] = [
        Section::Cpu,
        Section::Memory,
        Section::Gpu,
        Section::Disk,
        Section::Network,
        Section::Battery,
        Section::Temperatures,
        Section::Fans,
    ];

    fn hue(self) -> ChartHue {
        match self {
            Section::Cpu => ChartHue::Blue,
            Section::Memory => ChartHue::Purple,
            Section::Gpu => ChartHue::Teal,
            Section::Disk => ChartHue::Green,
            Section::Network => ChartHue::Orange,
            Section::Battery => ChartHue::Green,
            Section::Temperatures => ChartHue::Red,
            Section::Fans => ChartHue::Blue,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Section::Cpu => IconName::Cpu,
            Section::Memory => IconName::MemoryStick,
            Section::Gpu => IconName::Frame,
            Section::Disk => IconName::HardDrive,
            Section::Network => IconName::Network,
            Section::Battery => IconName::Battery,
            Section::Temperatures => IconName::Sun,
            Section::Fans => IconName::Loader,
        }
    }
}

/// The sections this machine has something for, in order. A machine without
/// a battery, a graphics card, or a fan has no block for it.
pub(crate) fn sections(readings: &Readings) -> Vec<Section> {
    Section::ORDER
        .into_iter()
        .filter(|section| match section {
            Section::Cpu => readings.has_cpu,
            Section::Memory => readings.has_memory,
            Section::Gpu => !readings.gpus.is_empty(),
            Section::Disk => !readings.disks.is_empty(),
            Section::Network => !readings.adapters.is_empty(),
            Section::Battery => readings.system_battery().is_some(),
            Section::Temperatures => {
                readings.hottest().is_some() || readings.cpu.temperature_celsius.is_some()
            }
            Section::Fans => readings.fastest_fan().is_some(),
        })
        .collect()
}

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a popup metric id is well formed")
}

fn entity_values<'a>(
    sample: &'a Sample,
    kind: EntityKind,
    id: &'a str,
) -> impl Iterator<Item = f64> + 'a {
    let id = metric(id);
    sample
        .entities
        .iter()
        .filter(move |entity| entity.kind == kind)
        .filter_map(move |entity| entity.metrics.get(&id).and_then(|o| o.as_f64()))
}

fn sum(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |total, value| Some(total.unwrap_or(0.0) + value))
}

fn max(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |peak: Option<f64>, value| {
        Some(peak.map_or(value, |peak| peak.max(value)))
    })
}

/// One stored sample's point on a section's sparkline.
pub(crate) fn sample_value(sample: &Sample, section: Section) -> Option<f64> {
    match section {
        Section::Cpu => sample
            .value_of(&metric("cpu.utilization.busy"))
            .map(|v| v * 100.0),
        Section::Memory => {
            let total = sample.value_of(&metric("memory.total"))?;
            let available = sample.value_of(&metric("memory.available"))?;
            (total > 0.0).then(|| (total - available).max(0.0) / total * 100.0)
        }
        Section::Gpu => {
            max(entity_values(sample, EntityKind::Gpu, "gpu.utilization")).map(|v| v * 100.0)
        }
        Section::Disk => {
            sum(
                entity_values(sample, EntityKind::BlockDevice, "storage.read.bytes.rate").chain(
                    entity_values(sample, EntityKind::BlockDevice, "storage.write.bytes.rate"),
                ),
            )
        }
        Section::Network => {
            let rx = metric("network.rx.bytes.rate");
            let tx = metric("network.tx.bytes.rate");
            sum(sample
                .entities
                .iter()
                .filter(|e| e.kind == EntityKind::NetworkInterface && e.key != "lo")
                .flat_map(|e| {
                    [e.metrics.get(&rx), e.metrics.get(&tx)]
                        .into_iter()
                        .flatten()
                        .filter_map(|o| o.as_f64())
                }))
        }
        Section::Battery => entity_values(sample, EntityKind::Battery, "battery.charge")
            .next()
            .map(|v| v * 100.0),
        Section::Temperatures => max(entity_values(
            sample,
            EntityKind::Sensor,
            "sensor.temperature",
        ))
        .or_else(|| sample.value_of(&metric("cpu.package.temperature"))),
        Section::Fans => max(entity_values(
            sample,
            EntityKind::Sensor,
            "sensor.fan.speed",
        )),
    }
}

/// A section's sparkline over the last `span_seconds`, one slot per stored
/// resolution period, with a gap wherever nothing was recorded.
pub(crate) fn history_line(
    samples: &[Sample],
    section: Section,
    now_ms: u64,
    span_seconds: u64,
    resolution_seconds: u64,
) -> Vec<Option<f64>> {
    let slots = (span_seconds / resolution_seconds.max(1)).clamp(2, 300) as usize;
    let span_ms = span_seconds.max(1) * 1000;
    let start = now_ms.saturating_sub(span_ms);
    let width = span_ms as f64 / slots as f64;
    let mut sums = vec![(0.0_f64, 0_u32); slots];
    for sample in samples {
        if sample.wall_unix_ms < start || sample.wall_unix_ms > now_ms {
            continue;
        }
        let Some(value) = sample_value(sample, section) else {
            continue;
        };
        let slot = (((sample.wall_unix_ms - start) as f64 / width) as usize).min(slots - 1);
        sums[slot].0 += value;
        sums[slot].1 += 1;
    }
    sums.into_iter()
        .map(|(total, count)| (count > 0).then(|| total / count as f64))
        .collect()
}

/// A sparkline's top: percentages stop at 100, everything else at a round
/// number above its peak.
pub(crate) fn line_top(section: Section, values: &[Option<f64>]) -> f64 {
    let peak = values.iter().flatten().copied().fold(0.0_f64, f64::max);
    match section {
        Section::Cpu | Section::Memory | Section::Gpu | Section::Battery => 100.0,
        Section::Temperatures => better_ui::fluent::chart::nice_maximum(peak, 100.0),
        Section::Fans => better_ui::fluent::chart::nice_maximum(peak, 1000.0),
        Section::Disk | Section::Network => crate::tables::byte_axis(peak, 1024.0),
    }
}

/// The processes using the most of a section's resource, with the figure to
/// write beside each. `processors` turns a process's processor time into a
/// share of the whole machine.
pub(crate) fn top_processes(
    facts: &[ProcessFacts],
    section: Section,
    processors: Option<f64>,
    limit: usize,
) -> Vec<(String, String)> {
    let score = |p: &ProcessFacts| -> Option<f64> {
        match section {
            Section::Cpu => p.cpu_utilization.any_value().copied(),
            Section::Memory => p.memory_resident.any_value().map(|v| *v as f64),
            Section::Gpu => p.gpu_utilization.any_value().copied(),
            Section::Disk => match (p.read_rate.any_value(), p.write_rate.any_value()) {
                (Some(r), Some(w)) => Some(r + w),
                (one, other) => one.or(other).copied(),
            },
            Section::Network => p.network_rate(),
            Section::Battery => p.energy_power.any_value().copied(),
            Section::Temperatures | Section::Fans => None,
        }
    };
    // Processes of one program are added together under its name, so five
    // compiler processes read as one line rather than five.
    let mut by_name: Vec<(String, f64, u32)> = Vec::new();
    for process in facts {
        let Some(value) = score(process).filter(|v| *v > 0.0) else {
            continue;
        };
        let name = process.display_name();
        match by_name.iter_mut().find(|(n, _, _)| *n == name) {
            Some(entry) => entry.1 += value,
            None => by_name.push((name, value, process.pid)),
        }
    }
    by_name.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    by_name
        .into_iter()
        .take(limit)
        .map(|(name, value, _)| {
            let figure = match section {
                Section::Cpu => share(value / processors.unwrap_or(1.0).max(1.0)),
                Section::Memory => bytes(value),
                Section::Gpu => share(value.min(1.0)),
                Section::Disk | Section::Network => rate(value),
                Section::Battery => watts(value),
                Section::Temperatures | Section::Fans => String::new(),
            };
            (name, figure)
        })
        .collect()
}

/// `--at X,Y`, the point the tray icon was clicked at.
pub(crate) fn parse_point(text: &str) -> Option<(i32, i32)> {
    let (x, y) = text.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Where the popup goes on a display: its top-right corner, inset by a
/// margin and kept under a top bar.
pub(crate) fn placement(
    display_origin: (f32, f32),
    display_size: (f32, f32),
    popup: (f32, f32),
) -> (f32, f32) {
    const MARGIN: f32 = 8.0;
    const TOP_BAR: f32 = 32.0;
    let x = display_origin.0 + (display_size.0 - popup.0 - MARGIN).max(0.0);
    let y = display_origin.1 + TOP_BAR.min((display_size.1 - popup.1).max(0.0));
    (x, y)
}

/// The popup's view.
pub(crate) struct PopupView {
    locale: Locale,
    readings: Readings,
    facts: Vec<ProcessFacts>,
    samples: Vec<Sample>,
    fetched_at_ms: u64,
    resolution_seconds: u64,
    focus: FocusHandle,
    /// Whether the popup has had focus, so losing it can close it.
    was_active: bool,
    _link: smol::channel::Sender<LinkRequest>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PopupView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = crate::app::language_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| Locale::parse(&text))
            .unwrap_or(Locale::System);
        let appearance = better_ui::fluent::theme::Appearance::default_path("monitor")
            .map(|path| better_ui::fluent::theme::Appearance::load(&path))
            .unwrap_or_default();
        let follow = appearance.apply_and_follow(window, cx);
        let (request_sender, request_receiver) = smol::channel::unbounded::<LinkRequest>();
        let (update_sender, update_receiver) = smol::channel::unbounded::<LinkUpdate>();
        link::spawn_with(request_receiver, update_sender, Some(POPUP_TOP as u32 + 2));
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(update) = update_receiver.recv().await {
                if this.update(cx, |view, cx| view.adopt(update, cx)).is_err() {
                    break;
                }
            }
        });
        let history_sender = request_sender.clone();
        let history = cx.spawn(async move |_, cx| {
            loop {
                if history_sender
                    .send(LinkRequest::History {
                        seconds: POPUP_HISTORY_SECONDS,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
            }
        });
        // Losing focus closes the popup, as a flyout does. GPUI reports a
        // Wayland window active once it has keyboard focus, which can come
        // after the window opens, so only a popup that has been active closes.
        let observe = cx.observe_window_activation(window, |view: &mut Self, window, cx| {
            if window.is_window_active() {
                view.was_active = true;
            } else if view.was_active {
                cx.quit();
            }
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            locale,
            readings: Readings::default(),
            facts: Vec::new(),
            samples: Vec::new(),
            fetched_at_ms: 0,
            resolution_seconds: 5,
            focus,
            was_active: false,
            _link: request_sender,
            _tasks: vec![pump, history],
            _subscriptions: std::iter::once(observe).chain(follow).collect(),
        }
    }

    fn adopt(&mut self, update: LinkUpdate, cx: &mut Context<Self>) {
        match update {
            LinkUpdate::Status(status) => {
                if let Some(round) = &status.latest_round {
                    let now = status.now_unix_ms as f64 / 1000.0;
                    self.readings = Readings::from_reports(round, now);
                    self.facts = round
                        .iter()
                        .find(|report| report.collector.as_str() == "linux.process")
                        .map(monitor_views::ProcessFacts::from_report)
                        .unwrap_or_default();
                }
            }
            LinkUpdate::History(view) => {
                self.resolution_seconds = view.document.resolution_seconds.max(1);
                self.fetched_at_ms = monitor_service::now_unix_ms();
                self.samples = view.document.slice.samples;
            }
            _ => {}
        }
        cx.notify();
    }

    fn figure(&self, section: Section) -> (String, String) {
        let c = copy(self.locale);
        let r = &self.readings;
        let missing = || c.not_available.to_string();
        match section {
            Section::Cpu => (
                r.cpu.utilization.map(share).unwrap_or_else(missing),
                r.cpu
                    .speed_hz
                    .map(|hz| i18n::sensor_value("frequency", hz))
                    .unwrap_or_default(),
            ),
            Section::Memory => (
                r.memory
                    .in_use_fraction()
                    .map(share)
                    .unwrap_or_else(missing),
                match (r.memory.in_use(), r.memory.total) {
                    (Some(used), Some(total)) => {
                        i18n::of_total(&bytes(used as f64), &bytes(total as f64), self.locale)
                    }
                    _ => String::new(),
                },
            ),
            Section::Gpu => {
                let gpu = r.primary_gpu();
                (
                    gpu.and_then(|g| g.utilization)
                        .map(share)
                        .unwrap_or_else(missing),
                    gpu.map(|g| {
                        let mut parts = Vec::new();
                        if let Some(t) = g.temperature_celsius {
                            parts.push(format!("{t:.0} °C"));
                        }
                        if let Some(w) = g.power_watts {
                            parts.push(watts(w));
                        }
                        parts.join(" · ")
                    })
                    .unwrap_or_default(),
                )
            }
            Section::Disk => {
                let (read, write) = r.disk_total();
                (
                    match (read, write) {
                        (Some(a), Some(b)) => rate(a + b),
                        (a, b) => a.or(b).map(rate).unwrap_or_else(missing),
                    },
                    i18n::read_write(read.map(rate), write.map(rate), self.locale),
                )
            }
            Section::Network => {
                let (receive, send) = r.network_total();
                (
                    match (receive, send) {
                        (Some(a), Some(b)) => rate(a + b),
                        (a, b) => a.or(b).map(rate).unwrap_or_else(missing),
                    },
                    i18n::receive_send_short(receive.map(rate), send.map(rate), self.locale),
                )
            }
            Section::Battery => {
                let battery = r.system_battery();
                (
                    battery
                        .and_then(|b| b.charge)
                        .map(share)
                        .unwrap_or_else(missing),
                    battery
                        .map(|b| i18n::battery_line(b, self.locale))
                        .unwrap_or_default(),
                )
            }
            Section::Temperatures => {
                let hottest = r.hottest();
                (
                    hottest
                        .and_then(|s| s.value)
                        .or(r.cpu.temperature_celsius)
                        .map(|t| format!("{t:.0} °C"))
                        .unwrap_or_else(missing),
                    hottest
                        .map(|s| i18n::sensor_title(s, self.locale))
                        .unwrap_or_default(),
                )
            }
            Section::Fans => (
                r.fastest_fan()
                    .map(|rpm| i18n::sensor_value("fan", rpm))
                    .unwrap_or_else(missing),
                String::new(),
            ),
        }
    }

    fn title(&self, section: Section) -> &'static str {
        let c = copy(self.locale);
        match section {
            Section::Cpu => c.cpu,
            Section::Memory => c.memory,
            Section::Gpu => c.gpu,
            Section::Disk => c.disk,
            Section::Network => c.network,
            Section::Battery => c.battery,
            Section::Temperatures => c.temperatures,
            Section::Fans => c.fans,
        }
    }

    /// The lines under a section: the busiest processes, or for temperatures
    /// and fans each reading.
    fn lines(&self, section: Section) -> Vec<(String, String)> {
        let r = &self.readings;
        match section {
            Section::Temperatures => {
                let mut temps: Vec<_> = r
                    .sensors_of("temperature")
                    .filter(|s| s.value.is_some())
                    .collect();
                temps.sort_by(|a, b| b.value.unwrap_or(0.0).total_cmp(&a.value.unwrap_or(0.0)));
                temps
                    .into_iter()
                    .take(POPUP_TOP)
                    .map(|s| {
                        (
                            i18n::sensor_title(s, self.locale),
                            i18n::sensor_value("temperature", s.value.unwrap_or(0.0)),
                        )
                    })
                    .collect()
            }
            Section::Fans => r
                .sensors_of("fan")
                .take(POPUP_TOP)
                .map(|s| {
                    (
                        i18n::sensor_title(s, self.locale),
                        s.value
                            .map(|v| i18n::sensor_value("fan", v))
                            .unwrap_or_default(),
                    )
                })
                .collect(),
            Section::Battery if r.on_battery != Some(true) => Vec::new(),
            _ => top_processes(
                &self.facts,
                section,
                r.cpu.logical.map(|n| n as f64),
                POPUP_TOP,
            ),
        }
    }

    fn section(&self, index: usize, section: Section, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let (figure, detail) = self.figure(section);
        let values = history_line(
            &self.samples,
            section,
            self.fetched_at_ms,
            POPUP_HISTORY_SECONDS,
            self.resolution_seconds,
        );
        let top = line_top(section, &values);
        let capacity = values.len().max(2);
        let lines = self.lines(section);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::MD)
            .bg(p.card)
            .border_1()
            .border_color(p.card_stroke)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::MD)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        Icon::new(section.icon())
                                            .size_4()
                                            .text_color(p.text_secondary),
                                    )
                                    .child(label(self.title(section), CAPTION, p.text_secondary)),
                            )
                            .child(label(figure, BODY_STRONG, p.text_primary))
                            .when(!detail.is_empty(), |el| {
                                el.child(label(detail, CAPTION, p.text_secondary).truncate())
                            }),
                    )
                    .child(
                        div()
                            .w(px(140.0))
                            .h(px(40.0))
                            .flex_none()
                            .child(LineChart::sparkline(
                                ElementId::named_usize("popup-line", index),
                                vec![ChartSeries::new(values, section.hue())],
                                capacity,
                                top,
                            )),
                    ),
            )
            .children(lines.into_iter().map(|(name, value)| {
                div()
                    .flex()
                    .justify_between()
                    .gap(spacing::MD)
                    .child(label(name, CAPTION, p.text_secondary).min_w_0().truncate())
                    .child(label(value, CAPTION, p.text_primary).flex_none())
                    .into_any_element()
            }))
            .into_any_element()
    }
}

impl Render for PopupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let sections = sections(&self.readings);
        let blocks: Vec<AnyElement> = sections
            .iter()
            .enumerate()
            .map(|(index, section)| self.section(index, *section, cx))
            .collect();
        div()
            .id("popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.quit();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window_background)
            .border_1()
            .border_color(p.card_stroke)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(spacing::LG)
                    .pt(spacing::MD)
                    .pb(spacing::SM)
                    .child(label(c.app_name, BODY, p.text_primary))
                    .child(label(c.popup_last_minutes, CAPTION, p.text_secondary)),
            )
            .child(
                div()
                    .id("popup-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(spacing::MD)
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .when(blocks.is_empty(), |el| {
                        el.child(label(c.waiting_for_first_reading, BODY, p.text_secondary))
                    })
                    .children(blocks),
            )
            .child(div().p(spacing::MD).flex().justify_end().child(
                Button::accent("open-full", c.open_full_window).on_click(|_, cx| {
                    if let Ok(program) = std::env::current_exe() {
                        let _ = std::process::Command::new(program)
                            .stdin(std::process::Stdio::null())
                            .spawn();
                    }
                    cx.quit();
                }),
            ))
    }
}

/// Open the popup window. `at` is where the tray icon was clicked.
pub(crate) fn open(at: Option<(i32, i32)>, cx: &mut App) {
    let size = size(px(POPUP_WIDTH), px(POPUP_HEIGHT));
    let display = at
        .and_then(|(x, y)| {
            let point = point(px(x as f32), px(y as f32));
            cx.displays()
                .into_iter()
                .find(|display| display.bounds().contains(&point))
        })
        .or_else(|| cx.primary_display());
    let origin = display
        .map(|display| {
            let bounds = display.bounds();
            let (x, y) = placement(
                (f32::from(bounds.origin.x), f32::from(bounds.origin.y)),
                (f32::from(bounds.size.width), f32::from(bounds.size.height)),
                (POPUP_WIDTH, POPUP_HEIGHT),
            );
            point(px(x), px(y))
        })
        .unwrap_or_else(|| point(px(0.0), px(0.0)));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size))),
        kind: WindowKind::PopUp,
        focus: true,
        is_resizable: false,
        // A title of its own, so a compositor rule (and a person reading a
        // window list) can tell the popup from the window.
        titlebar: Some(TitlebarOptions {
            title: Some(POPUP_TITLE.into()),
            ..Default::default()
        }),
        ..better_ui::window_chrome::overlay_window_options("io.betteros.Monitor")
    };
    cx.spawn(async move |cx| {
        let _ = cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| PopupView::new(window, cx));
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{
        EntityKind, ProcessFacts, Readings, Sample, Section, history_line, line_top, metric,
        parse_point, placement, sections, top_processes,
    };
    use monitor_core::{MetricSet, Observation};
    use monitor_store::EntitySample;

    fn sample(at_ms: u64, busy: f64, gpu: f64, fan: f64) -> Sample {
        let mut metrics = MetricSet::new();
        metrics.insert(metric("cpu.utilization.busy"), Observation::float(busy));
        let mut gpu_metrics = MetricSet::new();
        gpu_metrics.insert(metric("gpu.utilization"), Observation::float(gpu));
        let mut fan_metrics = MetricSet::new();
        fan_metrics.insert(metric("sensor.fan.speed"), Observation::float(fan));
        Sample {
            wall_unix_ms: at_ms,
            monotonic_ns: 0,
            rounds: 5,
            metrics,
            entities: vec![
                EntitySample {
                    kind: EntityKind::Gpu,
                    key: "0000:63:00.0".into(),
                    metrics: gpu_metrics,
                },
                EntitySample {
                    kind: EntityKind::Sensor,
                    key: "asus/fan1".into(),
                    metrics: fan_metrics,
                },
            ],
            processes: Vec::new(),
            collectors: Vec::new(),
        }
    }

    #[test]
    fn stored_samples_become_a_line_with_gaps_where_nothing_was_recorded() {
        let now = 1_000_000;
        let samples = vec![
            sample(now - 290_000, 0.5, 0.2, 3000.0),
            sample(now - 5_000, 0.1, 0.4, 3600.0),
        ];
        let line = history_line(&samples, Section::Cpu, now, 300, 5);
        assert_eq!(line.len(), 60);
        assert_eq!(line[2], Some(50.0));
        assert_eq!(line[59], Some(10.0));
        assert_eq!(line.iter().flatten().count(), 2, "everything else is a gap");
        let gpu = history_line(&samples, Section::Gpu, now, 300, 5);
        assert_eq!(gpu[59], Some(40.0));
        let fans = history_line(&samples, Section::Fans, now, 300, 5);
        assert_eq!(fans[59], Some(3600.0));
        assert_eq!(line_top(Section::Fans, &fans), 5000.0);
    }

    #[test]
    fn a_machine_shows_only_the_sections_it_has() {
        let mut readings = Readings {
            has_cpu: true,
            has_memory: true,
            ..Readings::default()
        };
        assert_eq!(sections(&readings), vec![Section::Cpu, Section::Memory]);
        readings.gpus.push(crate::charts::GpuReadings::default());
        readings.batteries.push(crate::charts::BatteryReadings {
            key: "BAT1".into(),
            ..Default::default()
        });
        readings.sensors.push(crate::charts::SensorReadings {
            key: "asus/fan1".into(),
            kind: "fan".into(),
            value: Some(3600.0),
            ..Default::default()
        });
        assert_eq!(
            sections(&readings),
            vec![
                Section::Cpu,
                Section::Memory,
                Section::Gpu,
                Section::Battery,
                Section::Fans
            ]
        );
    }

    #[test]
    fn the_busiest_processes_are_named_with_their_figure() {
        let mut a = ProcessFacts::synthetic(1, "firefox");
        a.cpu_utilization = monitor_views::field::Field::Value(4.8);
        let mut b = ProcessFacts::synthetic(2, "code");
        b.cpu_utilization = monitor_views::field::Field::Value(2.4);
        let c = ProcessFacts::synthetic(3, "idle");
        let mut d = ProcessFacts::synthetic(4, "code");
        d.cpu_utilization = monitor_views::field::Field::Value(4.8);
        let top = top_processes(&[b, a, c, d], Section::Cpu, Some(24.0), 3);
        assert_eq!(
            top,
            vec![
                ("code".to_string(), "30%".to_string()),
                ("firefox".to_string(), "20%".to_string())
            ]
        );
    }

    #[test]
    fn the_popup_sits_in_the_top_right_of_the_display() {
        assert_eq!(
            placement((0.0, 0.0), (1920.0, 1080.0), (400.0, 760.0)),
            (1512.0, 32.0)
        );
        assert_eq!(
            placement((1920.0, 0.0), (1280.0, 720.0), (400.0, 760.0)),
            (2792.0, 0.0)
        );
        assert_eq!(parse_point("1500,12"), Some((1500, 12)));
        assert_eq!(parse_point("garbage"), None);
    }
}

//! The History, Events, and System information pages.
//!
//! These three read what was stored rather than what is happening now, and
//! they share one rule that shapes all of them: an empty page is never drawn
//! as if it meant "nothing happened". A period with no samples is a gap in
//! the line and is listed as one, a machine with one inventory capture is
//! told it has nothing to compare against, and a window that is not recording
//! says so before it shows a single number.
//!
//! What each chart plots is decided by [`history_series`], which knows no
//! GPUI and is tested directly.

use better_ui::fluent::chart::{ChartHue, ChartSeries, LineChart, nice_maximum, peak};
use better_ui::fluent::settings_card::{SettingsExpander, SettingsRow};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem};
use better_ui::fluent::{BODY, BODY_STRONG, CAPTION, label, palette, spacing};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::IconName;
use monitor_core::{EntityKind, MetricId};
use monitor_store::{GapReason, Incident, Sample, Sensitivity};

use crate::app::MonitorApp;
use crate::i18n::{self, Copy, copy};
use crate::layout::card_columns;
use crate::shell::{card, grid, note_card, property_row, section_title, stat};
use crate::tables::{byte_axis, bytes, rate};

/// The charts on the History page, in the order it draws them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryChart {
    /// Processor utilization, in percent.
    Processor,
    /// Memory in use, in bytes.
    Memory,
    /// Every disk's reads and writes together, in bytes per second.
    Disk,
    /// Every adapter's traffic in both directions, in bytes per second.
    Network,
    /// The highest share of time any program waited for the processor,
    /// memory, or disk, in percent.
    Waiting,
}

impl HistoryChart {
    pub(crate) const ALL: [HistoryChart; 5] = [
        HistoryChart::Processor,
        HistoryChart::Memory,
        HistoryChart::Disk,
        HistoryChart::Network,
        HistoryChart::Waiting,
    ];

    /// The metrics a chart reads, so a renamed metric breaks a test rather
    /// than blanking a chart.
    pub(crate) fn metrics(self) -> &'static [&'static str] {
        match self {
            HistoryChart::Processor => &["cpu.utilization.busy"],
            HistoryChart::Memory => &["memory.total", "memory.available"],
            HistoryChart::Disk => &["storage.read.bytes.rate", "storage.write.bytes.rate"],
            HistoryChart::Network => &["network.rx.bytes.rate", "network.tx.bytes.rate"],
            HistoryChart::Waiting => &["pressure.some.avg10"],
        }
    }

    fn hue(self) -> ChartHue {
        match self {
            HistoryChart::Processor => ChartHue::Blue,
            HistoryChart::Memory => ChartHue::Purple,
            HistoryChart::Disk => ChartHue::Green,
            HistoryChart::Network => ChartHue::Orange,
            HistoryChart::Waiting => ChartHue::Teal,
        }
    }
}

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a page metric id must be well formed")
}

fn entity_sum(sample: &Sample, kind: EntityKind, ids: &[&str], skip: &[&str]) -> Option<f64> {
    let wanted: Vec<MetricId> = ids.iter().map(|id| metric(id)).collect();
    let mut total = None;
    for entity in sample
        .entities
        .iter()
        .filter(|entity| entity.kind == kind && !skip.contains(&entity.key.as_str()))
    {
        for id in &wanted {
            if let Some(value) = entity.metrics.get(id).and_then(|o| o.as_f64()) {
                total = Some(total.unwrap_or(0.0) + value);
            }
        }
    }
    total
}

/// One chart's values, one per stored sample, `None` where the sample had no
/// reading. A missing reading is a gap, never a zero.
pub(crate) fn history_series(samples: &[Sample], chart: HistoryChart) -> Vec<Option<f64>> {
    samples
        .iter()
        .map(|sample| match chart {
            HistoryChart::Processor => sample
                .value_of(&metric("cpu.utilization.busy"))
                .map(|v| v * 100.0),
            HistoryChart::Memory => {
                let total = sample.value_of(&metric("memory.total"))?;
                let available = sample.value_of(&metric("memory.available"))?;
                Some((total - available).max(0.0))
            }
            HistoryChart::Disk => entity_sum(
                sample,
                EntityKind::BlockDevice,
                HistoryChart::Disk.metrics(),
                &[],
            ),
            HistoryChart::Network => entity_sum(
                sample,
                EntityKind::NetworkInterface,
                HistoryChart::Network.metrics(),
                &["lo"],
            ),
            HistoryChart::Waiting => sample
                .entities
                .iter()
                .filter(|entity| entity.kind == EntityKind::PressureResource)
                .filter_map(|entity| {
                    entity
                        .metrics
                        .get(&metric("pressure.some.avg10"))
                        .and_then(|o| o.as_f64())
                })
                .fold(None, |peak: Option<f64>, value| {
                    Some(peak.map_or(value, |peak| peak.max(value)))
                }),
        })
        .collect()
}

/// One chart's values placed on the clock: the span is cut into equal
/// slots ending at `now_ms`, each slot holds the mean of the readings that
/// fall in it, and a slot with no reading stays `None`, so a time nothing
/// was recorded is a gap where it happened rather than a squeezed-out
/// stretch of line.
pub(crate) fn binned(
    samples: &[Sample],
    chart: HistoryChart,
    now_ms: u64,
    span_seconds: u64,
    resolution_seconds: u64,
) -> Vec<Option<f64>> {
    let slots = (span_seconds / resolution_seconds.max(1)).clamp(2, 720) as usize;
    let span_ms = span_seconds.max(1) * 1000;
    let start = now_ms.saturating_sub(span_ms);
    let width = (span_ms as f64 / slots as f64).max(1.0);
    let mut sums = vec![(0.0_f64, 0_u32); slots];
    for (sample, value) in samples.iter().zip(history_series(samples, chart)) {
        let (Some(value), true) = (value, sample.wall_unix_ms >= start) else {
            continue;
        };
        let slot = (((sample.wall_unix_ms - start) as f64 / width) as usize).min(slots - 1);
        sums[slot].0 += value;
        sums[slot].1 += 1;
    }
    sums.into_iter()
        .map(|(sum, count)| (count > 0).then(|| sum / count as f64))
        .collect()
}

/// A History reply with its charts already worked out, built on the link's
/// thread when the reply arrives so drawing only reads it. The span ends at
/// the moment the history was fetched, not at whatever time it is drawn.
#[derive(Debug)]
pub(crate) struct HistoryView {
    pub(crate) document: monitor_ipc::HistoryDocument,
    pub(crate) span_seconds: u64,
    pub(crate) charts: Vec<(HistoryChart, Vec<Option<f64>>)>,
    pub(crate) memory_total: Option<f64>,
}

impl HistoryView {
    pub(crate) fn build(
        document: monitor_ipc::HistoryDocument,
        fetched_at_ms: u64,
        span_seconds: u64,
    ) -> Self {
        let samples = &document.slice.samples;
        let resolution = document.resolution_seconds;
        let charts = HistoryChart::ALL
            .iter()
            .map(|chart| {
                (
                    *chart,
                    binned(samples, *chart, fetched_at_ms, span_seconds, resolution),
                )
            })
            .collect();
        let memory_total = memory_total(samples);
        Self {
            document,
            span_seconds,
            charts,
            memory_total,
        }
    }
}

/// The largest memory total any sample recorded, for the memory chart's top.
pub(crate) fn memory_total(samples: &[Sample]) -> Option<f64> {
    samples
        .iter()
        .filter_map(|sample| sample.value_of(&metric("memory.total")))
        .fold(None, |peak: Option<f64>, value| {
            Some(peak.map_or(value, |peak| peak.max(value)))
        })
}

/// The logical processor count a stored moment recorded.
pub(crate) fn snapshot_processors(sample: &Sample) -> Option<f64> {
    sample
        .value_of(&metric("cpu.logical.count"))
        .filter(|count| *count > 0.0)
}

/// A process's processor time as a share of every logical processor, or
/// `None` when either the reading or the count is missing.
pub(crate) fn machine_share(value: Option<f64>, processors: Option<f64>) -> Option<String> {
    Some(crate::tables::share(value? / processors?))
}

pub(crate) fn gap_reason_label(reason: GapReason, c: &'static Copy) -> &'static str {
    match reason {
        GapReason::ServiceStopped => c.gap_service_stopped,
        GapReason::MissedCadence => c.gap_missed_cadence,
        GapReason::InterruptedWrite => c.gap_interrupted_write,
        GapReason::Retention => c.gap_retention,
    }
}

impl MonitorApp {
    pub(crate) fn history_page(&mut self, available: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let spans = [900u64, 3_600, 21_600];
        let picker = SelectorBar::new(
            "history-span",
            vec![
                SelectorItem::new(c.span_15_minutes),
                SelectorItem::new(c.span_1_hour),
                SelectorItem::new(c.span_6_hours),
            ],
            spans.iter().position(|span| *span == self.history_seconds),
        )
        .on_select(Self::act_with(cx, move |this, index: usize, _, cx| {
            if let Some(span) = spans.get(index) {
                this.set_history_span(*span, cx);
            }
        }));
        let shown_span = self
            .history
            .as_ref()
            .map(|view| view.span_seconds)
            .unwrap_or(self.history_seconds);
        let span_words = match shown_span {
            900 => c.span_15_minutes,
            3_600 => c.span_1_hour,
            _ => c.span_6_hours,
        };

        let body = match self.history.as_ref().map(|view| (view, &view.document)) {
            None => note_card(c.history, c.loading, cx),
            Some((_, history)) if history.slice.samples.is_empty() => {
                let mut empty = card(p)
                    .child(label(c.no_history_yet, BODY_STRONG, p.text_primary))
                    .child(label(c.no_history_explained, BODY, p.text_secondary));
                for gap in history.slice.gaps.iter().take(6) {
                    empty = empty.child(label(
                        i18n::gap_sentence(
                            gap_reason_label(gap.reason, c),
                            gap.duration_ms(),
                            self.locale,
                        ),
                        CAPTION,
                        p.text_secondary,
                    ));
                }
                empty.into_any_element()
            }
            Some((view, history)) => {
                let samples = &history.slice.samples;
                let charts: Vec<AnyElement> = view
                    .charts
                    .iter()
                    .map(|(chart, values)| {
                        let values = values.clone();
                        let slots = values.len();
                        let series = vec![ChartSeries::new(values, chart.hue())];
                        let (top, top_label, caption) = match chart {
                            HistoryChart::Processor => {
                                (100.0, "100%".to_string(), c.history_processor)
                            }
                            HistoryChart::Memory => {
                                let total = view.memory_total.unwrap_or(1.0);
                                (total, bytes(total), c.history_memory)
                            }
                            HistoryChart::Disk => {
                                let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0 * 1024.0);
                                (top, rate(top), c.history_disk)
                            }
                            HistoryChart::Network => {
                                let top = byte_axis(peak(&series).unwrap_or(0.0), 1024.0);
                                (top, rate(top), c.history_network)
                            }
                            HistoryChart::Waiting => {
                                let top =
                                    nice_maximum(peak(&series).unwrap_or(0.0), 10.0).min(100.0);
                                (top, format!("{top:.0}%"), c.history_waiting)
                            }
                        };
                        card(p)
                            .child(
                                LineChart::new(
                                    ElementId::named_usize("history-chart", *chart as usize),
                                    series,
                                    slots,
                                    top,
                                )
                                .height(px(140.0))
                                .caption(caption)
                                .maximum_label(top_label)
                                .span_label(span_words)
                                .minimum_label("0"),
                            )
                            .into_any_element()
                    })
                    .collect();
                let gaps = &history.slice.gaps;
                let mut gap_card = card(p).child(label(
                    if gaps.is_empty() {
                        c.no_gaps
                    } else {
                        c.gaps_title
                    },
                    BODY_STRONG,
                    p.text_primary,
                ));
                for gap in gaps.iter().take(12) {
                    gap_card = gap_card.child(label(
                        i18n::gap_sentence(
                            gap_reason_label(gap.reason, c),
                            gap.duration_ms(),
                            self.locale,
                        ),
                        BODY,
                        p.text_secondary,
                    ));
                }
                let mut summary = div()
                    .flex()
                    .flex_wrap()
                    .gap(spacing::XL)
                    .child(stat(c.stored_samples, samples.len().to_string(), false, p))
                    .child(stat(
                        c.one_sample_every,
                        i18n::seconds(history.resolution_seconds, self.locale),
                        false,
                        p,
                    ));
                if let Some(status) = &self.status {
                    summary = summary
                        .child(stat(
                            c.kept_for,
                            i18n::seconds(status.retention.window_seconds, self.locale),
                            false,
                            p,
                        ))
                        .child(stat(
                            c.disk_budget,
                            bytes(status.retention.disk_budget_bytes as f64),
                            false,
                            p,
                        ));
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::LG)
                    .child(summary)
                    .when(history.slice.truncated, |page| {
                        page.child(label(c.history_truncated, CAPTION, p.caution))
                    })
                    .child(grid(
                        if card_columns(available) >= 3 { 2 } else { 1 },
                        charts,
                    ))
                    .child(gap_card)
                    .into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .child(label(c.history_subtitle, BODY, p.text_secondary))
            .child(picker)
            .child(body)
            .into_any_element()
    }

    pub(crate) fn events_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let incidents = self.incidents.clone();
        let intro = card(p)
            .child(label(c.events_intro_title, BODY_STRONG, p.text_primary))
            .child(label(
                if self.can_mark() {
                    c.events_intro
                } else {
                    c.mark_unavailable
                },
                BODY,
                p.text_secondary,
            ));
        let mut page = div().flex().flex_col().gap(spacing::LG).child(intro);
        if incidents.is_empty() {
            page = page.child(note_card(c.no_events_yet, c.no_events_explained, cx));
        } else {
            page = page.child(section_title(c.marked_moments, p));
            for incident in incidents.iter().rev() {
                page = page.child(self.incident_card(incident, cx));
            }
        }
        page.into_any_element()
    }

    fn incident_card(&self, incident: &Incident, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let latest = self.last_marked == Some(incident.id);
        let when = i18n::ago(self.now_unix_ms, incident.marked_at_unix_ms, self.locale);
        let shifts: Vec<(String, String)> = incident
            .largest_shifts(8)
            .into_iter()
            .filter_map(|(id, shift)| {
                i18n::shift_sentence(id, shift.baseline, shift.at_marker, self.locale)
            })
            .take(4)
            .collect();
        let mut body = card(p)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing::MD)
                    .child(label(
                        i18n::marked_title(&when, latest, self.locale),
                        BODY_STRONG,
                        p.text_primary,
                    ))
                    .child(label(
                        i18n::window_sentence(
                            incident.window.before_seconds,
                            incident.window.after_seconds,
                            self.locale,
                        ),
                        CAPTION,
                        p.text_secondary,
                    )),
            )
            .when_some(incident.note.clone(), |card, note| {
                card.child(label(note, BODY, p.text_primary))
            });
        body = body.child(label(c.what_changed, CAPTION, p.text_secondary));
        if shifts.is_empty() {
            body = body.child(label(c.baseline_none, BODY, p.text_secondary));
        }
        for (name, change) in shifts {
            body = body.child(property_row(name, change, false, p));
        }
        body = body.child(label(c.busiest_then, CAPTION, p.text_secondary));
        let processors = snapshot_processors(&incident.snapshot).or(self.processors());
        for process in incident.snapshot.processes.iter().take(5) {
            let share = machine_share(process.cpu_utilization, processors);
            body = body.child(property_row(
                process.name.clone(),
                share.clone().unwrap_or_else(|| c.not_collected.to_string()),
                share.is_none(),
                p,
            ));
        }
        body.into_any_element()
    }

    pub(crate) fn system_info_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let c = copy(self.locale);
        let Some(inventory) = &self.inventory else {
            return note_card(c.system_info, c.inventory_never_captured, cx);
        };
        let sections = i18n::inventory_sections(inventory, self.locale);
        let mut page = div().flex().flex_col().gap(px(4.0)).max_w(px(1000.0));
        page = page.child(label(
            i18n::captures_sentence(self.inventory_captures, self.locale),
            BODY,
            p.text_secondary,
        ));
        for (index, (section, rows)) in sections.into_iter().enumerate() {
            page = page.child(section_title(section, p));
            let mut list = card(p).gap(px(0.0));
            for (name, value, sensitivity) in rows {
                let shown = if sensitivity == Sensitivity::Public {
                    value
                } else {
                    format!("{value} ({})", c.stays_on_this_computer)
                };
                list = list.child(property_row(name, shown, false, p));
            }
            page = page.child(list.id(ElementId::named_usize("inventory-section", index)));
        }

        page = page.child(section_title(c.changes_title, p));
        let changes: Vec<(String, String)> = match &self.inventory_diff {
            Some(diff) => i18n::inventory_changes(diff, self.locale),
            None => Vec::new(),
        };
        let mut expander = SettingsExpander::new(
            "inventory-changes",
            if changes.is_empty() {
                if self.inventory_captures <= 1 {
                    c.inventory_first_capture
                } else {
                    c.inventory_no_changes
                }
            } else {
                c.changes_since_last
            },
            !changes.is_empty(),
        )
        .icon(IconName::Calendar);
        for (name, change) in changes {
            expander = expander.row(SettingsRow::new(name).description(change));
        }
        page.child(expander).into_any_element()
    }
}

//! The History, Incidents, and Inventory pages.
//!
//! These three read what was stored rather than what is happening now, and
//! they share one rule that shapes all of them: an empty page is never drawn
//! as if it meant "nothing happened". A period with no samples is labelled as
//! a gap in observation, a machine with one inventory capture is told it has
//! nothing to compare against, and a window that is not recording says so
//! before it shows a single number.
//!
//! The charts are deliberately plain — a bar per sample, drawn from the
//! stored observations. A missing reading draws no bar at all rather than a
//! bar of height zero, which is the same rule the tables follow.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    *,
};
use monitor_core::MetricId;
use monitor_store::{GapReason, Incident, Sample, Sensitivity};

use crate::app::{Collection, MonitorApp};
use crate::i18n::{Copy, copy};

/// The metrics the History page charts, in the order it draws them.
///
/// A short list on purpose. The point of the page is "what was the shape of
/// the machine at 14:03", and four rows answer that; forty would not.
pub(crate) const CHARTED: [&str; 4] = [
    "cpu.utilization.busy",
    "memory.available",
    "pressure.some.avg10",
    "storage.read.bytes.rate",
];

fn metric(raw: &str) -> MetricId {
    MetricId::new(raw).expect("a page metric id must be well formed")
}

/// How a stored value is turned into a bar height, and whether there is one.
///
/// `None` means the sample had no reading, and the chart leaves a space. That
/// space is the honest drawing of an unobserved moment.
pub(crate) fn bar_fraction(values: &[Option<f64>], index: usize) -> Option<f32> {
    let value = values.get(index).copied().flatten()?;
    let peak = values
        .iter()
        .filter_map(|value| *value)
        .fold(f64::MIN, f64::max);
    if !peak.is_finite() || peak <= 0.0 {
        // Everything measured is zero. A flat floor is the truthful drawing:
        // the readings exist and they are all zero.
        return Some(0.0);
    }
    Some((value / peak).clamp(0.0, 1.0) as f32)
}

impl MonitorApp {
    fn collection_note(&self, c: &'static Copy) -> Option<(&'static str, bool)> {
        match &self.collection {
            Collection::Connecting => Some((c.collection_connecting, false)),
            // Said out loud, because "you can close this window and it will
            // still be recording" is the single most useful fact about the
            // service and is otherwise invisible.
            Collection::Service => Some((c.collection_service, false)),
            Collection::InProcess { .. } => Some((c.collection_in_process, true)),
            Collection::Unavailable { .. } => Some((c.collection_unavailable, true)),
        }
    }

    /// The banner every stored-data page carries when collection is not the
    /// service's.
    fn collection_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let (message, warn) = self.collection_note(c)?;
        Some(
            self.surface(
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .items_start()
                    .child(Icon::new(if warn {
                        IconName::TriangleAlert
                    } else {
                        IconName::Info
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .when(warn, |element| element.text_color(cx.theme().warning))
                            .child(message),
                    ),
                cx,
            ),
        )
    }

    fn failure_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = copy(self.locale);
        let detail = self.last_failure.clone()?;
        Some(
            self.surface(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(c.request_failed),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail),
                    ),
                cx,
            ),
        )
    }

    pub(crate) fn history_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let spans = [
            (c.span_15_minutes, 900u64),
            (c.span_1_hour, 3_600),
            (c.span_6_hours, 21_600),
        ];

        let picker = h_flex().min_w_0().gap_2().flex_wrap().children(
            spans
                .into_iter()
                .map(|(label, seconds)| {
                    Button::new(label)
                        .label(label)
                        .when(self.history_seconds == seconds, |button| button.primary())
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.set_history_span(seconds, cx)),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        );

        let body = match &self.history {
            None => self.surface(
                div().text_sm().child(c.no_history_yet).into_any_element(),
                cx,
            ),
            Some(history) if history.slice.samples.is_empty() => self.surface(
                v_flex()
                    .min_w_0()
                    .gap_2()
                    .child(div().text_sm().child(c.no_history_yet))
                    .children(self.gap_lines(&history.slice.gaps, c, cx))
                    .into_any_element(),
                cx,
            ),
            Some(history) => {
                let samples = history.slice.samples.clone();
                let gaps = history.slice.gaps.clone();
                let truncated = history.slice.truncated;
                let resolution = history.resolution_seconds;
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .child(
                        self.surface(
                            h_flex()
                                .min_w_0()
                                .gap_4()
                                .flex_wrap()
                                .child(self.stat(
                                    c.stored_samples,
                                    div().child(samples.len().to_string()).into_any_element(),
                                    cx,
                                ))
                                .child(self.stat(
                                    c.observation_gaps,
                                    div().child(gaps.len().to_string()).into_any_element(),
                                    cx,
                                ))
                                .child(self.stat(
                                    c.resolution,
                                    div().child(format!("{resolution} s")).into_any_element(),
                                    cx,
                                ))
                                .children(self.retention_stats(cx))
                                .into_any_element(),
                            cx,
                        ),
                    )
                    .when(truncated, |element| {
                        element.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().warning)
                                .child(c.history_truncated),
                        )
                    })
                    .children(
                        CHARTED
                            .iter()
                            .map(|id| self.chart_card(id, &samples, cx))
                            .collect::<Vec<_>>(),
                    )
                    .children(self.gap_lines(&gaps, c, cx))
                    .into_any_element()
            }
        };

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.history, c.history_subtitle))
            .children(self.collection_banner(cx))
            .children(self.failure_banner(cx))
            .child(picker)
            .child(body)
            .into_any_element()
    }

    /// The bounds the store is actually keeping to, so a short history is
    /// explained by the policy rather than looking like lost data.
    fn retention_stats(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = copy(self.locale);
        let Some(status) = &self.status else {
            return Vec::new();
        };
        let window = status.retention.window_seconds / 60;
        let budget = status.retention.disk_budget_bytes / (1024 * 1024);
        vec![
            self.stat(
                c.retention_window,
                div().child(format!("{window} min")).into_any_element(),
                cx,
            ),
            self.stat(
                c.disk_budget,
                div().child(format!("{budget} MiB")).into_any_element(),
                cx,
            ),
        ]
    }

    /// One metric, drawn as a bar per stored sample.
    fn chart_card(&self, id: &str, samples: &[Sample], cx: &mut Context<Self>) -> AnyElement {
        let wanted = metric(id);
        let values: Vec<Option<f64>> = samples
            .iter()
            .map(|sample| {
                sample.value_of(&wanted).or_else(|| {
                    // Pressure and device metrics live on entities rather than
                    // on the machine as a whole.
                    sample
                        .entities
                        .iter()
                        .find_map(|entity| entity.metrics.get(&wanted)?.as_f64())
                })
            })
            .collect();
        let measured = values.iter().filter(|value| value.is_some()).count();
        let c = copy(self.locale);

        let bars = h_flex()
            .w_full()
            .h(px(56.0))
            .gap(px(1.0))
            .items_end()
            .children(
                (0..values.len())
                    .map(|index| match bar_fraction(&values, index) {
                        // A reading, drawn to scale. A measured zero still gets
                        // a visible sliver so it is not mistaken for a gap.
                        Some(fraction) => div()
                            .flex_1()
                            .min_w(px(1.0))
                            .h(px((fraction * 52.0).max(1.0)))
                            .bg(cx.theme().primary)
                            .into_any_element(),
                        // No reading. Nothing is drawn, and the space is the
                        // point.
                        None => div().flex_1().min_w(px(1.0)).h(px(1.0)).into_any_element(),
                    })
                    .collect::<Vec<_>>(),
            );

        self.surface(
            v_flex()
                .min_w_0()
                .gap_2()
                .child(
                    h_flex()
                        .min_w_0()
                        .justify_between()
                        .gap_2()
                        .flex_wrap()
                        .child(div().text_sm().font_semibold().child(id.to_string()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} / {} {}",
                                    measured,
                                    values.len(),
                                    c.coverage_value
                                )),
                        ),
                )
                .child(bars)
                .into_any_element(),
            cx,
        )
    }

    fn gap_lines(
        &self,
        gaps: &[monitor_store::Gap],
        c: &'static Copy,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if gaps.is_empty() {
            return Vec::new();
        }
        vec![
            self.surface(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().font_semibold().child(format!(
                        "{} · {}",
                        c.observation_gaps,
                        gaps.len()
                    )))
                    .children(
                        gaps.iter()
                            .take(12)
                            .map(|gap| {
                                div()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} — {} s",
                                        gap_reason_label(gap.reason, c),
                                        gap.duration_ms() / 1_000
                                    ))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .into_any_element(),
                cx,
            ),
        ]
    }

    pub(crate) fn incidents_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let can_mark = self.can_mark();
        let incidents = self.incidents.clone();
        let last_marked = self.last_marked;

        let action = v_flex()
            .min_w_0()
            .gap_2()
            .child(
                Button::new("mark-incident")
                    .primary()
                    .label(c.mark_incident)
                    .disabled(!can_mark)
                    .on_click(cx.listener(|this, _, _, cx| this.mark_incident(cx))),
            )
            .when(!can_mark, |element| {
                element.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.mark_unavailable),
                )
            });

        let list = if incidents.is_empty() {
            self.surface(
                div().text_sm().child(c.no_incidents_yet).into_any_element(),
                cx,
            )
        } else {
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .children(
                    incidents
                        .iter()
                        .rev()
                        .map(|incident| {
                            self.incident_card(incident, last_marked == Some(incident.id), cx)
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        };

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.incidents, c.incidents_subtitle))
            .children(self.collection_banner(cx))
            .children(self.failure_banner(cx))
            .child(self.surface(action.into_any_element(), cx))
            .child(list)
            .into_any_element()
    }

    fn incident_card(
        &self,
        incident: &Incident,
        highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = copy(self.locale);
        let shifts = incident.largest_shifts(4);
        let body = v_flex()
            .min_w_0()
            .gap_2()
            .child(
                h_flex()
                    .min_w_0()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        div()
                            .font_semibold()
                            .when(highlighted, |element| {
                                element.text_color(cx.theme().primary)
                            })
                            .child(format!("#{}", incident.id)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} {} · {} {} s / {} s",
                                c.marked_at,
                                incident.marked_at_unix_ms,
                                c.incident_window,
                                incident.window.before_seconds,
                                incident.window.after_seconds
                            )),
                    ),
            )
            .when_some(incident.note.clone(), |element, note| {
                element.child(div().min_w_0().text_sm().child(note))
            })
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.largest_shifts),
            )
            .children(if shifts.is_empty() {
                vec![
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(c.baseline_none)
                        .into_any_element(),
                ]
            } else {
                shifts
                    .iter()
                    .map(|(id, shift)| {
                        h_flex()
                            .min_w_0()
                            .justify_between()
                            .gap_3()
                            .child(div().truncate().text_xs().child(id.to_string()))
                            .child(div().text_xs().child(format!(
                                "{:.3} → {:.3} ({} )",
                                shift.baseline, shift.at_marker, shift.baseline_samples
                            )))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(c.captured_processes),
            )
            .children(
                incident
                    .snapshot
                    .processes
                    .iter()
                    .take(5)
                    .map(|process| {
                        h_flex()
                            .min_w_0()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .child(format!("{} [{}]", process.name, process.pid)),
                            )
                            .child(div().text_xs().child(match process.cpu_utilization {
                                Some(value) => format!("{:.1}%", value * 100.0),
                                // No reading is not zero, and the card says so
                                // rather than printing 0.0%.
                                None => c.not_collected.to_string(),
                            }))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            );
        self.surface(body.into_any_element(), cx)
    }

    pub(crate) fn inventory_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = copy(self.locale);
        let current = match &self.inventory {
            None => self.surface(
                div()
                    .text_sm()
                    .child(c.inventory_never_captured)
                    .into_any_element(),
                cx,
            ),
            Some(inventory) => {
                let entries: Vec<AnyElement> = inventory
                    .entries
                    .iter()
                    .map(|(key, entry)| {
                        h_flex()
                            .min_w_0()
                            .justify_between()
                            .gap_3()
                            .py_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(key.clone()),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .items_center()
                                    .child(div().truncate().text_xs().child(entry.value.clone()))
                                    .when(entry.sensitivity != Sensitivity::Public, |row| {
                                        row.child(
                                            div()
                                                .px_1()
                                                .rounded(cx.theme().radius)
                                                .border_1()
                                                .border_color(cx.theme().warning)
                                                .text_color(cx.theme().warning)
                                                .text_xs()
                                                .child(c.withheld_value),
                                        )
                                    }),
                            )
                            .into_any_element()
                    })
                    .collect();
                self.surface(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} {}",
                                    c.inventory_captures, self.inventory_captures
                                )),
                        )
                        .children(entries)
                        .into_any_element(),
                    cx,
                )
            }
        };

        let changes = match &self.inventory_diff {
            None => self.surface(
                div()
                    .text_sm()
                    .child(if self.inventory_captures <= 1 {
                        c.inventory_first_capture
                    } else {
                        c.inventory_no_changes
                    })
                    .into_any_element(),
                cx,
            ),
            Some(diff) if diff.is_empty() => self.surface(
                div()
                    .text_sm()
                    .child(c.inventory_no_changes)
                    .into_any_element(),
                cx,
            ),
            Some(diff) => {
                let mut rows: Vec<AnyElement> = Vec::new();
                for (label, changes) in [
                    (c.changed, &diff.changed),
                    (c.added, &diff.added),
                    (c.removed, &diff.removed),
                ] {
                    for change in changes {
                        rows.push(
                            h_flex()
                                .min_w_0()
                                .justify_between()
                                .gap_3()
                                .py_0p5()
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .child(format!("{label} · {}", change.key)),
                                )
                                .child(div().truncate().text_xs().child(format!(
                                    "{} → {}",
                                    change.before.clone().unwrap_or_else(|| "—".into()),
                                    change.after.clone().unwrap_or_else(|| "—".into())
                                )))
                                .into_any_element(),
                        );
                    }
                }
                self.surface(
                    v_flex().min_w_0().gap_1().children(rows).into_any_element(),
                    cx,
                )
            }
        };

        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(self.page_heading(c.inventory, c.inventory_subtitle))
            .children(self.collection_banner(cx))
            .children(self.failure_banner(cx))
            .child(current)
            .child(changes)
            .into_any_element()
    }
}

pub(crate) fn gap_reason_label(reason: GapReason, c: &'static Copy) -> &'static str {
    match reason {
        GapReason::ServiceStopped => c.gap_service_stopped,
        GapReason::MissedCadence => c.gap_missed_cadence,
        GapReason::InterruptedWrite => c.gap_interrupted_write,
        GapReason::Retention => c.gap_retention,
    }
}

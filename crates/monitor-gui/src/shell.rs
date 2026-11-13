//! The pieces every page is built from, drawn with `better_ui::fluent`.
//!
//! A page is a Title, the commands that act on it, the bars that say what the
//! person has to know (paused, recording in this window, the last action),
//! and its content on cards. Nothing here decides what a number means.

use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::{
    BODY, BODY_STRONG, CAPTION, Palette, SUBTITLE, label, palette, radius, spacing, text,
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use monitor_core::{ActionError, ActionOutcome, ActionRefusal};

use crate::app::{ActionReport, Collection, MonitorApp};
use crate::i18n::{Copy, copy};

impl MonitorApp {
    /// A listener for a control that reports nothing.
    pub(crate) fn act(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, window, cx);
                cx.notify();
            });
        }
    }

    /// A listener for a control that reports a value.
    pub(crate) fn act_with<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |value, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, value, window, cx);
                cx.notify();
            });
        }
    }

    /// A listener for a table's context menu: the row and where it opened.
    pub(crate) fn act_point(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, usize, Point<Pixels>, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(usize, Point<Pixels>, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |index, position, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, index, position, window, cx);
                cx.notify();
            });
        }
    }

    /// The bars under a page's title: the display is paused, this window is
    /// recording for itself, a request failed, the last action's result.
    pub(crate) fn info_bars(&self, stored: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = copy(self.locale);
        let words = self.locale.words();
        let mut bars = Vec::new();
        if self.paused {
            bars.push(
                InfoBar::new("bar-paused", Severity::Informational, c.paused_title)
                    .message(c.paused_message)
                    .locale(words)
                    .into_any_element(),
            );
        }
        match &self.collection {
            Collection::Unavailable { .. } => bars.push(
                InfoBar::new(
                    "bar-unavailable",
                    Severity::Error,
                    c.collection_unavailable_title,
                )
                .message(c.collection_unavailable)
                .locale(words)
                .into_any_element(),
            ),
            Collection::InProcess { detail } if stored => bars.push(
                InfoBar::new(
                    "bar-in-process",
                    Severity::Warning,
                    c.collection_in_process_title,
                )
                .message(if detail.contains("protocol_version") {
                    c.collection_version_mismatch
                } else if detail.contains("component_disabled") {
                    c.collection_component_off
                } else {
                    c.collection_in_process
                })
                .locale(words)
                .into_any_element(),
            ),
            _ => {}
        }
        if stored && let Some(detail) = &self.last_failure {
            bars.push(
                InfoBar::new("bar-failure", Severity::Error, c.request_failed)
                    .message(crate::i18n::failure_sentence(detail, self.locale))
                    .locale(words)
                    .on_close(Self::act(cx, |this, _, _| this.last_failure = None))
                    .into_any_element(),
            );
        }
        if let Some(ActionReport::SwitchFailed { name, detail }) = &self.last_action {
            // One plain sentence; what systemctl said is only under "Show
            // details", never the message itself.
            bars.push(
                ErrorBar::new(
                    "bar-switch-failed",
                    ErrorReport::new(
                        crate::i18n::switch_failed_title(name, self.locale),
                        detail.clone(),
                    ),
                )
                .locale(words)
                .on_close(Self::act(cx, |this, _, cx| this.dismiss_report(cx)))
                .into_any_element(),
            );
        } else if let Some(report) = &self.last_action {
            let (severity, title, message) = self.report_words(report, c);
            bars.push(
                InfoBar::new("bar-action", severity, title)
                    .message(message)
                    .locale(words)
                    .on_close(Self::act(cx, |this, _, cx| this.dismiss_report(cx)))
                    .into_any_element(),
            );
        }
        bars
    }

    /// What the last action did, in words: a severity, a title, a sentence.
    pub(crate) fn report_words(
        &self,
        report: &ActionReport,
        c: &'static Copy,
    ) -> (Severity, String, String) {
        match report {
            ActionReport::Succeeded {
                name,
                outcome,
                count,
            } => match outcome {
                ActionOutcome::SignalAccepted { .. } => (
                    Severity::Success,
                    name.clone(),
                    if *count > 1 {
                        crate::i18n::asked_to_close_many(*count, self.locale)
                    } else {
                        c.outcome_signal_sent.to_string()
                    },
                ),
                ActionOutcome::PriorityChanged { to, .. } => (
                    Severity::Success,
                    name.clone(),
                    format!("{} ({to})", c.outcome_priority_changed),
                ),
            },
            ActionReport::Refused { name, refusal } => (
                Severity::Warning,
                name.clone(),
                refusal_label(refusal, c).to_string(),
            ),
            ActionReport::Failed { name, error } => {
                let reason = match error {
                    ActionError::ProcessDisappeared { .. } => c.failure_disappeared,
                    ActionError::PermissionDenied { .. } => c.failure_denied,
                    ActionError::InvalidRequest { .. } => c.failure_invalid,
                    ActionError::Unsupported { .. } => c.failure_unsupported,
                    ActionError::Failed { .. } => c.failure_other,
                };
                (Severity::Error, name.clone(), reason.to_string())
            }
            ActionReport::NoLocation { name } => {
                (Severity::Warning, name.clone(), c.no_location.to_string())
            }
            ActionReport::Switched { name, enabled } => (
                Severity::Success,
                name.clone(),
                if *enabled {
                    c.switched_on.to_string()
                } else {
                    c.switched_off.to_string()
                },
            ),
            ActionReport::SwitchFailed { name, detail } => (
                Severity::Error,
                name.clone(),
                crate::i18n::switch_failed_sentence(detail, self.locale),
            ),
            ActionReport::ListPaused { name } => {
                (Severity::Warning, name.clone(), c.list_paused.to_string())
            }
        }
    }
}

/// Why an action is not offered, in words.
pub(crate) fn refusal_label(refusal: &ActionRefusal, c: &'static Copy) -> &'static str {
    match refusal {
        ActionRefusal::NotOwnedByCurrentUser { .. } => c.refusal_other_user,
        ActionRefusal::OwnershipUnknown => c.refusal_ownership_unknown,
        ActionRefusal::ProtectedProcess { .. } => c.refusal_protected,
        ActionRefusal::RaisingPriorityNeedsPrivilege { .. } => c.refusal_raise_priority,
        ActionRefusal::NiceOutOfRange { .. } => c.refusal_nice_range,
        ActionRefusal::CurrentPriorityUnknown => c.refusal_priority_unknown,
        ActionRefusal::ProtectedTree { .. } => c.refusal_protected_tree,
    }
}

/// A card: the card fill, a 1px stroke, 4px corners, 16px padding.
pub(crate) fn card(p: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .p(spacing::LG)
        .gap(spacing::MD)
        .rounded(radius::SM)
        .bg(p.card)
        .border_1()
        .border_color(p.card_stroke)
}

/// A Subtitle heading for a group of cards.
pub(crate) fn section_title(title: impl Into<SharedString>, p: &Palette) -> AnyElement {
    label(title, SUBTITLE, p.text_primary)
        .pt(spacing::SM)
        .into_any_element()
}

/// A figure with its name above it, as Task Manager lists them under a
/// chart: the name in Caption, the value in Body Strong... at Subtitle size
/// for the figures a person reads first.
pub(crate) fn stat(
    name: impl Into<SharedString>,
    value: impl Into<SharedString>,
    large: bool,
    p: &Palette,
) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(120.0))
        .child(label(name, CAPTION, p.text_secondary))
        .child(
            text(div(), if large { SUBTITLE } else { BODY_STRONG })
                .text_color(p.text_primary)
                .child(value.into()),
        )
}

/// A value that is missing, in the secondary color, so it never reads as a
/// measured figure.
pub(crate) fn stat_or_missing(
    name: impl Into<SharedString>,
    value: Option<String>,
    large: bool,
    c: &'static Copy,
    p: &Palette,
) -> Div {
    match value {
        Some(value) => stat(name, value, large, p),
        None => div()
            .flex()
            .flex_col()
            .min_w(px(120.0))
            .child(label(name, CAPTION, p.text_secondary))
            .child(
                text(div(), if large { SUBTITLE } else { BODY_STRONG })
                    .text_color(p.text_secondary)
                    .child(c.not_available),
            ),
    }
}

/// A row of label and value, for a sheet of properties.
pub(crate) fn property_row(
    name: impl Into<SharedString>,
    value: impl Into<SharedString>,
    secondary: bool,
    p: &Palette,
) -> AnyElement {
    div()
        .flex()
        .gap(spacing::MD)
        .min_w_0()
        .py(px(4.0))
        .child(
            text(div(), BODY)
                .w(px(220.0))
                .flex_none()
                .text_color(p.text_secondary)
                .child(name.into()),
        )
        .child(
            text(div(), BODY)
                .flex_1()
                .min_w_0()
                .text_color(if secondary {
                    p.text_secondary
                } else {
                    p.text_primary
                })
                .child(value.into()),
        )
        .into_any_element()
}

/// A short message in a card, for an empty or unavailable state.
pub(crate) fn note_card(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    cx: &App,
) -> AnyElement {
    let p = palette(cx);
    card(p)
        .child(label(title, BODY_STRONG, p.text_primary))
        .child(label(message, BODY, p.text_secondary))
        .into_any_element()
}

/// Lays out children in a grid of equal columns that wrap.
pub(crate) fn grid(columns: usize, children: Vec<AnyElement>) -> AnyElement {
    let columns = columns.max(1);
    let mut rows = div().flex().flex_col().gap(spacing::LG).w_full();
    let mut iter = children.into_iter().peekable();
    while iter.peek().is_some() {
        let mut row = div().flex().gap(spacing::LG).w_full();
        for _ in 0..columns {
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .when_some(iter.next(), |cell, child| cell.child(child)),
            );
        }
        rows = rows.child(row);
    }
    rows.into_any_element()
}

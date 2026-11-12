//! The info bar, and the error pattern every window shares: one plain
//! sentence, with the technical details a click away.
//!
//! `InfoBar_themeresources.xaml`: at least 48px tall, a 1px
//! `CardStrokeColorDefault` edge with 4px corners, the severity's
//! `SystemFillColor*Background` behind it — a tint, never the status color
//! itself, so the text stays primary text on a quiet fill — a 16px icon in
//! the status color with an inverse glyph, 16px in from the left and 14px
//! before the title; a Body Strong title and a Body message on one line that
//! wraps, at most one action 16px after them, and a 38px close button.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, Rgba, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{ActiveTheme as _, Icon, IconName};

use super::Handler;
use super::button::Button;
use super::words::{Action, Locale, action};
use super::{BODY, BODY_STRONG, CODE, Palette, palette, radius, spacing, text};

/// WinUI's four severities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum Severity {
    #[default]
    Informational,
    Success,
    Warning,
    Error,
}

/// The bar's fill, the icon's circle, and the glyph inside it.
pub fn severity_look(severity: Severity, p: &Palette) -> (Rgba, Rgba, IconName) {
    match severity {
        Severity::Informational => (p.attention_background, p.accent, IconName::Info),
        Severity::Success => (p.success_background, p.success, IconName::Check),
        Severity::Warning => (p.caution_background, p.caution, IconName::TriangleAlert),
        Severity::Error => (p.critical_background, p.critical, IconName::Close),
    }
}

/// The severity icon: a 16px circle in the status color holding a glyph in
/// `TextFillColorInverse`. Informational and warning use the letters WinUI's
/// glyphs draw, an "i" and a "!", because the shipped icon set has neither.
pub fn severity_icon(severity: Severity, p: &Palette) -> gpui::Div {
    let (_, circle, glyph) = severity_look(severity, p);
    let circle_el = div()
        .size(px(16.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(circle)
        .text_color(p.text_inverse);
    match severity {
        Severity::Informational | Severity::Warning => {
            let letter = if severity == Severity::Warning {
                "!"
            } else {
                "i"
            };
            circle_el
                .text_size(px(12.0))
                .line_height(px(16.0))
                .font_weight(gpui::FontWeight::BOLD)
                .child(letter)
        }
        _ => circle_el.child(Icon::new(glyph).size(px(10.0))),
    }
}

/// `InfoBar`.
#[derive(IntoElement)]
pub struct InfoBar {
    id: ElementId,
    severity: Severity,
    title: SharedString,
    message: Option<SharedString>,
    action: Option<AnyElement>,
    extra: Vec<AnyElement>,
    locale: Locale,
    on_close: Option<Handler>,
}

impl InfoBar {
    pub fn new(
        id: impl Into<ElementId>,
        severity: Severity,
        title: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            severity,
            title: title.into(),
            message: None,
            action: None,
            extra: Vec::new(),
            locale: Locale::English,
            on_close: None,
        }
    }

    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// The one action a bar may offer, usually a standard [`Button`].
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Shows the close button. Without a handler the bar cannot be
    /// dismissed (`IsClosable="False"`), which is right for a state that is
    /// still true.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    fn below(mut self, element: impl IntoElement) -> Self {
        self.extra.push(element.into_any_element());
        self
    }
}

impl RenderOnce for InfoBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (fill, _, _) = severity_look(self.severity, p);
        let close = self.on_close.map(|handler| {
            div().flex_none().p(px(5.0)).child(
                Button::icon_button(
                    super::child_id(&self.id, "close"),
                    IconName::Close,
                    action(self.locale, Action::Dismiss),
                )
                .on_click(move |window, cx| handler(window, cx)),
            )
        });
        let words = div()
            .flex()
            .flex_wrap()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_x(spacing::MD)
            .min_h(px(46.0))
            .py(px(6.0))
            .child(
                text(div(), BODY_STRONG)
                    .text_color(p.text_primary)
                    .child(self.title),
            )
            .children(self.message.map(|message| {
                text(div().min_w_0(), BODY)
                    .text_color(p.text_primary)
                    .whitespace_normal()
                    .child(message)
            }))
            .children(
                self.action
                    .map(|action| div().ml(px(4.0)).py(px(0.0)).child(action)),
            );
        div()
            .id(self.id)
            .w_full()
            .min_h(px(48.0))
            .flex()
            .flex_col()
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .bg(fill)
            .child(
                div()
                    .flex()
                    .items_start()
                    .pl(spacing::LG)
                    .child(
                        div()
                            .pt(px(15.0))
                            .pr(px(14.0))
                            .child(severity_icon(self.severity, p)),
                    )
                    .child(words)
                    .children(close.or_else(|| Some(div().w(spacing::LG)))),
            )
            .children(self.extra)
    }
}

/// An error as a person reads it: one plain sentence saying what happened
/// and what to do, and the technical details — a command's output, an error
/// chain — hidden until asked for, with a way to copy them for a report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorReport {
    /// One sentence, no jargon: "Better Git could not open this folder."
    pub summary: SharedString,
    /// What the person can do, when there is something.
    pub advice: Option<SharedString>,
    /// Everything a developer would want, verbatim.
    pub details: SharedString,
}

impl ErrorReport {
    pub fn new(summary: impl Into<SharedString>, details: impl Into<SharedString>) -> Self {
        Self {
            summary: summary.into(),
            advice: None,
            details: details.into(),
        }
    }

    pub fn advice(mut self, advice: impl Into<SharedString>) -> Self {
        self.advice = Some(advice.into());
        self
    }

    /// Builds a report from an error and its sources, one per line, so the
    /// details carry the whole chain.
    pub fn from_error(summary: impl Into<SharedString>, error: &dyn std::error::Error) -> Self {
        let mut details = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            details.push_str("\nCaused by: ");
            details.push_str(&cause.to_string());
            source = cause.source();
        }
        Self::new(summary, details)
    }
}

/// The error bar: an error info bar with the summary, the advice, a
/// details toggle, and, when open, the details in monospace with a copy
/// button.
#[derive(IntoElement)]
pub struct ErrorBar {
    id: ElementId,
    report: ErrorReport,
    locale: Locale,
    expanded: Option<bool>,
    on_close: Option<Handler>,
}

impl ErrorBar {
    pub fn new(id: impl Into<ElementId>, report: ErrorReport) -> Self {
        Self {
            id: id.into(),
            report,
            locale: Locale::English,
            expanded: None,
            on_close: None,
        }
    }

    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Draws the details open or closed, for the gallery.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ErrorBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let open_state =
            window.use_keyed_state(super::child_id(&self.id, "details"), cx, |_, _| false);
        let expanded = self.expanded.unwrap_or(*open_state.read(cx));
        let toggle_label = if expanded {
            action(self.locale, Action::HideDetails)
        } else {
            action(self.locale, Action::ShowDetails)
        };
        let toggle_state = open_state.clone();
        let toggle = Button::standard(super::child_id(&self.id, "toggle"), toggle_label).on_click(
            move |_, cx| {
                toggle_state.update(cx, |open, cx| {
                    *open = !*open;
                    cx.notify();
                })
            },
        );
        let mut bar = InfoBar::new(self.id.clone(), Severity::Error, self.report.summary)
            .locale(self.locale)
            .action(toggle);
        if let Some(advice) = self.report.advice {
            bar = bar.message(advice);
        }
        if let Some(handler) = self.on_close {
            bar = bar.on_close(move |window, cx| handler(window, cx));
        }
        if expanded {
            let details = self.report.details.clone();
            let mono = cx.theme().mono_font_family.clone();
            bar = bar.below(
                div()
                    .mx(spacing::LG)
                    .mb(spacing::LG)
                    .ml(px(46.0))
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .id(super::child_id(&self.id, "text"))
                            .max_h(px(160.0))
                            .overflow_y_scroll()
                            .p(spacing::SM)
                            .rounded(radius::SM)
                            .border_1()
                            .border_color(p.card_stroke)
                            .bg(p.card)
                            .font_family(mono)
                            .text_size(px(CODE.size))
                            .line_height(px(CODE.line_height))
                            .text_color(p.text_primary)
                            .whitespace_normal()
                            .child(self.report.details),
                    )
                    .child(
                        div().flex().child(
                            Button::standard(
                                super::child_id(&self.id, "copy"),
                                action(self.locale, Action::CopyDetails),
                            )
                            .icon(IconName::Copy)
                            .on_click(move |_, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    details.to_string(),
                                ))
                            }),
                        ),
                    ),
            );
        }
        bar
    }
}

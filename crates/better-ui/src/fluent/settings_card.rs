//! The settings card and the settings expander, as Windows 11 Settings draws
//! them.
//!
//! WinUI has no settings card of its own; Settings and the WinUI Gallery use
//! the Windows Community Toolkit's `SettingsCard` and `SettingsExpander`.
//! Their look is taken from the toolkit's resources: a card at least 68px
//! tall with 16px padding on `CardBackgroundFillColorDefault`, a
//! `CardStrokeColorDefault` edge and 4px corners, a 20px icon 20px before a
//! Body header over Caption description, the control at the end, and — for
//! a card that navigates — `ControlFillColorSecondary` under the pointer,
//! `ControlFillColorTertiary` pressed, and a chevron. The expander's header
//! is a card with a 32px chevron button (`ExpanderChevronButtonSize`); its
//! rows sit on `CardBackgroundFillColorSecondary` under a 1px divider.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::info_bar::{Severity, severity_icon};
use super::{
    BODY, CAPTION, Interaction, Key, Palette, SUBTITLE, ShownState, keyed_focus, palette, radius,
    resolve, shows_focus, text, with_focus_ring,
};
use super::{Handler, ValueHandler};

const MIN_HEIGHT: Pixels = px(68.0);

/// A card's fill in one state. Only a clickable card changes under the
/// pointer.
pub fn card_fill(clickable: bool, state: Interaction, p: &Palette) -> gpui::Rgba {
    match (clickable, state) {
        (true, Interaction::PointerOver) => p.control_fill_secondary,
        (true, Interaction::Pressed) => p.control_fill_tertiary,
        (_, Interaction::Disabled) => p.control_fill_disabled,
        _ => p.card,
    }
}

/// What stands at a card's leading edge.
#[derive(Clone)]
enum CardLeading {
    Icon(IconName),
    /// The info bar's status icon, for a card that reports a state (Home's
    /// attention cards).
    Status(Severity),
}

fn header_block(
    leading: Option<CardLeading>,
    header: SharedString,
    description: Option<SharedString>,
    disabled: bool,
    p: &Palette,
) -> gpui::Div {
    let (title, caption) = if disabled {
        (p.text_disabled, p.text_disabled)
    } else {
        (p.text_primary, p.text_secondary)
    };
    div()
        .flex()
        .items_center()
        .flex_1()
        .min_w_0()
        .children(leading.map(|leading| {
            let mark = match leading {
                CardLeading::Icon(icon) => Icon::new(icon)
                    .size(px(20.0))
                    .text_color(title)
                    .into_any_element(),
                CardLeading::Status(severity) => severity_icon(severity, p).into_any_element(),
            };
            div()
                .mr(px(20.0))
                .w(px(20.0))
                .flex_none()
                .flex()
                .justify_center()
                .child(mark)
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(text(div(), BODY).text_color(title).child(header))
                .children(description.map(|description| {
                    text(div(), CAPTION)
                        .text_color(caption)
                        .whitespace_normal()
                        .child(description)
                })),
        )
}

/// `SettingsCard`: one setting, with its control at the end; or, with
/// `on_click`, a card that opens a page of its own.
#[derive(IntoElement)]
pub struct SettingsCard {
    id: ElementId,
    icon: Option<CardLeading>,
    header: SharedString,
    description: Option<SharedString>,
    content: Option<AnyElement>,
    disabled: bool,
    shown: ShownState,
    on_click: Option<Handler>,
}

impl SettingsCard {
    pub fn new(id: impl Into<ElementId>, header: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            header: header.into(),
            description: None,
            content: None,
            disabled: false,
            shown: ShownState::Live,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(CardLeading::Icon(icon));
        self
    }

    /// The info bar's status icon in place of a glyph, for a card that
    /// reports something: an error, a warning, that all is well.
    pub fn status(mut self, severity: Severity) -> Self {
        self.icon = Some(CardLeading::Status(severity));
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The control at the card's end: a toggle switch, a combo box, a button.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    /// Makes the whole card a button with a chevron, as a card that opens
    /// another page.
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SettingsCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let clickable = self.on_click.is_some();
        let forced = resolve(self.disabled, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = clickable && !disabled && shows_focus(&focus, self.shown, window);
        let fill = card_fill(clickable, forced.unwrap_or(Interaction::Rest), p);
        let hover = card_fill(true, Interaction::PointerOver, p);
        let pressed = card_fill(true, Interaction::Pressed, p);

        let mut card = div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .min_h(MIN_HEIGHT)
            .flex()
            .items_center()
            .gap(px(16.0))
            .p(px(16.0))
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .bg(fill)
            .child(header_block(
                self.icon,
                self.header,
                self.description,
                disabled,
                p,
            ))
            .children(self.content.map(|content| div().flex_none().child(content)));
        if clickable {
            let chevron = if disabled {
                p.text_disabled
            } else {
                p.text_secondary
            };
            card = card.child(
                Icon::new(IconName::ChevronRight)
                    .size(px(12.0))
                    .text_color(chevron),
            );
        }
        if let Some(handler) = self.on_click
            && !disabled
        {
            let on_key = handler.clone();
            if forced.is_none() {
                card = card
                    .hover(move |s| s.bg(hover))
                    .active(move |s| s.bg(pressed));
            }
            card = card
                .track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
                .on_click(move |event, window, cx| {
                    // Keys are handled on key down, by the WinUI rule.
                    if !event.is_keyboard() {
                        handler(window, cx);
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                        cx.stop_propagation();
                        on_key(window, cx);
                    }
                });
        }
        with_focus_ring(card, ring, radius::SM, p)
    }
}

/// One row inside an open settings expander.
pub struct SettingsRow {
    pub header: SharedString,
    pub description: Option<SharedString>,
    pub content: Option<AnyElement>,
}

impl SettingsRow {
    pub fn new(header: impl Into<SharedString>) -> Self {
        Self {
            header: header.into(),
            description: None,
            content: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }
}

/// `SettingsExpander`: a card whose chevron shows a group of related rows.
/// The window keeps whether it is open.
#[derive(IntoElement)]
pub struct SettingsExpander {
    id: ElementId,
    icon: Option<CardLeading>,
    header: SharedString,
    description: Option<SharedString>,
    content: Option<AnyElement>,
    rows: Vec<SettingsRow>,
    expanded: bool,
    shown: ShownState,
    on_toggle: Option<ValueHandler<bool>>,
}

impl SettingsExpander {
    pub fn new(id: impl Into<ElementId>, header: impl Into<SharedString>, expanded: bool) -> Self {
        Self {
            id: id.into(),
            icon: None,
            header: header.into(),
            description: None,
            content: None,
            rows: Vec::new(),
            expanded,
            shown: ShownState::Live,
            on_toggle: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(CardLeading::Icon(icon));
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A control in the header, beside the chevron.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn row(mut self, row: SettingsRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SettingsExpander {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let forced = resolve(false, self.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let expanded = self.expanded;
        let state = forced.unwrap_or(Interaction::Rest);
        let chevron_fill = match state {
            Interaction::PointerOver => p.subtle_fill,
            Interaction::Pressed => p.subtle_fill_tertiary,
            _ => super::with_alpha(p.subtle_fill, 0.0),
        };
        let hover = p.subtle_fill;
        let pressed = p.subtle_fill_tertiary;
        let group: SharedString = "fluent-expander".into();

        let mut header = div()
            .id(self.id.clone())
            .group(group.clone())
            .relative()
            .w_full()
            .min_h(MIN_HEIGHT)
            .flex()
            .items_center()
            .gap(px(16.0))
            .pl(px(16.0))
            .pr(px(8.0))
            .py(px(16.0))
            .border_1()
            .border_color(p.card_stroke)
            .bg(if disabled {
                p.control_fill_disabled
            } else {
                p.card
            })
            .when(expanded, |h| h.rounded_t(radius::SM))
            .when(!expanded, |h| h.rounded(radius::SM))
            .child(header_block(
                self.icon,
                self.header,
                self.description,
                disabled,
                p,
            ))
            .children(self.content.map(|content| div().flex_none().child(content)))
            .child({
                let mut chevron = div()
                    .id("chevron")
                    .size(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::SM)
                    .bg(chevron_fill)
                    .text_color(if disabled {
                        p.text_disabled
                    } else {
                        p.text_primary
                    })
                    .child(
                        Icon::new(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(px(12.0)),
                    );
                if forced.is_none() {
                    chevron = chevron
                        .group_hover(group.clone(), move |s| s.bg(hover))
                        .group_active(group.clone(), move |s| s.bg(pressed));
                }
                chevron
            });
        if let Some(handler) = self.on_toggle
            && !disabled
        {
            let on_key = handler.clone();
            header = header
                .track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()))
                .on_click(move |event, window, cx| {
                    // Keys are handled on key down, by the WinUI rule.
                    if !event.is_keyboard() {
                        handler(!expanded, window, cx);
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                        cx.stop_propagation();
                        on_key(!expanded, window, cx);
                    }
                });
        }
        let header = with_focus_ring(header, ring, radius::SM, p);

        let mut root = div().flex().flex_col().w_full().child(header);
        if expanded {
            let count = self.rows.len();
            let mut body = div()
                .flex()
                .flex_col()
                .border_1()
                .border_t_0()
                .border_color(p.card_stroke)
                .rounded_b(radius::SM)
                .bg(p.card_secondary);
            for (index, row) in self.rows.into_iter().enumerate() {
                body = body.child(
                    div()
                        .min_h(px(52.0))
                        .flex()
                        .items_center()
                        .gap(px(16.0))
                        .pl(px(56.0))
                        .pr(px(16.0))
                        .py(px(8.0))
                        .when(index + 1 < count, |r| {
                            r.border_b_1().border_color(p.divider)
                        })
                        .child(header_block(None, row.header, row.description, false, p))
                        .children(row.content.map(|content| div().flex_none().child(content))),
                );
            }
            root = root.child(body);
        }
        root
    }
}

/// The banner at the top of a page that is about this computer, as Windows
/// 11 Settings' Home and System pages open with: a card with a 48px glyph,
/// the device's name in Subtitle over a Caption line, and something at the
/// trailing edge (the battery). It sits in the page like any card: card
/// fill, card stroke, 4px corners, 20px by 24px padding.
#[derive(IntoElement)]
pub struct DeviceBanner {
    icon: IconName,
    title: SharedString,
    caption: Option<SharedString>,
    trailing: Option<AnyElement>,
}

impl DeviceBanner {
    pub fn new(icon: IconName, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            caption: None,
            trailing: None,
        }
    }

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }
}

impl RenderOnce for DeviceBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(20.0))
            .py(px(20.0))
            .px(px(24.0))
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .bg(p.card)
            .child(
                div().flex_none().child(
                    Icon::new(self.icon)
                        .size(px(48.0))
                        .text_color(p.text_secondary),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(200.0))
                    .child(
                        text(div(), SUBTITLE)
                            .text_color(p.text_primary)
                            .whitespace_normal()
                            .child(self.title),
                    )
                    .children(self.caption.map(|caption| {
                        text(div(), CAPTION)
                            .text_color(p.text_secondary)
                            .whitespace_normal()
                            .child(caption)
                    })),
            )
            .children(
                self.trailing
                    .map(|trailing| div().flex_none().child(trailing)),
            )
    }
}

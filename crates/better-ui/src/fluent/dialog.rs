//! What floats above the page: the content dialog, the flyout, and the
//! tooltip.
//!
//! `ContentDialog_themeresources.xaml`: 320 to 548px wide, 24px padding, a
//! Subtitle title 12px above the content, the button row on
//! `SolidBackgroundFillColorBase` under a 1px `CardStrokeColorDefault`
//! separator, the body on `LayerFillColorAlt`, a `SurfaceStrokeColorDefault`
//! edge, and `SmokeFillColorDefault` behind it all.
//! `FlyoutPresenter_themeresources.xaml`: 16,15,16,17 padding.
//! `ToolTip_themeresources.xaml`: 12px text, 9,6,9,8 padding, 320px at most.

use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, ElementId, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, SharedString, Styled, Window, anchored, deferred, div, px,
    relative,
};
use gpui_component::FocusTrapElement as _;

use super::button::{Button, ButtonKind};
use super::menu::panel_frame;
use super::{BODY, CAPTION, Key, SUBTITLE, palette, radius, spacing, text};
use super::{Handler, ValueHandler};

/// Which of a dialog's buttons a person pressed, or Escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DialogResult {
    Primary,
    Secondary,
    /// The close button, or Escape.
    Close,
}

/// Which button is the default one: drawn in accent and pressed by Enter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DefaultButton {
    #[default]
    Primary,
    Secondary,
    Close,
    None,
}

/// The labels a dialog shows on its buttons. A missing label is a missing
/// button, as in WinUI.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DialogButtons {
    pub primary: Option<SharedString>,
    pub secondary: Option<SharedString>,
    pub close: Option<SharedString>,
    pub default: DefaultButton,
}

impl DialogButtons {
    /// The buttons in WinUI's order, left to right, each with whether it is
    /// the accent one.
    pub fn layout(&self) -> Vec<(DialogResult, SharedString, bool)> {
        [
            (DialogResult::Primary, &self.primary, DefaultButton::Primary),
            (
                DialogResult::Secondary,
                &self.secondary,
                DefaultButton::Secondary,
            ),
            (DialogResult::Close, &self.close, DefaultButton::Close),
        ]
        .into_iter()
        .filter_map(|(result, label, default)| {
            label
                .clone()
                .map(|label| (result, label, self.default == default))
        })
        .collect()
    }

    /// What a key does to an open dialog: Escape closes it, Enter presses
    /// the default button when there is one.
    pub fn key(&self, key: Key) -> Option<DialogResult> {
        match key {
            Key::Escape => Some(DialogResult::Close),
            Key::Enter => self
                .layout()
                .into_iter()
                .find(|(_, _, accent)| *accent)
                .map(|(result, _, _)| result),
            _ => None,
        }
    }

    /// [`DialogButtons::key`] when the primary button may be disabled
    /// (`IsPrimaryButtonEnabled="False"`): Enter does not press a button
    /// that cannot be pressed.
    pub fn key_with(&self, key: Key, primary_enabled: bool) -> Option<DialogResult> {
        self.key(key)
            .filter(|result| primary_enabled || *result != DialogResult::Primary)
    }
}

/// A content dialog over a smoke layer that covers the window. Draw it as
/// the last child of the window's root while it is open; `on_result` closes
/// it.
#[derive(IntoElement)]
pub struct ContentDialog {
    id: ElementId,
    title: SharedString,
    content: Vec<AnyElement>,
    buttons: DialogButtons,
    on_result: Option<ValueHandler<DialogResult>>,
    /// Draw in the page, without the smoke layer, for the gallery.
    inline: bool,
    primary_enabled: bool,
}

impl ContentDialog {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            content: Vec::new(),
            buttons: DialogButtons::default(),
            on_result: None,
            inline: false,
            primary_enabled: true,
        }
    }

    /// Disables the primary button, as `IsPrimaryButtonEnabled` does: drawn
    /// disabled, not clicked, not pressed by Enter.
    pub fn primary_enabled(mut self, enabled: bool) -> Self {
        self.primary_enabled = enabled;
        self
    }

    pub fn buttons(mut self, buttons: DialogButtons) -> Self {
        self.buttons = buttons;
        self
    }

    pub fn on_result(
        mut self,
        handler: impl Fn(DialogResult, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_result = Some(Rc::new(handler));
        self
    }

    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }
}

impl ParentElement for ContentDialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

impl RenderOnce for ContentDialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = super::keyed_focus(super::child_id(&self.id, "trap"), window, cx);
        let buttons = self.buttons.clone();
        let on_result = self.on_result.clone();
        let primary_enabled = self.primary_enabled;

        let mut row = div()
            .flex()
            .gap(spacing::SM)
            .p(spacing::XL)
            .bg(p.window_background)
            .border_t_1()
            .border_color(p.card_stroke);
        for (index, (result, label, accent)) in self.buttons.layout().into_iter().enumerate() {
            let kind = if accent {
                ButtonKind::Accent
            } else {
                ButtonKind::Standard
            };
            let handler = on_result.clone();
            row = row.child(
                div().flex_1().min_w_0().child(
                    Button::new(ElementId::named_usize("dialog-button", index), kind)
                        .label(label)
                        .full_width()
                        .disabled(result == DialogResult::Primary && !self.primary_enabled)
                        .on_click(move |window, cx| {
                            if let Some(handler) = handler.as_ref() {
                                handler(result, window, cx);
                            }
                        }),
                ),
            );
        }

        let body = text(div(), BODY)
            .text_color(p.text_primary)
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .children(self.content);
        let dialog = div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .min_w(px(320.0))
            .max_w(px(548.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(p.surface_stroke)
            .bg(p.dialog)
            .shadow_lg()
            .overflow_hidden()
            .track_focus(&focus)
            .on_key_down(move |event, window, cx| {
                if let Some(result) = Key::from_keystroke(&event.keystroke)
                    .and_then(|key| buttons.key_with(key, primary_enabled))
                {
                    cx.stop_propagation();
                    if let Some(handler) = on_result.as_ref() {
                        handler(result, window, cx);
                    }
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(spacing::XL)
                    .child(
                        text(div().mb(spacing::MD), SUBTITLE)
                            .text_color(p.text_primary)
                            .child(self.title),
                    )
                    .child(body),
            )
            .child(row);

        if self.inline {
            return dialog.into_any_element();
        }
        if !focus.contains_focused(window, cx) {
            focus.focus(window, cx);
        }
        let trap_id = super::child_id(&self.id, "smoke");
        deferred(
            div()
                .id(trap_id.clone())
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(p.smoke)
                .occlude()
                .child(dialog)
                .focus_trap(trap_id, &focus),
        )
        .with_priority(2)
        .into_any_element()
    }
}

/// A flyout: a light-dismiss panel with any content, anchored under the
/// control it belongs to. The control keeps whether it is open and passes
/// `on_dismiss`, called on Escape and on a click outside.
#[derive(IntoElement)]
pub struct Flyout {
    id: ElementId,
    content: Vec<AnyElement>,
    on_dismiss: Option<Handler>,
    inline: bool,
}

impl Flyout {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            content: Vec::new(),
            on_dismiss: None,
            inline: false,
        }
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }

    /// Draw in the page, for the gallery.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }
}

impl ParentElement for Flyout {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

impl RenderOnce for Flyout {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let focus = super::keyed_focus(self.id.clone(), window, cx);
        let dismiss_key = self.on_dismiss.clone();
        let dismiss_out = self.on_dismiss.clone();
        let panel = panel_frame(div().id(self.id.clone()), p)
            .occlude()
            .pt(px(15.0))
            .pb(px(17.0))
            .px(spacing::LG)
            .gap(spacing::MD)
            .track_focus(&focus)
            .on_key_down(move |event, window, cx| {
                if Key::from_keystroke(&event.keystroke) == Some(Key::Escape) {
                    cx.stop_propagation();
                    if let Some(handler) = dismiss_key.as_ref() {
                        handler(window, cx);
                    }
                }
            })
            .on_mouse_down_out(move |_, window, cx| {
                if let Some(handler) = dismiss_out.as_ref() {
                    handler(window, cx);
                }
            })
            .children(self.content);
        if self.inline {
            return panel.into_any_element();
        }
        if !focus.contains_focused(window, cx) {
            focus.focus(window, cx);
        }
        div()
            .absolute()
            .top(relative(1.0))
            .left_0()
            .child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.0))
                        .child(div().mt(px(4.0)).child(panel)),
                )
                .with_priority(1),
            )
            .into_any_element()
    }
}

/// WinUI's tooltip: 12px text in a 4px-cornered flyout-colored box.
pub struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        text(div(), CAPTION)
            .max_w(px(320.0))
            .pt(px(6.0))
            .pb(px(8.0))
            .px(px(9.0))
            .bg(p.flyout)
            .border_1()
            .border_color(p.flyout_stroke)
            .rounded(radius::SM)
            .shadow_md()
            .text_color(p.text_primary)
            .child(self.text.clone())
    }
}

/// A tooltip builder for any element's `.tooltip(...)`.
pub fn tooltip_builder(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
}

/// A tooltip drawn in the page, for the gallery.
pub fn tooltip_inline(text_value: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let p = palette(cx);
    text(div(), CAPTION)
        .max_w(px(320.0))
        .pt(px(6.0))
        .pb(px(8.0))
        .px(px(9.0))
        .bg(p.flyout)
        .border_1()
        .border_color(p.flyout_stroke)
        .rounded(radius::SM)
        .shadow_md()
        .text_color(p.text_primary)
        .child(text_value.into())
}

/// A helper for windows: whether a dialog's `on_result` is a confirmation.
pub fn confirmed(result: DialogResult) -> bool {
    result == DialogResult::Primary
}

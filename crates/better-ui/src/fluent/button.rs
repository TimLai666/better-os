//! Buttons: accent, standard, and subtle; the icon button; the toggle
//! button; and the split button.
//!
//! Sizes and colors are WinUI's `Button_themeresources.xaml`,
//! `ToggleButton_themeresources.xaml`, and `SplitButton_themeresources.xaml`:
//! 32px tall, `ButtonPadding` 11,5,11,6, a 1px elevation border, 4px corners.
//! Space and Enter invoke a focused button (`ButtonBaseKeyProcess.h`).

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::menu::{MenuEntry, MenuFlyout, MenuOpen, open_menu};
use super::{
    BODY, ControlColors, Interaction, Key, Palette, ShownState, StateColors, child_id, keyed_focus,
    paint, palette, radius, resolve, shows_focus, size, text, with_focus_ring,
};

pub use super::Handler;

/// WinUI's three button styles.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ButtonKind {
    /// `AccentButtonStyle`: the one primary action on a surface.
    Accent,
    /// `DefaultButtonStyle`.
    Standard,
    /// `SubtleButtonStyle`: no fill until hovered.
    Subtle,
}

const TRANSPARENT: gpui::Rgba = gpui::Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// The colors of a button of `kind` in every state. A checked toggle button
/// is drawn as an accent button (`ToggleButtonBackgroundChecked` is
/// `AccentFillColorDefaultBrush`).
pub fn colors(kind: ButtonKind, checked: bool, p: &Palette) -> StateColors {
    let kind = if checked { ButtonKind::Accent } else { kind };
    StateColors::from_fn(|state| match (kind, state) {
        (ButtonKind::Standard, Interaction::Rest) => ControlColors {
            background: p.control_fill,
            foreground: p.text_primary,
            border: p.stroke,
            elevation: Some(p.stroke_secondary),
        },
        (ButtonKind::Standard, Interaction::PointerOver) => ControlColors {
            background: p.control_fill_secondary,
            foreground: p.text_primary,
            border: p.stroke,
            elevation: Some(p.stroke_secondary),
        },
        (ButtonKind::Standard, Interaction::Pressed) => ControlColors {
            background: p.control_fill_tertiary,
            foreground: p.text_secondary,
            border: p.stroke,
            elevation: None,
        },
        (ButtonKind::Standard, Interaction::Disabled) => ControlColors {
            background: p.control_fill_disabled,
            foreground: p.text_disabled,
            border: p.stroke,
            elevation: None,
        },
        (ButtonKind::Accent, Interaction::Rest) => ControlColors {
            background: p.accent,
            foreground: p.text_on_accent,
            border: p.stroke_on_accent,
            elevation: Some(p.stroke_on_accent_secondary),
        },
        (ButtonKind::Accent, Interaction::PointerOver) => ControlColors {
            background: p.accent_secondary,
            foreground: p.text_on_accent,
            border: p.stroke_on_accent,
            elevation: Some(p.stroke_on_accent_secondary),
        },
        (ButtonKind::Accent, Interaction::Pressed) => ControlColors {
            background: p.accent_tertiary,
            foreground: p.text_on_accent_secondary,
            border: TRANSPARENT,
            elevation: None,
        },
        (ButtonKind::Accent, Interaction::Disabled) => ControlColors {
            background: p.accent_disabled,
            foreground: p.text_on_accent_disabled,
            border: TRANSPARENT,
            elevation: None,
        },
        (ButtonKind::Subtle, Interaction::Rest) => ControlColors {
            background: TRANSPARENT,
            foreground: p.text_primary,
            border: TRANSPARENT,
            elevation: None,
        },
        (ButtonKind::Subtle, Interaction::PointerOver) => ControlColors {
            background: p.subtle_fill,
            foreground: p.text_primary,
            border: TRANSPARENT,
            elevation: None,
        },
        (ButtonKind::Subtle, Interaction::Pressed) => ControlColors {
            background: p.subtle_fill_tertiary,
            foreground: p.text_secondary,
            border: TRANSPARENT,
            elevation: None,
        },
        (ButtonKind::Subtle, Interaction::Disabled) => ControlColors {
            background: TRANSPARENT,
            foreground: p.text_disabled,
            border: TRANSPARENT,
            elevation: None,
        },
    })
}

/// What a key does to a focused button: invoke it, or nothing.
pub fn invoked_by(key: Key) -> bool {
    key.invokes()
}

/// A button. With a label it is a text button; with only an icon it is the
/// 32px square icon button; with `checked` it is a toggle button.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    kind: ButtonKind,
    label: Option<SharedString>,
    icon: Option<Icon>,
    checked: Option<bool>,
    disabled: bool,
    shown: ShownState,
    full_width: bool,
    tooltip: Option<SharedString>,
    on_click: Option<Handler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, kind: ButtonKind) -> Self {
        Self {
            id: id.into(),
            kind,
            label: None,
            icon: None,
            checked: None,
            disabled: false,
            shown: ShownState::Live,
            full_width: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn accent(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonKind::Accent).label(label)
    }

    pub fn standard(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonKind::Standard).label(label)
    }

    pub fn subtle(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, ButtonKind::Subtle).label(label)
    }

    /// A 32px square button that holds only a 16px icon. It needs a tooltip,
    /// because it has no words.
    pub fn icon_button(
        id: impl Into<ElementId>,
        icon: impl Into<Icon>,
        tooltip: impl Into<SharedString>,
    ) -> Self {
        Self::new(id, ButtonKind::Subtle)
            .icon(icon)
            .tooltip(tooltip)
    }

    /// `ToggleButton`: standard at rest, accent while checked.
    pub fn toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, checked: bool) -> Self {
        Self::new(id, ButtonKind::Standard)
            .label(label)
            .checked(checked)
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
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

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let state = resolve(self.disabled, self.shown);
        let disabled = state == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let icon_only = self.label.is_none();
        let colors = colors(self.kind, self.checked.unwrap_or(false), p);

        let mut button = div()
            .id(self.id.clone())
            .h(size::CONTROL_HEIGHT)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .when(icon_only, |b| b.w(size::CONTROL_HEIGHT))
            .when(!icon_only, |b| b.px(px(11.0)))
            .when(self.full_width, |b| b.w_full());
        button = text(button, BODY);
        if !disabled {
            button = button.track_focus(&focus.tab_index(0).tab_stop(self.shown.is_live()));
        }
        let mut button = paint(button, colors, state, radius::SM, p.is_dark());
        if let Some(icon) = self.icon {
            button = button.child(icon.size(size::ICON));
        }
        if let Some(label) = self.label {
            button = button.child(div().whitespace_nowrap().child(label));
        }
        if let Some(tooltip) = self.tooltip {
            button = button.tooltip(super::dialog::tooltip_builder(tooltip));
        }
        if !disabled && let Some(handler) = self.on_click {
            let on_key = handler.clone();
            button = button
                .on_click(move |event, window, cx| {
                    // Keys are handled on key down, by the WinUI rule.
                    if !event.is_keyboard() {
                        handler(window, cx);
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if Key::from_keystroke(&event.keystroke).is_some_and(invoked_by) {
                        cx.stop_propagation();
                        on_key(window, cx);
                    }
                });
        }
        with_focus_ring(button, ring, radius::SM, p)
    }
}

/// `SplitButton`: a primary action and a chevron that opens a menu of
/// related ones. Enter and Space run the primary action; Alt+Down and F4
/// open the menu, as WinUI's split button does.
#[derive(IntoElement)]
pub struct SplitButton {
    id: ElementId,
    label: SharedString,
    kind: ButtonKind,
    entries: Vec<MenuEntry>,
    disabled: bool,
    shown: ShownState,
    on_click: Option<Handler>,
}

/// `SplitButtonSecondaryButtonSize`.
const SPLIT_CHEVRON_WIDTH: Pixels = px(35.0);

impl SplitButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: ButtonKind::Standard,
            entries: Vec::new(),
            disabled: false,
            shown: ShownState::Live,
            on_click: None,
        }
    }

    pub fn accent(mut self) -> Self {
        self.kind = ButtonKind::Accent;
        self
    }

    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
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

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SplitButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let state = resolve(self.disabled, self.shown);
        let disabled = state == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, self.shown, window);
        let open = MenuOpen::keyed(child_id(&self.id, "menu"), window, cx);
        let is_open = open.read(cx).is_open();
        let colors = colors(self.kind, false, p);
        let divider = if self.kind == ButtonKind::Accent {
            p.stroke_on_accent_secondary
        } else {
            p.stroke
        };

        let primary_id = child_id(&self.id, "primary");
        let mut primary = text(
            div()
                .id(primary_id)
                .h_full()
                .px(px(11.0))
                .flex()
                .items_center(),
            BODY,
        )
        .child(div().whitespace_nowrap().child(self.label));
        let chevron_id = child_id(&self.id, "chevron");
        let mut chevron = div()
            .id(chevron_id)
            .h_full()
            .w(SPLIT_CHEVRON_WIDTH)
            .flex()
            .items_center()
            .justify_center()
            .border_l_1()
            .border_color(divider)
            .child(Icon::new(IconName::ChevronDown).size(px(12.0)));

        // Each half takes its own hover and press, as WinUI's two buttons do.
        if state.is_none() {
            let hover = colors.pointer_over.background;
            let pressed = colors.pressed.background;
            primary = primary
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed));
            chevron = chevron
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed));
        }
        if !disabled {
            if let Some(handler) = self.on_click.clone() {
                primary = primary.on_click(move |_, window, cx| handler(window, cx));
            }
            let open_click = open.clone();
            chevron = chevron.on_click(move |_, window, cx| {
                cx.stop_propagation();
                if open_click.read(cx).is_open() {
                    open_click.update(cx, |state, cx| {
                        state.close();
                        cx.notify();
                    });
                } else {
                    open_menu(&open_click, None, window, cx);
                }
            });
        }

        let base = colors.get(state.unwrap_or(Interaction::Rest));
        let mut root = div()
            .id(self.id.clone())
            .relative()
            .h(size::CONTROL_HEIGHT)
            .flex()
            .flex_none()
            .rounded(radius::SM)
            .border_1()
            .border_color(base.border)
            .bg(base.background)
            .text_color(base.foreground)
            .overflow_hidden()
            .child(primary)
            .child(chevron);
        if let Some(edge) = base.elevation {
            root = root.child(super::elevation_edge(edge, radius::SM, p.is_dark()));
        }
        if !disabled {
            let handler = self.on_click.clone();
            let open_key = open.clone();
            root = root
                .track_focus(&focus.clone().tab_index(0).tab_stop(self.shown.is_live()))
                .on_key_down(move |event, window, cx| {
                    match Key::from_keystroke(&event.keystroke) {
                        Some(Key::OpenDropDown) => {
                            cx.stop_propagation();
                            open_menu(&open_key, Some(0), window, cx);
                        }
                        Some(key) if key.invokes() && !open_key.read(cx).is_open() => {
                            cx.stop_propagation();
                            if let Some(handler) = handler.as_ref() {
                                handler(window, cx);
                            }
                        }
                        _ => {}
                    }
                });
        }
        let root = with_focus_ring(root, ring, radius::SM, p);
        let menu: Option<AnyElement> = is_open.then(|| {
            MenuFlyout::new(child_id(&self.id, "flyout"), self.entries)
                .state(open.clone())
                .return_focus(focus.clone())
                .into_any_element()
        });
        div().relative().flex_none().child(root).children(menu)
    }
}

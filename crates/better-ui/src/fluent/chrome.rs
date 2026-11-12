//! The window's own titlebar, as Windows 11 draws one.
//!
//! Mutter gives an `xdg-toplevel` client no decorations, so every Better OS
//! window draws this bar (see `crate::window_chrome`). The shape is Windows
//! 11's standard caption: a 32px bar on the window background, the 16px app
//! icon 16px from the leading edge, the title in Caption 16px after it, and
//! the three caption buttons at the trailing edge, 46 by 32 each, with 10px
//! glyphs. Minimize and maximize take `SubtleFillColorSecondary` under the
//! pointer and `SubtleFillColorTertiary` pressed; close turns `#C42B1C` with a
//! white glyph, and 90% of that pressed. While the window is inactive the
//! title and the glyphs are in tertiary text.
//!
//! Dragging, double-click to maximize, and the right-click window menu go to
//! the compositor through the same calls gpui-component's own titlebar uses.
//!
//! A window with page history puts a back button at the bar's leading edge,
//! where Windows 11 Settings has it (WinUI's `NavigationView` back button,
//! drawn in the titlebar since Windows 11): a 40 by 32 subtle button with a
//! 16px arrow, the icon and title moving 8px after it. It is there only while
//! there is somewhere to go back to. Alt+Left and the mouse's back button do
//! the same; [`is_back_shortcut`] is the key rule.

use gpui::{
    AnyElement, App, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    RenderOnce, Rgba, SharedString, StatefulInteractiveElement, Styled, Window, WindowControlArea,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName, InteractiveElementExt as _};

use super::words::Locale;
use super::{CAPTION, Handler, Interaction, Palette, ShownState, hex, palette, text, with_alpha};

/// The back button's width.
pub const BACK_BUTTON_WIDTH: Pixels = px(40.0);

/// The back button's tooltip.
pub fn back_tooltip(locale: Locale) -> &'static str {
    match locale {
        Locale::English => "Back",
        Locale::TraditionalChinese => "返回",
    }
}

/// Whether a key press means "go back": Alt+Left, as in every Windows
/// application with page history. Any other modifier with it does not.
pub fn is_back_shortcut(key: &str, control: bool, alt: bool, shift: bool, platform: bool) -> bool {
    key == "left" && alt && !control && !shift && !platform
}

/// The bar's height.
pub const TITLE_BAR_HEIGHT: Pixels = px(32.0);
/// One caption button's width.
pub const CAPTION_BUTTON_WIDTH: Pixels = px(46.0);
/// The close button's fill under the pointer. Windows uses this red in both
/// appearances.
pub const CLOSE_RED: Rgba = hex(0xc42b1c);

/// The three caption buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CaptionButton {
    Minimize,
    /// Maximize, or restore when the window is maximized.
    Maximize,
    Close,
}

/// A caption button's fill and glyph color.
pub fn caption_colors(
    button: CaptionButton,
    state: Interaction,
    active: bool,
    p: &Palette,
) -> (Rgba, Rgba) {
    let idle = if active {
        p.text_primary
    } else {
        p.text_tertiary
    };
    match (button, state) {
        (CaptionButton::Close, Interaction::PointerOver) => (CLOSE_RED, hex(0xffffff)),
        (CaptionButton::Close, Interaction::Pressed) => {
            (with_alpha(CLOSE_RED, 0.9), with_alpha(hex(0xffffff), 0.7))
        }
        (_, Interaction::PointerOver) => (p.subtle_fill, p.text_primary),
        (_, Interaction::Pressed) => (p.subtle_fill_tertiary, p.text_secondary),
        (_, Interaction::Disabled) => (with_alpha(p.subtle_fill, 0.0), p.text_disabled),
        (_, Interaction::Rest) => (with_alpha(p.subtle_fill, 0.0), idle),
    }
}

/// The titlebar. Put it first in the window's root column.
#[derive(IntoElement)]
pub struct TitleBar {
    icon: Option<AnyElement>,
    title: SharedString,
    /// Content between the title and the caption buttons, such as a search
    /// box, as Windows 11's tall titlebar holds.
    middle: Option<AnyElement>,
    /// Forces a caption button's state, for the gallery.
    shown: Option<(CaptionButton, ShownState)>,
    /// Draws the bar as if the window were inactive, for the gallery.
    inactive: bool,
    /// A dialog's bar: the close button alone, and no maximize on a double
    /// click, as a Windows 11 dialog window has.
    dialog: bool,
    /// The back button's handler and tooltip, while there is history.
    back: Option<(Handler, SharedString)>,
}

/// The caption buttons a bar shows, left to right.
pub fn caption_buttons(dialog: bool) -> &'static [CaptionButton] {
    if dialog {
        &[CaptionButton::Close]
    } else {
        &[
            CaptionButton::Minimize,
            CaptionButton::Maximize,
            CaptionButton::Close,
        ]
    }
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            title: title.into(),
            middle: None,
            shown: None,
            inactive: false,
            dialog: false,
            back: None,
        }
    }

    /// A back button at the leading edge. Pass it only while the window has
    /// somewhere to go back to; without it the bar has none.
    pub fn back(
        mut self,
        locale: Locale,
        handler: impl Fn(&mut gpui::Window, &mut App) + 'static,
    ) -> Self {
        self.back = Some((std::rc::Rc::new(handler), back_tooltip(locale).into()));
        self
    }

    /// The bar of a dialog window: close only.
    pub fn dialog(mut self) -> Self {
        self.dialog = true;
        self
    }

    pub fn icon(mut self, icon: impl IntoElement) -> Self {
        self.icon = Some(icon.into_any_element());
        self
    }

    pub fn middle(mut self, content: impl IntoElement) -> Self {
        self.middle = Some(content.into_any_element());
        self
    }

    pub fn shown(mut self, button: CaptionButton, state: ShownState) -> Self {
        self.shown = Some((button, state));
        self
    }

    pub fn inactive(mut self) -> Self {
        self.inactive = true;
        self
    }
}

/// Remembers a press on the bar so that the first move starts a window move.
#[derive(Default)]
struct DragState {
    pressed: bool,
}

fn caption_button(
    button: CaptionButton,
    maximized: bool,
    active: bool,
    forced: Option<Interaction>,
    p: &Palette,
) -> impl IntoElement {
    let (fill, glyph) = caption_colors(button, forced.unwrap_or(Interaction::Rest), active, p);
    let (hover_fill, hover_glyph) = caption_colors(button, Interaction::PointerOver, active, p);
    let (pressed_fill, pressed_glyph) = caption_colors(button, Interaction::Pressed, active, p);
    let (id, icon, area) = match (button, maximized) {
        (CaptionButton::Minimize, _) => {
            ("minimize", IconName::WindowMinimize, WindowControlArea::Min)
        }
        (CaptionButton::Maximize, false) => {
            ("maximize", IconName::WindowMaximize, WindowControlArea::Max)
        }
        (CaptionButton::Maximize, true) => {
            ("restore", IconName::WindowRestore, WindowControlArea::Max)
        }
        (CaptionButton::Close, _) => ("close", IconName::WindowClose, WindowControlArea::Close),
    };
    let mut element = div()
        .id(id)
        .w(CAPTION_BUTTON_WIDTH)
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(glyph)
        .child(Icon::new(icon).size(px(10.0)));
    if forced.is_none() {
        element = element
            .hover(move |s| s.bg(hover_fill).text_color(hover_glyph))
            .active(move |s| s.bg(pressed_fill).text_color(pressed_glyph));
    }
    if cfg!(target_os = "windows") {
        element = element.window_control_area(area);
    }
    element
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            match button {
                CaptionButton::Minimize => window.minimize_window(),
                CaptionButton::Maximize => window.zoom_window(),
                CaptionButton::Close => window.remove_window(),
            }
        })
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let active = window.is_window_active() && !self.inactive;
        let maximized = window.is_maximized();
        let drag = window.use_state(cx, |_, _| DragState::default());
        let title_color = if active {
            p.text_primary
        } else {
            p.text_tertiary
        };
        let forced = |button: CaptionButton| {
            self.shown
                .filter(|(b, _)| *b == button)
                .and_then(|(_, state)| state.forced())
        };
        let down = drag.clone();
        let up = drag.clone();
        let out = drag.clone();
        let moving = drag.clone();
        let has_back = self.back.is_some();
        let back = self.back.map(|(handler, tooltip)| {
            let hover = p.subtle_fill;
            let pressed = p.subtle_fill_tertiary;
            let pressed_text = p.text_secondary;
            div()
                .id("fluent-title-back")
                .ml(px(4.0))
                .w(BACK_BUTTON_WIDTH)
                .h_full()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(super::radius::SM)
                .text_color(title_color)
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed).text_color(pressed_text))
                .child(Icon::new(IconName::ArrowLeft).size(super::size::ICON))
                .tooltip(super::dialog::tooltip_builder(tooltip))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    handler(window, cx);
                })
        });
        div()
            .id("fluent-title-bar")
            .flex()
            .flex_none()
            .items_center()
            .h(TITLE_BAR_HEIGHT)
            .w_full()
            .bg(p.window_background)
            .children(back)
            .child(
                div()
                    .id("fluent-title-drag")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .pl(if has_back { px(8.0) } else { px(16.0) })
                    .gap(px(16.0))
                    .window_control_area(WindowControlArea::Drag)
                    .when(!self.dialog, |bar| {
                        bar.on_double_click(|_, window, _| window.zoom_window())
                    })
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        down.update(cx, |s, _| s.pressed = true)
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        up.update(cx, |s, _| s.pressed = false)
                    })
                    .on_mouse_down_out(move |_, _, cx| out.update(cx, |s, _| s.pressed = false))
                    .on_mouse_move(move |_, window: &mut Window, cx: &mut App| {
                        if moving.read(cx).pressed {
                            moving.update(cx, |s, _| s.pressed = false);
                            window.start_window_move();
                        }
                    })
                    .on_mouse_down(MouseButton::Right, |event, window, _| {
                        window.show_window_menu(event.position)
                    })
                    .children(self.icon.map(|icon| {
                        div()
                            .size(px(16.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(!active, |d| d.opacity(0.6))
                            .child(icon)
                    }))
                    .child(
                        text(div(), CAPTION)
                            .min_w_0()
                            .truncate()
                            .text_color(title_color)
                            .child(self.title),
                    )
                    .children(
                        self.middle
                            .map(|middle| div().flex_1().flex().justify_center().child(middle)),
                    ),
            )
            .child(
                div().flex().flex_none().h_full().children(
                    caption_buttons(self.dialog).iter().map(|button| {
                        caption_button(*button, maximized, active, forced(*button), p)
                    }),
                ),
            )
    }
}

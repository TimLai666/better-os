//! The window's title bar, with the desktop's header and window buttons.
//!
//! Since ticket 108 (owner, 2026-11-14) the bar looks like every other window
//! on the desktop: GTK draws the header background and the minimize,
//! maximize/restore, and close buttons from the current theme (see
//! [`crate::system_frame`]), in the order and on the side
//! `org.gnome.desktop.wm.preferences button-layout` names, and the bar is as
//! tall as GTK's header bar. Double, middle, and right clicks on its empty
//! space do what `action-*-click-titlebar` say; dragging moves the window, and
//! the compositor restores a maximized window that is dragged. What sits
//! between the button groups (the back button, the app's icon and title, and
//! any controls the app puts there) stays Fluent.
//!
//! A window with page history puts a back button at the bar's leading edge,
//! where Windows 11 Settings has it: a 40px subtle button with a 16px arrow,
//! there only while there is somewhere to go back to. Alt+Left and the
//! mouse's back button do the same; [`is_back_shortcut`] is the key rule.

use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Corners, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, RenderImage, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
    canvas, div, img, point, px, size,
};
use gpui_component::{Icon, IconName};

use super::window_frame::{FrameLook, frame_look, paint_strip, rgba, window_state};
use super::words::Locale;
use super::{BODY_STRONG, Handler, Interaction, ShownState, text, with_alpha};
pub use crate::system_frame::{ButtonLayout, WindowButton};
use crate::system_frame::{ButtonState, FallbackLook, Insets, TitlebarClick, TitlebarResponse};

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

/// The buttons a bar draws: the desktop's layout, or close alone for a
/// dialog, on the side the desktop puts close.
pub fn bar_buttons(layout: &ButtonLayout, dialog: bool) -> ButtonLayout {
    if dialog {
        layout.dialog()
    } else {
        layout.clone()
    }
}

/// What a click on the bar's empty space does in this bar. A dialog does not
/// maximize, as a GTK dialog does not.
pub fn bar_response(response: TitlebarResponse, dialog: bool) -> TitlebarResponse {
    if dialog && response == TitlebarResponse::ToggleMaximize {
        TitlebarResponse::Nothing
    } else {
        response
    }
}

/// The titlebar. Put it first in the window's root column.
#[derive(IntoElement)]
pub struct TitleBar {
    icon: Option<AnyElement>,
    title: SharedString,
    /// Content between the title and the end buttons, such as a search box.
    middle: Option<AnyElement>,
    /// Forces a button's state, for the gallery.
    shown: Option<(WindowButton, ShownState)>,
    /// Draws the bar as if the window were inactive, for the gallery.
    inactive: bool,
    /// A dialog's bar: the close button alone, and no maximize on a double
    /// click.
    dialog: bool,
    /// The back button's handler and tooltip, while there is history.
    back: Option<(Handler, SharedString)>,
    /// A layout other than the desktop's, for the gallery.
    layout: Option<ButtonLayout>,
    /// Light or dark other than the window's, for the gallery.
    dark: Option<bool>,
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
            layout: None,
            dark: None,
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

    pub fn shown(mut self, button: WindowButton, state: ShownState) -> Self {
        self.shown = Some((button, state));
        self
    }

    pub fn inactive(mut self) -> Self {
        self.inactive = true;
        self
    }

    /// Draws `layout` rather than the desktop's (the gallery).
    pub fn layout(mut self, layout: ButtonLayout) -> Self {
        self.layout = Some(layout);
        self
    }

    /// Draws the bar light or dark whatever the window is (the gallery).
    pub fn appearance(mut self, dark: bool) -> Self {
        self.dark = Some(dark);
        self
    }
}

/// Remembers a press on the bar so that the first move starts a window move.
#[derive(Default)]
struct DragState {
    /// Where the left button went down, until it comes up or a move starts.
    pressed: Option<(f32, f32)>,
}

fn act(response: TitlebarResponse, position: gpui::Point<Pixels>, window: &mut Window) {
    match response {
        TitlebarResponse::ToggleMaximize => window.zoom_window(),
        TitlebarResponse::Minimize => window.minimize_window(),
        TitlebarResponse::WindowMenu => window.show_window_menu(position),
        TitlebarResponse::Nothing => {}
    }
}

fn press(button: WindowButton, window: &mut Window) {
    match button {
        WindowButton::Minimize => window.minimize_window(),
        WindowButton::Maximize => window.zoom_window(),
        WindowButton::Close => window.remove_window(),
    }
}

/// The button state the gallery forces, if any.
pub fn forced_button_state(shown: ShownState) -> Option<ButtonState> {
    shown.forced().map(|state| match state {
        Interaction::PointerOver => ButtonState::Hover,
        Interaction::Pressed => ButtonState::Pressed,
        Interaction::Rest | Interaction::Disabled => ButtonState::Normal,
    })
}

fn state_index(state: ButtonState) -> usize {
    match state {
        ButtonState::Normal => 0,
        ButtonState::Hover => 1,
        ButtonState::Pressed => 2,
    }
}

fn button_shell(button: WindowButton, width: f32, height: f32) -> gpui::Stateful<gpui::Div> {
    let group: SharedString = format!("better-title-button-{}", button.css_class()).into();
    div()
        .id(group.clone())
        .group(group)
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            if !event.is_keyboard() {
                press(button, window);
            }
        })
}

/// One window button: GTK's three images stacked, the one for the pointer's
/// state showing.
fn gtk_button(
    button: WindowButton,
    images: &[[Arc<RenderImage>; 2]; 3],
    size: (f32, f32),
    active: bool,
    forced: Option<ButtonState>,
) -> AnyElement {
    let pick = if active { 0 } else { 1 };
    let group: SharedString = format!("better-title-button-{}", button.css_class()).into();
    let layer = |state: ButtonState| -> AnyElement {
        let image = img(images[state_index(state)][pick].clone())
            .absolute()
            .top_0()
            .left_0()
            .w(px(size.0))
            .h(px(size.1));
        let holder = div()
            .id(SharedString::from(format!(
                "{group}-{}",
                state_index(state)
            )))
            .absolute()
            .top_0()
            .left_0()
            .size_full();
        match (forced, state) {
            (Some(f), s) if f == s => holder.child(image).into_any_element(),
            (Some(_), _) => holder.into_any_element(),
            (None, ButtonState::Normal) => holder
                .group_hover(group.clone(), |s| s.opacity(0.0))
                .child(image)
                .into_any_element(),
            (None, ButtonState::Hover) => holder
                .opacity(0.0)
                .group_hover(group.clone(), |s| s.opacity(1.0))
                .group_active(group.clone(), |s| s.opacity(0.0))
                .child(image)
                .into_any_element(),
            (None, ButtonState::Pressed) => holder
                .opacity(0.0)
                .group_active(group.clone(), |s| s.opacity(1.0))
                .child(image)
                .into_any_element(),
        }
    };
    button_shell(button, size.0, size.1)
        .children(ButtonState::ALL.map(layer))
        .into_any_element()
}

/// One window button in Adwaita's look, for when GTK is not there.
fn fallback_button(
    button: WindowButton,
    maximized: bool,
    look: &FallbackLook,
    forced: Option<ButtonState>,
) -> AnyElement {
    let text = rgba(look.text);
    let fill = |state: ButtonState| with_alpha(text, text.a * look.button_fill[state_index(state)]);
    let icon = match (button, maximized) {
        (WindowButton::Minimize, _) => IconName::WindowMinimize,
        (WindowButton::Maximize, false) => IconName::WindowMaximize,
        (WindowButton::Maximize, true) => IconName::WindowRestore,
        (WindowButton::Close, _) => IconName::WindowClose,
    };
    let group: SharedString = format!("better-title-button-{}", button.css_class()).into();
    let (hover, pressed) = (fill(ButtonState::Hover), fill(ButtonState::Pressed));
    let mut image = div()
        .id("image")
        .size(px(look.button_image))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(fill(forced.unwrap_or(ButtonState::Normal)))
        .text_color(text)
        .child(Icon::new(icon).size(px(12.0)));
    if forced.is_none() {
        image = image
            .group_hover(group.clone(), move |s| s.bg(hover))
            .group_active(group, move |s| s.bg(pressed));
    }
    button_shell(button, look.button_size, look.button_size)
        .flex()
        .items_center()
        .justify_center()
        .child(image)
        .into_any_element()
}

/// The header background behind the bar's contents: GTK's header bar, its
/// top corners cut by the window's radius unless the window is maximized or
/// tiled.
fn header_background(look: &FrameLook, active: bool, square: bool) -> AnyElement {
    match look {
        FrameLook::Gtk(rendered, images) => {
            let set = images.header[if active { 0 } else { 1 }].clone();
            let r = if square { 0.0 } else { rendered.corner_radius };
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let h = bounds.size.height;
                    let (x, y, w) = (bounds.origin.x, bounds.origin.y, bounds.size.width);
                    if r > 0.0 && w > px(2.0 * r) {
                        let corner = |at: Pixels| Bounds::new(point(at, y), size(px(r), h));
                        let _ = window.paint_image(
                            corner(x),
                            corner(x),
                            Corners::default(),
                            set[0].clone(),
                            0,
                            false,
                        );
                        let right = corner(x + w - px(r));
                        let _ = window.paint_image(
                            right,
                            right,
                            Corners::default(),
                            set[2].clone(),
                            0,
                            false,
                        );
                        let middle = Bounds::new(point(x + px(r), y), size(w - px(2.0 * r), h));
                        paint_strip(window, &set[1], middle, true);
                    } else {
                        paint_strip(window, &set[1], bounds, true);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element()
        }
        FrameLook::Fallback(look) => {
            let fill = if active {
                look.header
            } else {
                look.header_backdrop
            };
            let radius = if square { 0.0 } else { look.corner_radius };
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .bg(rgba(fill))
                .rounded_tl(px(radius))
                .rounded_tr(px(radius))
                .border_b_1()
                .border_color(rgba(look.header_shade))
                .into_any_element()
        }
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (look, settings) = frame_look(window, cx, self.dark);
        let state = window_state(window);
        let active = state.active && !self.inactive;
        let maximized = state.maximized;
        let layout = bar_buttons(
            self.layout.as_ref().unwrap_or(&settings.button_layout),
            self.dialog,
        );
        let shown = self.shown;
        let forced = move |button: WindowButton| {
            shown
                .filter(|(b, _)| *b == button)
                .and_then(|(_, state)| forced_button_state(state))
        };
        let (height, text_color, spacing, padding, opacity) = match &look {
            FrameLook::Gtk(rendered, _) => (
                rendered.header_height,
                rgba(rendered.header_text[if active { 0 } else { 1 }]),
                rendered.button_spacing,
                rendered.header_padding,
                if active {
                    1.0
                } else {
                    rendered.backdrop_opacity
                },
            ),
            FrameLook::Fallback(look) => (
                look.header_height,
                rgba(look.text),
                look.button_spacing,
                // libadwaita's `headerbar > windowhandle > box` padding, less
                // the header's -1px side margins.
                Insets {
                    top: 6.0,
                    right: 6.0,
                    bottom: 7.0,
                    left: 6.0,
                },
                if active { 1.0 } else { look.backdrop_opacity },
            ),
        };
        let group = |buttons: &[WindowButton]| -> Option<AnyElement> {
            if buttons.is_empty() {
                return None;
            }
            let elements: Vec<AnyElement> = buttons
                .iter()
                .map(|button| match &look {
                    FrameLook::Gtk(rendered, images) => {
                        let restore = *button == WindowButton::Maximize && maximized;
                        match images.buttons.get(&(*button, restore)) {
                            Some(set) => gtk_button(
                                *button,
                                set,
                                rendered.button_size,
                                active,
                                forced(*button),
                            ),
                            None => div().into_any_element(),
                        }
                    }
                    FrameLook::Fallback(look) => {
                        fallback_button(*button, maximized, look, forced(*button))
                    }
                })
                .collect();
            Some(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(spacing))
                    .opacity(opacity)
                    .children(elements)
                    .into_any_element(),
            )
        };
        let start = group(&layout.start);
        let end = group(&layout.end);
        let background = header_background(&look, active, maximized || state.tiled.any());
        let drag = window.use_state(cx, |_, _| DragState::default());
        let (down, up, out, moving) = (drag.clone(), drag.clone(), drag.clone(), drag);
        let dialog = self.dialog;
        let (double, middle, right) = (
            settings.titlebar_response(TitlebarClick::Double),
            settings.titlebar_response(TitlebarClick::Middle),
            settings.titlebar_response(TitlebarClick::Right),
        );
        let has_back = self.back.is_some();
        let back = self.back.map(|(handler, tooltip)| {
            let p = super::palette(cx);
            let hover = p.subtle_fill;
            let pressed = p.subtle_fill_tertiary;
            div()
                .id("fluent-title-back")
                .w(BACK_BUTTON_WIDTH)
                .h(px(32.0))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(super::radius::SM)
                .text_color(text_color)
                .opacity(opacity)
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
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
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .h(px(height))
            .w_full()
            .pl(px(padding.left))
            .pr(px(padding.right))
            .pt(px(padding.top))
            .pb(px(padding.bottom))
            .gap(px(6.0))
            .child(background)
            .children(start)
            .children(back)
            .child(
                div()
                    .id("fluent-title-drag")
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .items_center()
                    .pl(if has_back { px(4.0) } else { px(6.0) })
                    .gap(px(12.0))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        if event.click_count == 2 {
                            down.update(cx, |s, _| s.pressed = None);
                            act(bar_response(double, dialog), event.position, window);
                        } else {
                            let at = (f32::from(event.position.x), f32::from(event.position.y));
                            down.update(cx, |s, _| s.pressed = Some(at));
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        up.update(cx, |s, _| s.pressed = None)
                    })
                    .on_mouse_down_out(move |_, _, cx| out.update(cx, |s, _| s.pressed = None))
                    .on_mouse_move(move |event, window: &mut Window, cx: &mut App| {
                        // A press becomes a move only past GTK's drag
                        // threshold, so a slightly shaky click stays a click.
                        let Some(from) = moving.read(cx).pressed else {
                            return;
                        };
                        let to = (f32::from(event.position.x), f32::from(event.position.y));
                        if crate::system_frame::is_drag(from, to) {
                            moving.update(cx, |s, _| s.pressed = None);
                            window.start_window_move();
                        }
                    })
                    .on_mouse_down(MouseButton::Middle, move |event, window, _| {
                        act(bar_response(middle, dialog), event.position, window);
                    })
                    .on_mouse_down(MouseButton::Right, move |event, window, _| {
                        act(bar_response(right, dialog), event.position, window);
                    })
                    .children(self.icon.map(|icon| {
                        div()
                            .size(px(16.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(opacity)
                            .child(icon)
                    }))
                    .child(
                        text(div(), BODY_STRONG)
                            .min_w_0()
                            .truncate()
                            .opacity(opacity)
                            .text_color(text_color)
                            .child(self.title),
                    )
                    .children(
                        self.middle
                            .map(|middle| div().flex_1().flex().justify_center().child(middle)),
                    ),
            )
            .children(end)
    }
}

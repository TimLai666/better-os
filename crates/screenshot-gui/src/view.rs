//! The surfaces: the interface over each output ([`OutputView`]), the
//! shading and the bar while recording ([`ShadeView`], [`BarView`]), and the
//! portal's flash ([`FlashView`]). Each reads the [`Shooter`] and sends what
//! the person does back to it.

use std::time::Duration;

use better_ui::fluent::button::{Button, ButtonKind};
use better_ui::fluent::{self, Palette};
use better_ui::icon_set::Glyph;
use better_ui::layer::{self, InputRegion};
use better_ui::restart::{Blocker, Restartable, UpdateRestart};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, CursorStyle, DispatchPhase, Entity,
    FocusHandle, Focusable, HitboxBehavior, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Render,
    Rgba, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, canvas,
    div, img, px, rgba, svg,
};
use screenshot_core::area::{Button as AreaButton, Grip};
use screenshot_core::geometry::{Point, Rect};
use screenshot_core::interface::{Command, Direction, Key, Kind, Mode, navigate};
use screenshot_core::recording::elapsed_label;

use crate::app::{APP_ID, FADE, Open, Shooter, Use, ease_out_quad};
use crate::assets::path;
use crate::restart::{ScreenshotView, restart_blocked};

/// GNOME's shading inside the interface and while recording
/// (`_screenshot.scss`: `rgba(0,0,0,.5)` in the selector, `.3` outside it).
const SHADE: u32 = 0x0000_0080;
const RECORDING_SHADE: u32 = 0x0000_004d;
/// GNOME's red for the recording capture button (`$red_4`).
const RECORD_RED: u32 = 0xe0_1b_24_ff;

fn grip_cursor(grip: Grip, dragging: bool) -> CursorStyle {
    match grip {
        Grip::Crosshair => CursorStyle::Crosshair,
        Grip::Move if dragging => CursorStyle::ClosedHand,
        Grip::Move => CursorStyle::OpenHand,
        Grip::North | Grip::South => CursorStyle::ResizeUpDown,
        Grip::East | Grip::West => CursorStyle::ResizeLeftRight,
        Grip::NorthEast | Grip::SouthWest => CursorStyle::ResizeUpRightDownLeft,
        Grip::NorthWest | Grip::SouthEast => CursorStyle::ResizeUpLeftDownRight,
    }
}

fn key_of(event: &KeyDownEvent) -> (Key, bool) {
    let keystroke = &event.keystroke;
    let key = match keystroke.key.as_str() {
        "enter" | "kp_enter" | "return" => Key::Enter,
        "space" => Key::Space,
        "escape" => Key::Escape,
        "left" => Key::Arrow(Direction::Left),
        "right" => Key::Arrow(Direction::Right),
        "up" => Key::Arrow(Direction::Up),
        "down" => Key::Arrow(Direction::Down),
        letter if letter.chars().count() == 1 => letter
            .chars()
            .next()
            .filter(char::is_ascii_alphabetic)
            .map(|c| Key::Letter(c.to_ascii_lowercase()))
            .unwrap_or(Key::Other),
        _ => Key::Other,
    };
    (key, keystroke.modifiers.control)
}

impl Shooter {
    /// The pointer shape over an output.
    fn cursor(&self, output: usize, at: Option<Point>) -> CursorStyle {
        let Some(open) = &self.open else {
            return CursorStyle::Arrow;
        };
        if open.using == Use::PickColor {
            return CursorStyle::Crosshair;
        }
        match open.interface.kind {
            Kind::Selection => grip_cursor(open.grip, open.area.is_dragging()),
            Kind::Screen => CursorStyle::PointingHand,
            Kind::Window => {
                let over = at.is_some_and(|point| {
                    open.windows
                        .iter()
                        .any(|window| window.output == output && window.rect.contains(point))
                });
                if over {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::Arrow
                }
            }
        }
    }

    fn pointer_down(
        &mut self,
        point: Point,
        button: MouseButton,
        ctrl: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.open.as_mut().filter(|open| !open.closing) else {
            return;
        };
        if open.using == Use::PickColor {
            if button == MouseButton::Left {
                self.pick(point, cx);
            }
            return;
        }
        let button = match button {
            MouseButton::Left => AreaButton::Primary,
            MouseButton::Right => AreaButton::Secondary,
            _ => return,
        };
        match open.interface.kind {
            Kind::Selection => {
                open.area.press(point, button, ctrl);
                if let Some(grip) = open.area.drag_grip() {
                    open.grip = grip;
                }
            }
            Kind::Screen => {
                if let Some(index) = open.output_at(point) {
                    open.interface.screen = index;
                }
            }
            Kind::Window => {
                if let Some(index) = open.windows.iter().position(|w| w.rect.contains(point)) {
                    open.interface.window = Some(index);
                }
            }
        }
        cx.notify();
    }

    fn pointer_move(&mut self, point: Point, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut().filter(|open| !open.closing) else {
            return;
        };
        open.pointer_at = Some(point);
        match open.interface.kind {
            Kind::Selection => open.grip = open.area.motion(point, open.stage),
            Kind::Screen => open.hover_screen = open.output_at(point),
            Kind::Window => {
                open.hover_window = open.windows.iter().position(|w| w.rect.contains(point));
            }
        }
        cx.notify();
    }

    fn pointer_up(&mut self, point: Point, button: MouseButton, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut().filter(|open| !open.closing) else {
            return;
        };
        let button = match button {
            MouseButton::Left => AreaButton::Primary,
            MouseButton::Right => AreaButton::Secondary,
            _ => return,
        };
        if open.interface.kind == Kind::Selection {
            open.area.release(button, open.stage);
            open.grip = open.area.grip_at(point);
        }
        cx.notify();
    }

    fn key(&mut self, key: Key, ctrl: bool, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut().filter(|open| !open.closing) else {
            return;
        };
        if open.using == Use::PickColor {
            if key == Key::Escape {
                self.close_interface(cx);
            }
            return;
        }
        match open.interface.command_for(key, ctrl) {
            Command::Capture => self.capture(cx),
            Command::Close => self.close_interface(cx),
            Command::Navigate(direction) => {
                let rects = open.choices();
                match open.interface.kind {
                    Kind::Screen => {
                        if let Some(next) = navigate(&rects, Some(open.interface.screen), direction)
                        {
                            open.interface.screen = next;
                        }
                    }
                    Kind::Window => {
                        open.interface.window = navigate(&rects, open.interface.window, direction);
                    }
                    Kind::Selection => {}
                }
                cx.notify();
            }
            command => {
                if open.interface.apply(command) {
                    cx.notify();
                }
            }
        }
    }

    fn set_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open
            && open.interface.choose(kind)
        {
            cx.notify();
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open
            && open.interface.set_mode(mode)
        {
            cx.notify();
        }
    }

    fn toggle_pointer(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.open {
            open.interface.show_pointer = !open.interface.show_pointer;
            cx.notify();
        }
    }
}

/// The interface over one output.
pub struct OutputView {
    shooter: Entity<Shooter>,
    output: usize,
    focus: FocusHandle,
    restart: UpdateRestart,
    _observe: Subscription,
}

impl OutputView {
    /// `panel` says whether this output shows the panel; it is passed in
    /// because the controller is being updated while it opens surfaces.
    pub fn new(
        shooter: Entity<Shooter>,
        output: usize,
        panel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> OutputView {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let appearance = better_ui::fluent::theme::Appearance::default_path("screenshot")
            .map(|path| better_ui::fluent::theme::Appearance::load(&path))
            .unwrap_or_default();
        appearance.apply_and_follow(window, cx);
        let observe = cx.observe(&shooter, |_, _, cx| cx.notify());
        let restart = if panel {
            UpdateRestart::new(
                APP_ID,
                "Better Screenshot",
                crate::app::PACKAGE,
                env!("CARGO_PKG_VERSION"),
                None,
                window,
                cx,
            )
        } else {
            UpdateRestart::inactive(APP_ID, cx)
        };
        OutputView {
            shooter,
            output,
            focus,
            restart,
            _observe: observe,
        }
    }
}

impl Focusable for OutputView {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Restartable for OutputView {
    type State = ScreenshotView;

    fn update_restart(&mut self) -> &mut UpdateRestart {
        &mut self.restart
    }

    fn restart_blocked(&self, cx: &App) -> Option<Blocker> {
        restart_blocked(&self.shooter.read(cx).holds())
    }

    fn restart_state(&self, _: &Window, _: &App) -> ScreenshotView {
        ScreenshotView::default()
    }
}

fn rect_div(rect: Rect, origin: Rect) -> gpui::Div {
    div()
        .absolute()
        .left(px((rect.x - origin.x) as f32))
        .top(px((rect.y - origin.y) as f32))
        .w(px(rect.width as f32))
        .h(px(rect.height as f32))
}

/// The four rectangles around `hole` inside `bounds`, both relative to the
/// output.
fn around(hole: Option<Rect>, output: Rect, color: Rgba) -> Vec<AnyElement> {
    let local = Rect::new(output.x, output.y, output.width, output.height);
    let Some(hole) = hole.and_then(|hole| hole.intersection(&local)) else {
        return vec![rect_div(local, output).bg(color).into_any_element()];
    };
    let pieces = [
        Rect::new(local.x, local.y, local.width, hole.y - local.y),
        Rect::new(
            local.x,
            hole.bottom(),
            local.width,
            local.bottom() - hole.bottom(),
        ),
        Rect::new(local.x, hole.y, hole.x - local.x, hole.height),
        Rect::new(
            hole.right(),
            hole.y,
            local.right() - hole.right(),
            hole.height,
        ),
    ];
    pieces
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .map(|piece| rect_div(piece, output).bg(color).into_any_element())
        .collect()
}

fn icon(path_name: &'static str) -> Glyph {
    Glyph::Path(SharedString::new_static(path_name))
}

impl OutputView {
    fn events(&self, origin: Rect) -> impl IntoElement {
        let shooter = self.shooter.clone();
        let output = self.output;
        canvas(
            move |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, cx| {
                let at = window.mouse_position();
                let global = move |position: gpui::Point<Pixels>| {
                    Point::new(
                        f64::from(origin.x) + f64::from(f32::from(position.x)),
                        f64::from(origin.y) + f64::from(f32::from(position.y)),
                    )
                };
                let style = shooter.read(cx).cursor(output, Some(global(at)));
                window.set_cursor_style(style, &hitbox);
                let down = shooter.clone();
                let over = hitbox.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !over.is_hovered(window) {
                        return;
                    }
                    let point = global(event.position);
                    down.update(cx, |shooter, cx| {
                        shooter.pointer_down(point, event.button, event.modifiers.control, cx)
                    });
                });
                let moved = shooter.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let point = global(event.position);
                    moved.update(cx, |shooter, cx| shooter.pointer_move(point, cx));
                });
                let up = shooter.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let point = global(event.position);
                    up.update(cx, |shooter, cx| {
                        shooter.pointer_up(point, event.button, cx)
                    });
                });
            },
        )
        .absolute()
        .size_full()
    }

    fn selection(&self, open: &Open, origin: Rect) -> Vec<AnyElement> {
        let area = open.area.geometry();
        let mut out = around(Some(area), origin, rgba(SHADE));
        if area.intersection(&origin).is_some() {
            out.push(
                rect_div(area, origin)
                    .border_2()
                    .border_color(rgba(0xffff_ffff))
                    .into_any_element(),
            );
            let half = (screenshot_core::area::HANDLE_SIZE / 2.0) as i32;
            let size = screenshot_core::area::HANDLE_SIZE as i32;
            let (left, top) = (area.x, area.y);
            let (right, bottom) = (area.x + area.width - 1, area.y + area.height - 1);
            for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
                out.push(
                    rect_div(Rect::new(x - half, y - half, size, size), origin)
                        .rounded_full()
                        .bg(rgba(0xffff_ffff))
                        .shadow_md()
                        .into_any_element(),
                );
            }
        }
        out
    }

    fn screens(&self, open: &Open, origin: Rect) -> Vec<AnyElement> {
        let chosen = open.interface.screen == self.output;
        let hovered = open.hover_screen == Some(self.output);
        if chosen {
            vec![
                rect_div(origin, origin)
                    .border_2()
                    .border_color(rgba(0xffff_ffff))
                    .into_any_element(),
            ]
        } else {
            let shade = if hovered { 0x0000_004d } else { SHADE };
            vec![rect_div(origin, origin).bg(rgba(shade)).into_any_element()]
        }
    }

    fn windows(&self, open: &Open, origin: Rect, p: &Palette, cx: &App) -> Vec<AnyElement> {
        let words = &self.shooter.read(cx).words;
        let mut out = vec![
            rect_div(origin, origin)
                .bg(p.window_background)
                .into_any_element(),
        ];
        let mine: Vec<(usize, &crate::app::WindowPlace)> = open
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.output == self.output)
            .collect();
        if mine.is_empty() {
            out.push(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(p.text_secondary)
                    .child(words.no_windows_here)
                    .into_any_element(),
            );
        }
        for (index, window) in mine {
            let chosen = open.interface.window == Some(index);
            let hovered = open.hover_window == Some(index);
            let image = if open.interface.show_pointer {
                window.image_pointer.clone()
            } else {
                window.image.clone()
            };
            let border = 6;
            let frame = Rect::new(
                window.rect.x - border,
                window.rect.y - border,
                window.rect.width + 2 * border,
                window.rect.height + 2 * border,
            );
            let mut accent_soft = p.accent;
            accent_soft.a = 0.2;
            out.push(
                rect_div(frame, origin)
                    .rounded(px(12.0))
                    .border(px(border as f32))
                    .border_color(if chosen {
                        p.accent
                    } else if hovered {
                        p.accent_tertiary
                    } else {
                        rgba(0x0000_0000)
                    })
                    .when(chosen, |frame| frame.bg(accent_soft))
                    .into_any_element(),
            );
            out.push(
                rect_div(window.rect, origin)
                    .child(img(image).size_full())
                    .into_any_element(),
            );
            if chosen {
                let badge = 48;
                let center = window.rect.center();
                out.push(
                    rect_div(
                        Rect::new(
                            center.x as i32 - badge / 2,
                            center.y as i32 - badge / 2,
                            badge,
                            badge,
                        ),
                        origin,
                    )
                    .rounded_full()
                    .bg(p.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(path::CHECK)
                            .size(px(24.0))
                            .text_color(p.text_on_accent),
                    )
                    .into_any_element(),
                );
            }
        }
        out
    }

    fn panel(&self, open: &Open, p: &Palette, cx: &App) -> impl IntoElement {
        let shooter = self.shooter.clone();
        let words = self.shooter.read(cx).words;
        let interface = &open.interface;
        let dragging = open.area.is_dragging();
        let recording = interface.mode == Mode::Recording;

        let kind_button = |id: &'static str,
                           kind: Kind,
                           label: &'static str,
                           tip: &'static str,
                           glyph: &'static str,
                           enabled: bool| {
            let shooter = shooter.clone();
            Button::toggle(id, label, interface.kind == kind)
                .icon(icon(glyph))
                .tooltip(tip)
                .disabled(!enabled)
                .on_click(move |_, cx| shooter.update(cx, |s, cx| s.set_kind(kind, cx)))
        };
        let mode_button = |id: &'static str,
                           mode: Mode,
                           tip: &'static str,
                           glyph: &'static str,
                           enabled: bool| {
            let shooter = shooter.clone();
            Button::new(id, ButtonKind::Subtle)
                .icon(icon(glyph))
                .checked(interface.mode == mode)
                .tooltip(tip)
                .disabled(!enabled)
                .on_click(move |_, cx| shooter.update(cx, |s, cx| s.set_mode(mode, cx)))
        };

        let capture = {
            let shooter = shooter.clone();
            let ring = p.text_primary;
            let fill = if recording {
                rgba(RECORD_RED)
            } else {
                p.text_primary
            };
            div()
                .id("capture")
                .size(px(56.0))
                .rounded_full()
                .border(px(4.0))
                .border_color(ring)
                .p(px(4.0))
                .cursor_pointer()
                .tooltip(fluent::dialog::tooltip_builder(words.capture_tip.into()))
                .on_click(move |_, _, cx| shooter.update(cx, |s, cx| s.capture(cx)))
                .child(
                    div()
                        .size_full()
                        .rounded_full()
                        .bg(fill)
                        .hover(|circle| circle.opacity(0.8)),
                )
        };

        let pointer = {
            let shooter = shooter.clone();
            Button::new("pointer", ButtonKind::Subtle)
                .icon(icon(path::POINTER))
                .checked(interface.show_pointer)
                .tooltip(words.pointer_tip)
                .on_click(move |_, cx| shooter.update(cx, |s, cx| s.toggle_pointer(cx)))
        };

        let close = {
            let shooter = shooter.clone();
            // GNOME's close button is round, like a window's
            // (`.screenshot-ui-close-button` extends `.window-close`).
            div()
                .id("close")
                .size(px(32.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(p.flyout)
                .border_1()
                .border_color(p.card_stroke)
                .shadow_md()
                .cursor_pointer()
                .hover(|button| button.bg(p.control_fill_secondary))
                .tooltip(fluent::dialog::tooltip_builder(words.close_tip.into()))
                .on_click(move |_, _, cx| shooter.update(cx, |s, cx| s.close_interface(cx)))
                .child(
                    svg()
                        .path(path::CLOSE)
                        .size(px(16.0))
                        .text_color(p.text_primary),
                )
        };

        let close_on_left = self.shooter.read(cx).close_on_left;
        let body = div()
            .id("panel")
            .occlude()
            .relative()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(18.0))
            .pb(px(12.0))
            .rounded(px(24.0))
            .bg(p.flyout)
            .border_1()
            .border_color(p.card_stroke)
            .shadow_lg()
            .text_color(p.text_primary)
            .child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(kind_button(
                        "selection",
                        Kind::Selection,
                        words.selection,
                        words.selection_tip,
                        path::AREA,
                        true,
                    ))
                    .child(kind_button(
                        "screen",
                        Kind::Screen,
                        words.screen,
                        words.screen_tip,
                        path::SCREEN,
                        true,
                    ))
                    .child(kind_button(
                        "window",
                        Kind::Window,
                        words.window,
                        words.window_tip,
                        path::WINDOW,
                        interface.window_available(),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .p(px(3.0))
                            .rounded_full()
                            .bg(p.control_fill)
                            .child(mode_button(
                                "picture",
                                Mode::Picture,
                                words.picture_tip,
                                path::CAMERA,
                                true,
                            ))
                            .when(interface.recording_shown(), |pill| {
                                pill.child(mode_button(
                                    "recording",
                                    Mode::Recording,
                                    words.recording_tip,
                                    path::VIDEO,
                                    interface.recording_available(),
                                ))
                            }),
                    )
                    .child(capture)
                    .child(div().w(px(76.0)).flex().justify_end().child(pointer)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(-16.0))
                    .when(close_on_left, |c| c.left(px(-16.0)))
                    .when(!close_on_left, |c| c.right(px(-16.0)))
                    .child(close),
            );

        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(64.0))
            .flex()
            .justify_center()
            .opacity(if dragging { 100.0 / 255.0 } else { 1.0 })
            .child(body)
    }

    fn pick_preview(&self, open: &Open, origin: Rect, p: &Palette) -> Option<AnyElement> {
        let at = open.pointer_at?;
        if !origin.contains(at) {
            return None;
        }
        let pixel = open.pixel_at(at)?;
        let color = gpui::Rgba {
            r: f32::from(pixel[2]) / 255.0,
            g: f32::from(pixel[1]) / 255.0,
            b: f32::from(pixel[0]) / 255.0,
            a: 1.0,
        };
        let hex = format!("#{:02X}{:02X}{:02X}", pixel[2], pixel[1], pixel[0]);
        Some(
            div()
                .absolute()
                .left(px((at.x - f64::from(origin.x)) as f32 + 16.0))
                .top(px((at.y - f64::from(origin.y)) as f32 + 16.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .p(px(6.0))
                .rounded(px(8.0))
                .bg(p.flyout)
                .shadow_md()
                .text_color(p.text_primary)
                .child(
                    div()
                        .size(px(24.0))
                        .rounded(px(4.0))
                        .bg(color)
                        .border_1()
                        .border_color(p.card_stroke),
                )
                .child(hex)
                .into_any_element(),
        )
    }
}

impl Render for OutputView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        let shooter = self.shooter.read(cx);
        let Some(open) = &shooter.open else {
            return div().size_full().into_any_element();
        };
        let Some(output) = open.outputs.get(self.output) else {
            return div().size_full().into_any_element();
        };
        let origin = output.logical;
        let picture_mode = open.interface.mode == Mode::Picture;
        let closing = open.closing;
        let background = if open.interface.show_pointer && open.using == Use::Capture {
            output.pointer.clone()
        } else {
            output.plain.clone()
        };

        let mut root = div()
            .id("screenshot")
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let (key, ctrl) = key_of(event);
                this.shooter
                    .update(cx, |shooter, cx| shooter.key(key, ctrl, cx));
            }))
            // The frozen screen, while taking a picture (O2, P2).
            .when(picture_mode, |root| {
                root.child(img(background).absolute().size_full())
            });

        if open.using == Use::PickColor {
            root = root.children(self.pick_preview(open, origin, p));
        } else {
            let layer: Vec<AnyElement> = match open.interface.kind {
                Kind::Selection => self.selection(open, origin),
                Kind::Screen => self.screens(open, origin),
                Kind::Window => self.windows(open, origin, p, cx),
            };
            root = root.children(layer);
        }
        root = root.child(self.events(origin));
        if open.using == Use::Capture && open.panel_output == self.output {
            let panel = self.panel(open, p, cx);
            root = root.child(panel);
        }
        let toasts = self.restart.toast_layer(
            match shooter.locale {
                screenshot_core::words::Locale::TraditionalChinese => {
                    fluent::words::Locale::TraditionalChinese
                }
                screenshot_core::words::Locale::English => fluent::words::Locale::English,
            },
            cx,
        );
        let _ = window;
        // GNOME fades the interface in and out over 200 ms (`open` 1665,
        // `close` 1710), easing out.
        let root = root.children(toasts);
        if closing {
            root.with_animation(
                "fade-out",
                Animation::new(FADE).with_easing(ease_out_quad),
                |root, delta| root.opacity(1.0 - delta),
            )
            .into_any_element()
        } else {
            root.with_animation(
                "fade-in",
                Animation::new(FADE).with_easing(ease_out_quad),
                |root, delta| root.opacity(delta),
            )
            .into_any_element()
        }
    }
}

/// The shading outside the recorded area on one output (R3). It takes no
/// input.
pub struct ShadeView {
    shooter: Entity<Shooter>,
    key: Option<String>,
}

impl ShadeView {
    pub fn new(
        shooter: Entity<Shooter>,
        key: Option<String>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> ShadeView {
        layer::apply_input_region(window, &InputRegion::Nothing);
        ShadeView { shooter, key }
    }
}

impl Render for ShadeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        layer::apply_input_region(window, &InputRegion::Nothing);
        let shooter = self.shooter.read(cx);
        let output = shooter.output_rect(self.key.as_deref());
        let (Some(run), Some(output)) = (&shooter.run, output) else {
            return div().size_full();
        };
        div()
            .size_full()
            .relative()
            .children(around(Some(run.area), output, rgba(RECORDING_SHADE)))
    }
}

/// The recording bar (R4): a red dot, the time, and the stop button.
pub struct BarView {
    shooter: Entity<Shooter>,
    _observe: Subscription,
}

impl BarView {
    pub fn new(shooter: Entity<Shooter>, window: &mut Window, cx: &mut Context<Self>) -> BarView {
        let appearance = better_ui::fluent::theme::Appearance::default_path("screenshot")
            .map(|path| better_ui::fluent::theme::Appearance::load(&path))
            .unwrap_or_default();
        appearance.apply_and_follow(window, cx);
        let observe = cx.observe(&shooter, |_, _, cx| cx.notify());
        BarView {
            shooter,
            _observe: observe,
        }
    }
}

impl Render for BarView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = fluent::palette(cx);
        let shooter = self.shooter.read(cx);
        let elapsed = shooter
            .run
            .as_ref()
            .map(|run| run.started.elapsed())
            .unwrap_or(Duration::ZERO);
        let words = shooter.words;
        let stop = {
            let shooter = self.shooter.clone();
            Button::icon_button("stop", icon(path::STOP), words.stop_recording)
                .on_click(move |_, cx| shooter.update(cx, |s, cx| s.stop_recording(cx)))
        };
        div().size_full().p(px(2.0)).child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_between()
                .pl(px(14.0))
                .pr(px(4.0))
                .rounded_full()
                .bg(p.flyout)
                .border_1()
                .border_color(p.card_stroke)
                .text_color(p.text_primary)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().size(px(10.0)).rounded_full().bg(rgba(RECORD_RED)))
                        .child(elapsed_label(elapsed)),
                )
                .child(stop),
        )
    }
}

/// The white flash after a portal's instant picture (A2): it fades out over
/// 500 ms (GNOME's `FLASHSPOT_ANIMATION_OUT_TIME`) and takes no input.
pub struct FlashView {
    _done: gpui::Task<()>,
}

/// GNOME's flash length.
pub const FLASH: Duration = Duration::from_millis(500);

impl FlashView {
    pub fn new(shooter: Entity<Shooter>, window: &mut Window, cx: &mut Context<Self>) -> FlashView {
        layer::apply_input_region(window, &InputRegion::Nothing);
        let done = cx.spawn(async move |_, cx| {
            cx.background_executor().timer(FLASH).await;
            shooter.update(cx, |shooter, cx| shooter.flash_done(cx));
        });
        FlashView { _done: done }
    }
}

impl Render for FlashView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgba(0xffff_ffff)).with_animation(
            "flash",
            Animation::new(FLASH),
            |flash, delta| flash.opacity(1.0 - delta),
        )
    }
}

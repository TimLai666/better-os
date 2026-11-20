//! `better-layer-check`: ticket 200's GPUI program. It opens a window as a
//! `zwlr_layer_shell_v1` surface through `better_ui::layer` and prints one
//! line per thing that happens to it, so a shell script can drive it in the
//! nested compositor and read what it saw:
//!
//!   surface output=ID size=WxH scale=S     a configure, a rescale
//!   active output=ID true|false           keyboard focus came or went
//!   click output=ID x=X y=Y               a press inside the input region
//!   key output=ID KEYSTROKE               a key the surface received
//!   popup open|key KEYSTROKE|closed       the popup parented to the surface
//!   toplevel size=WxH | active | click | key   with --toplevel
//!   spec zone=N | anchor=EDGES            a change after creation
//!   error MESSAGE                         the surface could not be opened
//!
//! A development tool, not packaged.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use better_ui::layer::{
    self, Edges, ExclusiveZone, Extent, InputRegion, LayerSurfaces, OutputInfo, Placement, Rect,
    SurfaceSpec,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Bounds, Context, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point,
    Render, Size, Styled as _, Subscription, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, point,
    popup::{PopupAnchor, PopupConstraintAdjustment, PopupGravity, PopupOptions},
    px, rgb,
};
use layer_check::{CheckArgs, HELP, edges_name, parse_args};

fn say(line: String) {
    println!("{line}");
}

struct Surface {
    args: Rc<CheckArgs>,
    output: u64,
    focus: FocusHandle,
    logged: Option<(Size<Pixels>, f32)>,
    region: Option<InputRegion>,
    clicks: usize,
    keys: String,
    active: bool,
    _activation: Subscription,
}

impl Surface {
    fn new(
        args: Rc<CheckArgs>,
        output: &OutputInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = output.id;
        let activation =
            cx.observe_window_activation(window, move |this: &mut Surface, window, cx| {
                this.active = window.is_window_active();
                say(format!("active output={id} {}", this.active));
                cx.notify();
            });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        say(format!("open output={id}"));
        cx.on_release(move |_, _| say(format!("closed output={id}")))
            .detach();
        Surface {
            args,
            output: id,
            focus,
            logged: None,
            region: None,
            clicks: 0,
            keys: String::new(),
            active: false,
            _activation: activation,
        }
    }

    fn open_popup(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let parent = window.window_handle();
        let options = WindowOptions {
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.0), px(0.0)),
                size: Size::new(px(280.0), px(140.0)),
            })),
            kind: WindowKind::AnchoredPopup(PopupOptions {
                parent,
                anchor_rect: Bounds {
                    origin: position,
                    size: Size::new(px(1.0), px(1.0)),
                },
                anchor: PopupAnchor::BottomLeft,
                gravity: PopupGravity::BottomRight,
                constraint_adjustment: PopupConstraintAdjustment::SLIDE_X
                    | PopupConstraintAdjustment::FLIP_Y,
                offset: point(px(0.0), px(0.0)),
                grab: true,
            }),
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("io.betteros.LayerCheck".into()),
            ..Default::default()
        };
        cx.defer(move |cx| {
            match cx.open_window(options, |window, cx| cx.new(|cx| Popup::new(window, cx))) {
                Ok(_) => say("popup open".into()),
                Err(error) => say(format!("error popup {error:#}")),
            }
        });
    }
}

impl Focusable for Surface {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Surface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let scale = window.scale_factor();
        if self.logged != Some((size, scale)) {
            self.logged = Some((size, scale));
            say(format!(
                "surface output={} size={}x{} scale={scale}",
                self.output,
                f32::from(size.width),
                f32::from(size.height)
            ));
        }
        if let Some(left) = self.args.input_left {
            let region = layer::input_region(
                (f32::from(size.width), f32::from(size.height)),
                &[Rect::new(0.0, 0.0, left, f32::from(size.height))],
            );
            if self.region.as_ref() != Some(&region) {
                layer::apply_input_region(window, &region);
                say(format!("region output={} {region:?}", self.output));
                self.region = Some(region);
            }
        }
        let color = self.args.color;
        let status = format!(
            "{}  ·  {}×{} @{}  ·  output {}  ·  {}  ·  clicks {}  ·  keys {}",
            self.args.label,
            f32::from(size.width),
            f32::from(size.height),
            scale,
            self.output,
            if self.active {
                "focused"
            } else {
                "not focused"
            },
            self.clicks,
            if self.keys.is_empty() {
                "-"
            } else {
                &self.keys
            },
        );
        let input_left = self.args.input_left;
        div()
            .id("surface")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(color))
            .text_color(rgb(0xffffff))
            .text_size(px(13.0))
            .flex()
            .items_center()
            .px(px(10.0))
            .child(status)
            .when_some(input_left, |this, left| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(px(left))
                        .border_r_2()
                        .border_color(rgb(0xffd400)),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.clicks += 1;
                    say(format!(
                        "click output={} x={} y={}",
                        this.output,
                        f32::from(event.position.x),
                        f32::from(event.position.y)
                    ));
                    if this.args.popup {
                        this.open_popup(event.position, window, cx);
                    }
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.unparse();
                say(format!("key output={} {key}", this.output));
                this.keys
                    .push_str(event.keystroke.key_char.as_deref().unwrap_or(""));
                cx.notify();
            }))
    }
}

struct Popup {
    focus: FocusHandle,
    typed: String,
}

impl Popup {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.on_release(|_, _| say("popup closed".into())).detach();
        Popup {
            focus,
            typed: String::new(),
        }
    }
}

impl Render for Popup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("popup")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0xfafafa))
            .border_2()
            .border_color(rgb(0xd9467a))
            .text_color(rgb(0x202020))
            .text_size(px(14.0))
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child("Popup parented to the layer surface")
            .child(format!(
                "typed: {}",
                if self.typed.is_empty() {
                    "-"
                } else {
                    &self.typed
                }
            ))
            .child("Escape closes it")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.unparse();
                say(format!("popup key {key}"));
                if key == "escape" {
                    window.remove_window();
                    return;
                }
                this.typed
                    .push_str(event.keystroke.key_char.as_deref().unwrap_or(""));
                cx.notify();
            }))
    }
}

struct Toplevel {
    focus: FocusHandle,
    logged: Option<Size<Pixels>>,
    active: bool,
    keys: String,
    _activation: Subscription,
}

impl Render for Toplevel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        if self.logged != Some(size) {
            self.logged = Some(size);
            say(format!(
                "toplevel size={}x{} scale={}",
                f32::from(size.width),
                f32::from(size.height),
                window.scale_factor()
            ));
        }
        div()
            .id("toplevel")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0xe8e3d9))
            .border_4()
            .border_color(rgb(0x1a9e5c))
            .text_color(rgb(0x202020))
            .text_size(px(16.0))
            .p(px(16.0))
            .child(format!(
                "Ordinary window · {}×{} · {} · keys {}",
                f32::from(size.width),
                f32::from(size.height),
                if self.active {
                    "focused"
                } else {
                    "not focused"
                },
                if self.keys.is_empty() {
                    "-"
                } else {
                    &self.keys
                }
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, event: &MouseDownEvent, _, _| {
                    say(format!(
                        "toplevel click x={} y={}",
                        f32::from(event.position.x),
                        f32::from(event.position.y)
                    ));
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                say(format!("toplevel key {}", event.keystroke.unparse()));
                this.keys
                    .push_str(event.keystroke.key_char.as_deref().unwrap_or(""));
                cx.notify();
            }))
    }
}

fn open_toplevel(args: &CheckArgs, cx: &mut App) {
    let size = Size::new(px(720.0), px(420.0));
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size,
    };
    let options = WindowOptions {
        window_bounds: Some(if args.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }),
        app_id: Some("io.betteros.LayerCheck".into()),
        ..Default::default()
    };
    let result = cx.open_window(options, |window, cx| {
        cx.new(|cx| {
            let activation =
                cx.observe_window_activation(window, |this: &mut Toplevel, window, cx| {
                    this.active = window.is_window_active();
                    say(format!("toplevel active {}", this.active));
                    cx.notify();
                });
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Toplevel {
                focus,
                logged: None,
                active: false,
                keys: String::new(),
                _activation: activation,
            }
        })
    });
    if let Err(error) = result {
        say(format!("error toplevel {error:#}"));
    }
}

fn after(seconds: f32, cx: &mut App, f: impl FnOnce(&mut App) + 'static) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs_f32(seconds))
            .await;
        cx.update(f);
    })
    .detach();
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|arg| arg == "--help") {
        println!("{HELP}");
        return;
    }
    let args = match parse_args(arguments) {
        Ok(args) => Rc::new(args),
        Err(error) => {
            eprintln!("better-layer-check: {error}");
            std::process::exit(2);
        }
    };

    gpui_platform::application().run(move |cx: &mut App| {
        if args.toplevel {
            open_toplevel(&args, cx);
            return;
        }
        let placement = if args.every_output {
            Placement::EveryOutput
        } else {
            Placement::One { preferred: None }
        };
        let build_args = args.clone();
        let surfaces = match LayerSurfaces::open(
            "io.betteros.LayerCheck",
            args.spec.clone(),
            placement,
            move |output, window, cx| Surface::new(build_args.clone(), output, window, cx),
            cx,
        ) {
            Ok(surfaces) => Rc::new(RefCell::new(surfaces)),
            Err(error) => {
                say(format!("error spec {error:?}"));
                cx.quit();
                return;
            }
        };
        if let Some(error) = surfaces.borrow().last_error() {
            say(format!("error {error}"));
        }

        if let Some((seconds, pixels)) = args.zone_after {
            let surfaces = surfaces.clone();
            let zone_only = args.zone_only;
            after(seconds, cx, move |cx| {
                let mut spec: SurfaceSpec = surfaces.borrow().spec();
                spec.exclusive_zone = ExclusiveZone::Reserve(pixels);
                let strip_edge = spec.reserving_edge();
                let vertical = strip_edge == Some(Edges::TOP) || strip_edge == Some(Edges::BOTTOM);
                if !zone_only && vertical {
                    spec.height = Extent::Logical(pixels);
                } else if !zone_only && strip_edge.is_some() {
                    spec.width = Extent::Logical(pixels);
                }
                say(format!("spec zone={pixels}"));
                if let Err(error) = surfaces.borrow().set_spec(spec, cx) {
                    say(format!("error spec {error:?}"));
                }
            });
        }
        if let Some((seconds, anchor)) = args.anchor_after {
            let surfaces = surfaces.clone();
            after(seconds, cx, move |cx| {
                let mut spec: SurfaceSpec = surfaces.borrow().spec();
                spec.anchor = anchor;
                say(format!("spec anchor={}", edges_name(anchor)));
                if let Err(error) = surfaces.borrow().set_spec(spec, cx) {
                    say(format!("error spec {error:?}"));
                }
            });
        }
        // Kept for the life of the application.
        std::mem::forget(surfaces);
    });
}

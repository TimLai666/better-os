//! The controller: one per process, an entity every surface reads. It takes
//! the bus's messages, opens and closes the interface, captures, saves,
//! records, answers the portal, and ends the process when nothing holds it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use better_core::self_update::{ProgramFile, UpdateState, UpdateTracker};
use better_ui::layer::{
    Edges, ExclusiveZone, Extent, Keyboard, Layer, LayerSurfaces, Margin, OutputInfo as LayerOutput,
    Placement, SurfaceSpec,
};
use gpui::{App, AppContext as _, Context, Entity, RenderImage, Task, WeakEntity};
use screenshot_capture::clipboard::{self, ClipboardHold};
use screenshot_capture::recorder::{self, Recording, RecordingRequest};
use screenshot_capture::wayland::WaylandCapture;
use screenshot_capture::{Capture, Snapshot, Want};
use screenshot_core::area::{AreaSelector, Grip};
use screenshot_core::compose::{Frame, Placed, compose};
use screenshot_core::files::{
    create_unique, now, picture_folder, picture_stem, recording_folder, recording_stem, save_unique,
};
use screenshot_core::geometry::{Point, Rect, stage_of};
use screenshot_core::interface::{Interface, Kind, Mode, Purpose};
use screenshot_core::layout::{arrange, layout_area};
use screenshot_core::portal::{Outcome, color_of, may_start};
use screenshot_core::recording::{Ending, elapsed_label};
use screenshot_core::session::{Action, Holds, Remembered, Request, action_for};
use screenshot_core::words::{InterfaceWords, Locale, Notice, NoticeKind, SaveWords};

use crate::platform;
use crate::service::{self, Message, Service};
use crate::view::{BarView, FlashView, OutputView, ShadeView};

pub const APP_ID: &str = "io.betteros.Screenshot";
/// The package the program ships in (ticket 203's), for the update watch.
pub const PACKAGE: &str = "better-os-session";

/// How long the process waits with nothing to hold it before it ends; a
/// D-Bus-activated portal backend waits longer, so the portal's next call
/// finds it.
pub const IDLE_EXIT: Duration = Duration::from_secs(5);
pub const PORTAL_IDLE_EXIT: Duration = Duration::from_secs(60);

/// One output of the frozen screen.
pub struct OutputPlace {
    pub name: String,
    pub logical: Rect,
    pub plain: Arc<RenderImage>,
    pub pointer: Arc<RenderImage>,
    pub frame: Frame,
    pub frame_pointer: Option<Frame>,
}

/// One window in window mode.
pub struct WindowPlace {
    pub title: String,
    pub output: usize,
    /// Where its picture is drawn, in global logical pixels.
    pub rect: Rect,
    pub image: Arc<RenderImage>,
    pub image_pointer: Arc<RenderImage>,
    pub frame: Frame,
    pub frame_pointer: Option<Frame>,
    pub activated: bool,
}

/// What the interface is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Use {
    Capture,
    PickColor,
}

/// The interface, open.
pub struct Open {
    pub using: Use,
    pub outputs: Vec<OutputPlace>,
    pub windows: Vec<WindowPlace>,
    pub interface: Interface,
    pub area: AreaSelector,
    pub stage: Rect,
    pub panel_output: usize,
    pub grip: Grip,
    pub hover_screen: Option<usize>,
    pub hover_window: Option<usize>,
    /// For colour picking: where the pointer is, globally.
    pub pointer_at: Option<Point>,
    pub surfaces: Option<LayerSurfaces<OutputView>>,
}

impl Open {
    /// Which output a global point is on.
    pub fn output_at(&self, point: Point) -> Option<usize> {
        self.outputs
            .iter()
            .position(|output| output.logical.contains(point))
    }

    /// The pixel under a global point, from the frozen pictures.
    pub fn pixel_at(&self, point: Point) -> Option<[u8; 4]> {
        let output = &self.outputs[self.output_at(point)?];
        let scale = f64::from(output.frame.width) / f64::from(output.logical.width.max(1));
        let x = ((point.x - f64::from(output.logical.x)) * scale).floor().max(0.0) as u32;
        let y = ((point.y - f64::from(output.logical.y)) * scale).floor().max(0.0) as u32;
        (x < output.frame.width && y < output.frame.height).then(|| output.frame.pixel(x, y))
    }

    /// The picture Capture takes now (spec S2, C14, P1).
    pub fn picture(&self) -> Option<Frame> {
        let pointer = self.interface.show_pointer;
        match self.interface.kind {
            Kind::Selection => {
                let placed: Vec<Placed<'_>> = self
                    .outputs
                    .iter()
                    .map(|output| Placed {
                        logical: output.logical,
                        frame: if pointer {
                            output.frame_pointer.as_ref().unwrap_or(&output.frame)
                        } else {
                            &output.frame
                        },
                    })
                    .collect();
                compose(self.area.geometry(), &placed)
            }
            Kind::Screen => self.outputs.get(self.interface.screen).map(|output| {
                if pointer {
                    output.frame_pointer.clone().unwrap_or_else(|| output.frame.clone())
                } else {
                    output.frame.clone()
                }
            }),
            Kind::Window => {
                let window = self.windows.get(self.interface.window?)?;
                Some(if pointer {
                    window.frame_pointer.clone().unwrap_or_else(|| window.frame.clone())
                } else {
                    window.frame.clone()
                })
            }
        }
    }

    /// What a recording covers (spec R1).
    pub fn recorded_area(&self) -> Option<Rect> {
        match self.interface.kind {
            Kind::Selection => Some(self.area.geometry()),
            Kind::Screen => self.outputs.get(self.interface.screen).map(|o| o.logical),
            Kind::Window => None,
        }
    }

    /// The rectangles of the screens or the windows, for the arrow keys.
    pub fn choices(&self) -> Vec<Rect> {
        match self.interface.kind {
            Kind::Screen => self.outputs.iter().map(|output| output.logical).collect(),
            Kind::Window => self.windows.iter().map(|window| window.rect).collect(),
            Kind::Selection => Vec::new(),
        }
    }
}

/// A recording in progress.
pub struct Run {
    pub recording: Option<Recording>,
    pub area: Rect,
    pub path: PathBuf,
    pub started: Instant,
    pub shade: Option<LayerSurfaces<ShadeView>>,
    pub bar: Option<LayerSurfaces<BarView>>,
}

/// A portal call waiting for the interface.
struct Pending {
    handle: String,
    reply: smol::channel::Sender<Outcome>,
}

/// The controller.
pub struct Shooter {
    pub service: Service,
    pub locale: Locale,
    pub words: InterfaceWords,
    pub remembered: Remembered,
    state_path: Option<PathBuf>,
    pub recording_supported: Result<(), String>,
    /// The desktop puts close on the left of a title bar (GNOME's
    /// `_refreshButtonLayout` 1486), so the panel's close button goes left.
    pub close_on_left: bool,
    pub open: Option<Open>,
    opening: bool,
    pub run: Option<Run>,
    clipboard: Vec<ClipboardHold>,
    saving: usize,
    portal: Option<Pending>,
    portal_started: bool,
    flash: Option<LayerSurfaces<FlashView>>,
    /// The outputs of the last interface, by name, for placing the
    /// recording's bar.
    last_outputs: Vec<(String, Rect)>,
    idle_since: Option<Instant>,
    tracker: Option<UpdateTracker>,
    weak: WeakEntity<Shooter>,
    _tasks: Vec<Task<()>>,
}

fn render_image(frame: &Frame) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.to_bgra())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// GPUI names a Wayland output by a UUID made from its name
/// (`gpui_linux` `wayland/display.rs` `uuid`); the capture knows the name.
pub fn display_key(name: &str) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, name.as_bytes()).to_string()
}

/// The interface's surface: on the overlay layer over everything, above
/// other surfaces' reserved space, with the keyboard.
pub fn interface_spec() -> SurfaceSpec {
    SurfaceSpec {
        namespace: "better-screenshot".into(),
        layer: Layer::Overlay,
        anchor: Edges::ALL,
        width: Extent::Fill,
        height: Extent::Fill,
        exclusive_zone: ExclusiveZone::IgnoreOthers,
        exclusive_edge: None,
        margin: Margin::default(),
        keyboard: Keyboard::Exclusive,
    }
}

/// The shading over the outputs while recording, and the flash: nothing
/// takes the keyboard or a click.
pub fn passive_spec(namespace: &str) -> SurfaceSpec {
    SurfaceSpec {
        keyboard: Keyboard::None,
        namespace: namespace.into(),
        ..interface_spec()
    }
}

/// The recording bar: a small strip at the top centre of the recorded
/// output, on the top layer.
pub const BAR_WIDTH: f32 = 168.0;
pub const BAR_HEIGHT: f32 = 40.0;

pub fn bar_spec() -> SurfaceSpec {
    SurfaceSpec {
        namespace: "better-screenshot-recording".into(),
        layer: Layer::Overlay,
        anchor: Edges::TOP,
        width: Extent::Logical(BAR_WIDTH),
        height: Extent::Logical(BAR_HEIGHT),
        exclusive_zone: ExclusiveZone::None,
        exclusive_edge: None,
        margin: Margin {
            top: 8.0,
            ..Margin::default()
        },
        keyboard: Keyboard::None,
    }
}

impl Shooter {
    pub fn new(
        service: Service,
        messages: smol::channel::Receiver<Message>,
        first: Request,
        cx: &mut Context<Self>,
    ) -> Shooter {
        let locale = match better_ui::fluent::words::Locale::from_environment() {
            better_ui::fluent::words::Locale::TraditionalChinese => Locale::TraditionalChinese,
            better_ui::fluent::words::Locale::English => Locale::English,
        };
        let state_path = platform::state_path();
        let remembered = platform::load_remembered(state_path.as_deref());
        let mut tasks = Vec::new();

        // The bus's messages.
        tasks.push(cx.spawn(async move |this, cx| {
            while let Ok(message) = messages.recv().await {
                if this
                    .update(cx, |this, cx| this.on_message(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        // Once a second: the recording's time, the update watch, and
        // whether anything still holds the process.
        tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        }));

        let tracker = ProgramFile::current()
            .ok()
            .map(|program| UpdateTracker::new(program, Some(PACKAGE)));
        let mut shooter = Shooter {
            service,
            locale,
            words: InterfaceWords::of(locale),
            remembered,
            state_path,
            recording_supported: recorder::supported(),
            close_on_left: close_on_left(),
            open: None,
            opening: false,
            run: None,
            clipboard: Vec::new(),
            saving: 0,
            portal: None,
            portal_started: first == Request::Portal,
            flash: None,
            last_outputs: Vec::new(),
            idle_since: None,
            tracker,
            weak: cx.weak_entity(),
            _tasks: tasks,
        };
        shooter.on_message(Message::Request(first), cx);
        shooter
    }

    /// What holds the process now.
    pub fn holds(&self) -> Holds {
        Holds {
            interface_open: self.open.is_some() || self.opening,
            recording: self.run.is_some(),
            clipboard: self.clipboard.iter().any(ClipboardHold::is_held),
            notifications: self
                .service
                .status
                .notifications
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0,
            portal_pending: self.portal.is_some(),
            saving: self.saving > 0 || self.flash.is_some(),
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        // The recording.
        if let Some(run) = &self.run {
            let seconds = run.started.elapsed().as_secs() as u32;
            let label = elapsed_label(run.started.elapsed());
            let title = self.words.recording_title.replace("{}", &label);
            let ended = run.recording.as_ref().is_some_and(Recording::has_ended);
            self.service.set_recording(true, seconds, &title);
            if let Some(bar) = &run.bar {
                for (_, window) in bar.windows() {
                    let _ = window.update(cx, |_, _, cx| cx.notify());
                }
            }
            if ended {
                self.stop_recording(cx);
            }
        }
        self.clipboard.retain(ClipboardHold::is_held);

        // An update replaced the program: a process holding nothing that a
        // restart would lose leaves, and the next key press starts the new
        // one. With the interface open, its window says it waits.
        let holds = self.holds();
        if let Some(tracker) = &mut self.tracker
            && let UpdateState::Ready { .. } = tracker.poll(Instant::now())
            && crate::restart::restart_blocked(&holds).is_none()
        {
            cx.quit();
            return;
        }

        if holds.may_exit() {
            let since = *self.idle_since.get_or_insert_with(Instant::now);
            let patience = if self.portal_started {
                PORTAL_IDLE_EXIT
            } else {
                IDLE_EXIT
            };
            if since.elapsed() >= patience {
                cx.quit();
            }
        } else {
            self.idle_since = None;
        }
    }

    pub fn on_message(&mut self, message: Message, cx: &mut Context<Self>) {
        match message {
            Message::Request(request) => {
                if request == Request::Portal {
                    self.portal_started = true;
                }
                let action = action_for(request, self.run.is_some(), self.open.is_some() || self.opening);
                match action {
                    Action::OpenInterface => self.open_interface(Use::Capture, Purpose::Picture, cx),
                    Action::OpenRecordingInterface => {
                        self.open_interface(Use::Capture, Purpose::Recording, cx)
                    }
                    Action::TakeScreens => self.take_at_once(false, None, cx),
                    Action::TakeFocusedWindow => self.take_at_once(true, None, cx),
                    Action::StopRecording => self.stop_recording(cx),
                    Action::Nothing => {}
                }
            }
            Message::StopRecording => self.stop_recording(cx),
            Message::PortalScreenshot {
                handle,
                options,
                reply,
            } => {
                if let Err(refusal) = may_start(self.portal.is_some() || self.open.is_some() || self.opening) {
                    let _ = reply.try_send(refusal);
                    return;
                }
                if options.interactive {
                    self.portal = Some(Pending { handle, reply });
                    self.open_interface(Use::Capture, Purpose::PictureOnly, cx);
                } else {
                    self.portal = Some(Pending { handle, reply });
                    self.take_at_once(false, Some(()), cx);
                }
            }
            Message::PortalPickColor { handle, reply } => {
                if let Err(refusal) = may_start(self.portal.is_some() || self.open.is_some() || self.opening) {
                    let _ = reply.try_send(refusal);
                    return;
                }
                self.portal = Some(Pending { handle, reply });
                self.open_interface(Use::PickColor, Purpose::PictureOnly, cx);
            }
            Message::PortalClose { handle } => {
                if self.portal.as_ref().is_some_and(|pending| pending.handle == handle) {
                    self.close_interface(cx);
                    self.answer_portal(Outcome::Cancelled);
                }
            }
            Message::NotificationAction { key, path } => {
                let connection = self.service.connection.clone();
                cx.background_spawn(async move {
                    let result = if key == "show-in-files" {
                        service::show_in_files(&connection, &path)
                    } else {
                        service::open_uri(&connection, &path)
                    };
                    if let Err(error) = result {
                        eprintln!("better-screenshot: could not open {}: {error}", path.display());
                    }
                })
                .detach();
            }
        }
    }

    fn answer_portal(&mut self, outcome: Outcome) {
        if let Some(pending) = self.portal.take() {
            let _ = pending.reply.try_send(outcome);
        }
    }

    /// Copies the screen and opens the interface over it (spec O1, O2).
    pub fn open_interface(&mut self, using: Use, purpose: Purpose, cx: &mut Context<Self>) {
        if self.open.is_some() || self.opening {
            return;
        }
        self.opening = true;
        let task = cx.background_spawn(async move {
            WaylandCapture.snapshot(Want {
                windows: using == Use::Capture,
                ..Want::default()
            })
        });
        cx.spawn(async move |this, cx| {
            let snapshot = task.await;
            let _ = this.update(cx, |this, cx| {
                this.opening = false;
                match snapshot {
                    Ok(snapshot) => this.show(snapshot, using, purpose, cx),
                    Err(error) => {
                        this.answer_portal(Outcome::Refused(error.to_string()));
                        this.notify(&NoticeKind::CaptureFailed(error.to_string()), None, None, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn show(&mut self, snapshot: Snapshot, using: Use, purpose: Purpose, cx: &mut Context<Self>) {
        let pointer_output = snapshot.pointer_output.unwrap_or(0);
        let outputs: Vec<OutputPlace> = snapshot
            .outputs
            .into_iter()
            .map(|(info, shot)| OutputPlace {
                name: info.name,
                logical: info.logical,
                plain: render_image(&shot.plain),
                pointer: render_image(shot.frame(true)),
                frame: shot.plain,
                frame_pointer: shot.with_pointer,
            })
            .collect();
        if outputs.is_empty() {
            self.answer_portal(Outcome::Refused("no outputs".into()));
            return;
        }
        let stage = stage_of(outputs.iter().map(|output| &output.logical));
        self.last_outputs = outputs
            .iter()
            .map(|output| (output.name.clone(), output.logical))
            .collect();
        let panel_output = pointer_output.min(outputs.len() - 1);

        // The windows, laid out per output as the overview spreads them.
        let mut windows: Vec<WindowPlace> = Vec::new();
        for (index, output) in outputs.iter().enumerate() {
            let on_here: Vec<_> = snapshot
                .windows
                .iter()
                .filter(|(info, _)| {
                    info.output.as_deref() == Some(output.name.as_str())
                        || (info.output.is_none() && index == 0)
                })
                .collect();
            if on_here.is_empty() {
                continue;
            }
            let scale = f64::from(output.frame.width) / f64::from(output.logical.width.max(1));
            let sizes: Vec<(i32, i32)> = on_here
                .iter()
                .map(|(_, shot)| {
                    (
                        (f64::from(shot.plain.width) / scale).round() as i32,
                        (f64::from(shot.plain.height) / scale).round() as i32,
                    )
                })
                .collect();
            let rects = arrange(&sizes, layout_area(output.logical, index == panel_output));
            for ((info, shot), rect) in on_here.into_iter().zip(rects) {
                windows.push(WindowPlace {
                    title: info.title.clone(),
                    output: index,
                    rect,
                    image: render_image(&shot.plain),
                    image_pointer: render_image(shot.frame(true)),
                    frame: shot.plain.clone(),
                    frame_pointer: shot.with_pointer.clone(),
                    activated: info.activated,
                });
            }
        }
        let focused = windows.iter().position(|window| window.activated).or(
            (!windows.is_empty()).then_some(0),
        );

        let mut area = match self.remembered.area {
            Some(rect) => AreaSelector::remembered(rect, 1.0),
            None => AreaSelector::new(1.0),
        };
        area.reset(stage, outputs.get(panel_output).map(|output| output.logical));
        let interface = Interface::open(
            self.remembered.kept,
            purpose,
            self.recording_supported.is_ok(),
            !windows.is_empty(),
            panel_output,
            focused,
        );
        self.open = Some(Open {
            using,
            outputs,
            windows,
            interface,
            area,
            stage: stage.unwrap_or_default(),
            panel_output,
            grip: Grip::Crosshair,
            hover_screen: None,
            hover_window: None,
            pointer_at: None,
            surfaces: None,
        });

        let shooter = cx.entity();
        let keys: Vec<String> = self
            .open
            .as_ref()
            .map(|open| open.outputs.iter().map(|output| display_key(&output.name)).collect())
            .unwrap_or_default();
        // A surface draws its first frame while it opens, and that frame
        // reads this controller, so the surfaces open after this update.
        cx.spawn(async move |this, cx| {
            let opened = cx.update(|cx| {
                LayerSurfaces::open(
                    APP_ID,
                    interface_spec(),
                    Placement::EveryOutput,
                    move |output: &LayerOutput, window, cx| {
                        let index = output
                            .key
                            .as_ref()
                            .and_then(|key| keys.iter().position(|candidate| candidate == key))
                            .unwrap_or(0);
                        OutputView::new(shooter.clone(), index, index == panel_output, window, cx)
                    },
                    cx,
                )
            });
            let _ = this.update(cx, |this, cx| {
                match opened {
                    Ok(surfaces) => {
                        if let Some(error) = surfaces.last_error() {
                            eprintln!("better-screenshot: {error}");
                        }
                        match &mut this.open {
                            Some(open) => open.surfaces = Some(surfaces),
                            // Closed before its surfaces were up.
                            None => surfaces.close_all(cx),
                        }
                    }
                    Err(error) => {
                        eprintln!("better-screenshot: {error:?}");
                        this.open = None;
                        this.answer_portal(Outcome::Refused("the interface could not open".into()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Closes the interface without capturing (spec O4); the portal hears
    /// "cancelled".
    pub fn close_interface(&mut self, cx: &mut Context<Self>) {
        if let Some(mut open) = self.open.take() {
            self.remember(&open);
            if let Some(surfaces) = open.surfaces.take() {
                surfaces.close_all(cx);
            }
        }
        if self.portal.is_some() {
            self.answer_portal(Outcome::Cancelled);
        }
        cx.notify();
    }

    fn remember(&mut self, open: &Open) {
        self.remembered.area = Some(open.area.geometry());
        self.remembered.kept = open.interface.kept();
        platform::save_remembered(self.state_path.as_deref(), &self.remembered);
    }

    /// Capture: a picture, a recording, or a picked colour.
    pub fn capture(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else { return };
        if open.using == Use::PickColor {
            return;
        }
        match open.interface.mode {
            Mode::Picture => {
                let picture = open.picture();
                let mut open = self.open.take().expect("checked above");
                self.remember(&open);
                if let Some(surfaces) = open.surfaces.take() {
                    surfaces.close_all(cx);
                }
                match picture {
                    Some(frame) => self.store(frame, cx),
                    None => self.answer_portal(Outcome::Refused("nothing chosen".into())),
                }
            }
            Mode::Recording => self.start_recording(cx),
        }
        cx.notify();
    }

    /// The pixel under the pointer, for the portal's `PickColor` (spec A3).
    pub fn pick(&mut self, point: Point, cx: &mut Context<Self>) {
        let Some(open) = &self.open else { return };
        let pixel = open.pixel_at(point);
        let mut open = self.open.take().expect("checked above");
        if let Some(surfaces) = open.surfaces.take() {
            surfaces.close_all(cx);
        }
        let outcome = match pixel {
            Some(pixel) => {
                let (r, g, b) = color_of(pixel);
                Outcome::Color(r, g, b)
            }
            None => Outcome::Cancelled,
        };
        self.answer_portal(outcome);
        cx.notify();
    }

    /// Shift+Print, Alt+Print, and the portal's non-interactive call: no
    /// interface (spec S8, A2).
    fn take_at_once(&mut self, window: bool, portal: Option<()>, cx: &mut Context<Self>) {
        self.saving += 1;
        let task = cx.background_spawn(async move {
            WaylandCapture.snapshot(Want {
                focused_window_only: window,
                ..Want::default()
            })
        });
        cx.spawn(async move |this, cx| {
            let snapshot = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving -= 1;
                let snapshot = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        this.answer_portal(Outcome::Refused(error.to_string()));
                        this.notify(&NoticeKind::CaptureFailed(error.to_string()), None, None, cx);
                        return;
                    }
                };
                let frame = if window {
                    snapshot.windows.first().map(|(_, shot)| shot.plain.clone())
                } else {
                    let placed: Vec<Placed<'_>> = snapshot
                        .outputs
                        .iter()
                        .map(|(info, shot)| Placed {
                            logical: info.logical,
                            frame: &shot.plain,
                        })
                        .collect();
                    stage_of(snapshot.outputs.iter().map(|(info, _)| &info.logical))
                        .and_then(|stage| compose(stage, &placed))
                };
                if portal.is_some() {
                    this.flash(snapshot.outputs.iter().map(|(info, _)| info.name.clone()).collect(), cx);
                }
                match frame {
                    Some(frame) => this.store(frame, cx),
                    None => this.answer_portal(Outcome::Refused("no picture".into())),
                }
            });
        })
        .detach();
    }

    /// The white flash over every output that fades in 500 ms (spec A2).
    fn flash(&mut self, _names: Vec<String>, cx: &mut Context<Self>) {
        let shooter = cx.entity();
        cx.spawn(async move |this, cx| {
            let opened = cx.update(|cx| {
                LayerSurfaces::open(
                    APP_ID,
                    passive_spec("better-screenshot-flash"),
                    Placement::EveryOutput,
                    move |_, window, cx| FlashView::new(shooter.clone(), window, cx),
                    cx,
                )
            });
            let _ = this.update(cx, |this, _| match opened {
                Ok(surfaces) => this.flash = Some(surfaces),
                Err(error) => eprintln!("better-screenshot: no flash: {error:?}"),
            });
        })
        .detach();
    }

    pub fn flash_done(&mut self, cx: &mut Context<Self>) {
        if let Some(flash) = self.flash.take() {
            flash.close_all(cx);
        }
    }

    /// The picture to the clipboard, the Pictures folder, the recently used
    /// files, and a notification (spec S1–S7); the portal gets its URI.
    fn store(&mut self, frame: Frame, cx: &mut Context<Self>) {
        platform::play_shutter();
        self.saving += 1;
        let locale = self.locale;
        let task = cx.background_spawn(async move {
            let png = screenshot_capture::png::encode(&frame).map_err(|error| error.to_string())?;
            let hold = clipboard::hold_png(png.clone()).map_err(|error| error.to_string());
            let saved = platform::user_folder(files_platform::UserDirectory::Pictures)
                .ok_or_else(|| "there is no home folder".to_string())
                .and_then(|pictures| {
                    let words = SaveWords::of(locale);
                    let folder = picture_folder(&pictures, &words);
                    save_unique(&folder, &picture_stem(&now(), &words), "png", &png)
                        .map_err(|error| error.to_string())
                });
            if let Ok(path) = &saved {
                platform::add_recent(path, "image/png");
            }
            Ok::<_, String>((hold, saved))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving -= 1;
                match result {
                    Ok((hold, saved)) => {
                        match hold {
                            Ok(hold) => this.clipboard.push(hold),
                            Err(error) => eprintln!("better-screenshot: clipboard: {error}"),
                        }
                        match saved {
                            Ok(path) => {
                                this.answer_portal(Outcome::Picture(better_core::uri::file_uri(&path)));
                                this.notify(&NoticeKind::PictureSaved, Some(&path), Some(&path), cx);
                            }
                            Err(error) => {
                                this.answer_portal(Outcome::Refused(error.clone()));
                                this.notify(&NoticeKind::PictureNotSaved(error), None, None, cx);
                            }
                        }
                    }
                    Err(error) => {
                        this.answer_portal(Outcome::Refused(error.clone()));
                        this.notify(&NoticeKind::CaptureFailed(error), None, None, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn notify(&self, kind: &NoticeKind, file: Option<&Path>, image: Option<&Path>, _cx: &mut Context<Self>) {
        let notice = Notice::of(kind, self.locale);
        eprintln!("better-screenshot: {} — {}", notice.title, notice.body);
        self.service.notify(&notice, file, image);
    }

    /// Starts recording what the interface chose (spec R1, R2).
    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else { return };
        let Some(area) = open.recorded_area() else { return };
        let pointer = open.interface.show_pointer;
        // Closed at once, so no fade is recorded.
        let mut open = self.open.take().expect("checked above");
        self.remember(&open);
        if let Some(surfaces) = open.surfaces.take() {
            surfaces.close_all(cx);
        }
        let words = SaveWords::of(self.locale);
        let created = platform::user_folder(files_platform::UserDirectory::Videos)
            .ok_or_else(|| "there is no home folder".to_string())
            .and_then(|videos| {
                let folder = recording_folder(&videos, &words);
                create_unique(&folder, &recording_stem(&now(), &words), "webm")
                    .map(|(_, path)| path)
                    .map_err(|error| error.to_string())
            });
        let path = match created {
            Ok(path) => path,
            Err(error) => {
                self.notify(&NoticeKind::RecordingFailedToStart(error), None, None, cx);
                return;
            }
        };
        self.run = Some(Run {
            recording: None,
            area,
            path: path.clone(),
            started: Instant::now(),
            shade: None,
            bar: None,
        });
        let request = RecordingRequest {
            area,
            pointer,
            path: path.clone(),
        };
        let task = cx.background_spawn(async move { recorder::start(request) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            let _ = this.update(cx, |this, cx| match started {
                Ok(recording) => {
                    if let Some(run) = &mut this.run {
                        run.recording = Some(recording);
                        run.started = Instant::now();
                    }
                    this.show_recording(cx);
                }
                Err(ending) => {
                    this.run = None;
                    let _ = std::fs::remove_file(&path);
                    this.notify(&ending.notice(), None, None, cx);
                    this.service.set_recording(false, 0, "");
                }
            });
        })
        .detach();
    }

    /// The shading outside the recorded area and the bar (spec R3, R4).
    fn show_recording(&mut self, cx: &mut Context<Self>) {
        let shooter = cx.entity();
        // The bar goes on the output that holds the area's centre.
        let preferred = self.open_outputs_key_for_run();
        cx.spawn(async move |this, cx| {
            let (shade, bar) = cx.update(|cx| {
                let shade = LayerSurfaces::open(
                    APP_ID,
                    passive_spec("better-screenshot-shade"),
                    Placement::EveryOutput,
                    {
                        let shooter = shooter.clone();
                        move |output: &LayerOutput, window, cx| {
                            ShadeView::new(shooter.clone(), output.key.clone(), window, cx)
                        }
                    },
                    cx,
                )
                .map_err(|error| eprintln!("better-screenshot: no shading: {error:?}"))
                .ok();
                let bar = LayerSurfaces::open(
                    APP_ID,
                    bar_spec(),
                    Placement::One { preferred },
                    move |_, window, cx| BarView::new(shooter.clone(), window, cx),
                    cx,
                )
                .map_err(|error| eprintln!("better-screenshot: no recording bar: {error:?}"))
                .ok();
                (shade, bar)
            });
            let _ = this.update(cx, |this, cx| match &mut this.run {
                Some(run) => {
                    run.shade = shade;
                    run.bar = bar;
                }
                // Stopped before its surfaces were up.
                None => {
                    if let Some(shade) = shade {
                        shade.close_all(cx);
                    }
                    if let Some(bar) = bar {
                        bar.close_all(cx);
                    }
                }
            });
        })
        .detach();
        let title = self.words.recording_title.replace("{}", "0:00");
        self.service.set_recording(true, 0, &title);
        cx.notify();
    }

    /// The GPUI key of the output the recording's area is centred on.
    fn open_outputs_key_for_run(&self) -> Option<String> {
        let run = self.run.as_ref()?;
        let centre = run.area.center();
        self.last_outputs
            .iter()
            .find(|(_, rect)| rect.contains(centre))
            .map(|(name, _)| display_key(name))
    }

    /// Stops the recording, finishes the file, and says so (spec R6–R8).
    pub fn stop_recording(&mut self, cx: &mut Context<Self>) {
        let Some(mut run) = self.run.take() else { return };
        if let Some(shade) = run.shade.take() {
            shade.close_all(cx);
        }
        if let Some(bar) = run.bar.take() {
            bar.close_all(cx);
        }
        self.service.set_recording(false, 0, "");
        let Some(recording) = run.recording.take() else {
            return;
        };
        self.saving += 1;
        let path = run.path.clone();
        let task = cx.background_spawn(async move { recording.stop() });
        cx.spawn(async move |this, cx| {
            let ending = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving -= 1;
                if ending.keeps_file() {
                    platform::add_recent(&path, "video/webm");
                    this.notify(&ending.notice(), Some(&path), None, cx);
                } else {
                    let _ = std::fs::remove_file(&path);
                    this.notify(&ending.notice(), None, None, cx);
                }
                if matches!(ending, Ending::Stopped) {
                    eprintln!("better-screenshot: recorded {}", path.display());
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// The logical rectangle of the output GPUI names by `key`.
    pub fn output_rect(&self, key: Option<&str>) -> Option<Rect> {
        let key = key?;
        self.last_outputs
            .iter()
            .find(|(name, _)| display_key(name) == key)
            .map(|(_, rect)| *rect)
    }

    pub fn weak(&self) -> WeakEntity<Shooter> {
        self.weak.clone()
    }
}

fn close_on_left() -> bool {
    use better_ui::system_frame::{DesktopSettings, GSettingsListing, WindowButton};
    DesktopSettings::read(&GSettingsListing::read())
        .button_layout
        .start
        .contains(&WindowButton::Close)
}

/// Starts the controller in a running GPUI application.
pub fn start(
    service: Service,
    messages: smol::channel::Receiver<Message>,
    first: Request,
    cx: &mut App,
) -> Entity<Shooter> {
    cx.new(|cx| Shooter::new(service, messages, first, cx))
}

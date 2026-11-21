//! The controller: one per process, an entity every surface reads. It takes
//! the bus's messages, opens and closes the interface, captures, saves,
//! records, answers the portal, and ends the process when nothing holds it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use better_core::self_update::{ProgramFile, UpdateState, UpdateTracker};
use better_ui::layer::{
    Edges, ExclusiveZone, Extent, Keyboard, Layer, LayerSurfaces, Margin,
    OutputInfo as LayerOutput, Placement, SurfaceSpec,
};
use gpui::{App, AppContext as _, Context, Entity, RenderImage, Task};
use screenshot_capture::clipboard::{self, ClipboardHold};
use screenshot_capture::recorder::{self, Recording, RecordingRequest};
use screenshot_capture::wayland::WaylandCapture;
use screenshot_capture::{Capture, Snapshot, Want};
use screenshot_core::area::{AreaSelector, Grip};
use screenshot_core::compose::{Frame, Placed, compose};
use screenshot_core::files::{
    PORTAL_STEM, create_unique, now, picture_folder, picture_stem, portal_folder, recording_folder,
    recording_stem, save_unique,
};
use screenshot_core::geometry::{Point, Rect, stage_of};
use screenshot_core::interface::{Interface, Kind, Mode, Purpose};
use screenshot_core::layout::{arrange, layout_area};
use screenshot_core::lockdown::{Allowed, Asked, Saving, allowed};
use screenshot_core::portal::{Answer, Origin, Outcome, RequestKind, Requests, color_of};
use screenshot_core::recording::{Phase, Started, Stop, elapsed_label};
use screenshot_core::session::{Action, Holds, Lock, Remembered, Request, Shown, action_for};
use screenshot_core::words::{InterfaceWords, Locale, Notice, NoticeKind, SaveWords};

use crate::platform;
use crate::service::{self, Message, NoticeImage, Service};
use crate::view::{BarView, FlashView, OutputView, ShadeView};

pub const APP_ID: &str = "io.betteros.Screenshot";
/// The package the program ships in (ticket 203's), for the update watch.
pub const PACKAGE: &str = "better-os-session";

/// How long the process waits with nothing to hold it before it ends; a
/// D-Bus-activated portal backend waits longer, so the portal's next call
/// finds it.
pub const IDLE_EXIT: Duration = Duration::from_secs(5);
/// GNOME's fade in and out of the interface (`ease({opacity, duration:
/// 200, mode: EASE_OUT_QUAD})`, `js/ui/screenshot.js` `open` 1665–1668 and
/// `close` 1710–1715).
pub const FADE: Duration = Duration::from_millis(200);

/// Clutter's `EASE_OUT_QUAD`.
pub fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}
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
    /// Who it was opened for: only that portal request is answered by it.
    pub origin: Origin,
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
    /// The interface is fading out (spec O5); it takes no input.
    pub closing: bool,
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
        let x = ((point.x - f64::from(output.logical.x)) * scale)
            .floor()
            .max(0.0) as u32;
        let y = ((point.y - f64::from(output.logical.y)) * scale)
            .floor()
            .max(0.0) as u32;
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
                    output
                        .frame_pointer
                        .clone()
                        .unwrap_or_else(|| output.frame.clone())
                } else {
                    output.frame.clone()
                }
            }),
            Kind::Window => {
                let window = self.windows.get(self.interface.window?)?;
                Some(if pointer {
                    window
                        .frame_pointer
                        .clone()
                        .unwrap_or_else(|| window.frame.clone())
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
    pub phase: Phase,
    pub recording: Option<Recording>,
    pub area: Rect,
    pub path: PathBuf,
    pub started: Instant,
    pub shade: Option<LayerSurfaces<ShadeView>>,
    pub bar: Option<LayerSurfaces<BarView>>,
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
    /// The portal request in flight and who the interface answers for.
    portal: Requests<smol::channel::Sender<Outcome>>,
    portal_started: bool,
    /// The session's lock, from logind.
    pub lock: Lock,
    /// Counts fades: a fade's end closes the interface only if no reopening
    /// came after it began.
    pub fade_generation: u64,
    flash: Option<LayerSurfaces<FlashView>>,
    /// The outputs of the last interface, by name, for placing the
    /// recording's bar.
    last_outputs: Vec<(String, Rect)>,
    idle_since: Option<Instant>,
    tracker: Option<UpdateTracker>,
    _tasks: Vec<Task<()>>,
}

fn render_image(frame: &Frame) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.to_bgra())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

/// A window's size in logical pixels: its width as the compositor says,
/// with its picture's shape; without one, its picture at the output's scale.
pub fn window_size(logical_width: Option<i32>, picture: &Frame, output_scale: f64) -> (i32, i32) {
    let width = picture.width.max(1);
    match logical_width.filter(|width| *width > 0) {
        Some(logical) => (
            logical,
            (f64::from(picture.height) * f64::from(logical) / f64::from(width)).round() as i32,
        ),
        None => (
            (f64::from(picture.width) / output_scale).round() as i32,
            (f64::from(picture.height) / output_scale).round() as i32,
        ),
    }
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

        let locked = service::watch_session_lock(service.sender());
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
            portal: Requests::default(),
            portal_started: first == Request::Portal,
            lock: Lock { locked },
            fade_generation: 0,
            flash: None,
            last_outputs: Vec::new(),
            idle_since: None,
            tracker,
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
            portal_pending: self.portal.is_pending(),
            saving: self.saving > 0 || self.flash.is_some(),
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        // The recording.
        if let Some(run) = self.run.as_ref().filter(|run| run.phase == Phase::Running) {
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

        // An output came or went while the interface is open: it closes, as
        // GNOME's does ("Nope, not dealing with monitor changes", 1394).
        if let Some(open) = &self.open
            && open.surfaces.is_some()
        {
            let mut now: Vec<String> = cx
                .displays()
                .iter()
                .filter_map(|display| display.uuid().ok())
                .map(|uuid| uuid.to_string())
                .collect();
            let mut then: Vec<String> = open
                .outputs
                .iter()
                .map(|output| display_key(&output.name))
                .collect();
            now.sort();
            then.sort();
            // GPUI at the pinned revision never drops a removed output from
            // its list (better-os-session.md, "Surfaces on layer-shell"), but
            // the compositor closes that output's surface.
            let lost = open
                .surfaces
                .as_ref()
                .is_some_and(|surfaces| surfaces.windows().len() < open.outputs.len());
            if now != then || lost {
                self.close_instantly(cx);
            }
        }

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

    /// Whether the interface is shown, for [`action_for`].
    fn shown(&self) -> Shown {
        match &self.open {
            Some(open) if open.closing => Shown::Closing,
            Some(_) => Shown::Open,
            None if self.opening => Shown::Open,
            None => Shown::Closed,
        }
    }

    pub fn on_message(&mut self, message: Message, cx: &mut Context<Self>) {
        match message {
            Message::Request(request) => {
                if request == Request::Portal {
                    self.portal_started = true;
                }
                let action = action_for(request, self.run.is_some(), self.shown(), self.lock);
                match action {
                    Action::OpenInterface => {
                        self.open_interface(Use::Capture, Purpose::Picture, Origin::Person, cx)
                    }
                    Action::OpenRecordingInterface => {
                        self.open_interface(Use::Capture, Purpose::Recording, Origin::Person, cx)
                    }
                    Action::Reopen => self.reopen(Purpose::Picture, cx),
                    Action::ReopenRecording => self.reopen(Purpose::Recording, cx),
                    Action::TakeScreens => self.take_at_once(false, Origin::Person, cx),
                    Action::TakeFocusedWindow => self.take_at_once(true, Origin::Person, cx),
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
                let kind = if options.interactive {
                    RequestKind::Interface
                } else {
                    RequestKind::Instant
                };
                // The instant picture opens nothing, so only another request
                // keeps it waiting; the interface needs the screen free.
                let busy = kind == RequestKind::Interface && self.shown() != Shown::Closed;
                let origin = match self
                    .portal
                    .begin(handle, kind, reply, busy, self.lock.locked)
                {
                    Ok(origin) => origin,
                    Err(answer) => return send(Some(answer)),
                };
                match kind {
                    RequestKind::Interface => {
                        self.open_interface(Use::Capture, Purpose::PictureOnly, origin, cx)
                    }
                    _ => self.take_at_once(false, origin, cx),
                }
            }
            Message::PortalPickColor { handle, reply } => {
                let busy = self.shown() != Shown::Closed;
                let origin = match self.portal.begin(
                    handle,
                    RequestKind::PickColor,
                    reply,
                    busy,
                    self.lock.locked,
                ) {
                    Ok(origin) => origin,
                    Err(answer) => return send(Some(answer)),
                };
                self.open_interface(Use::PickColor, Purpose::PictureOnly, origin, cx);
            }
            Message::PortalClose { handle } => {
                // `Request.Close`, or the caller left the bus: the request
                // ends as cancelled, its interface closes, and a capture still
                // running for it is dropped when it finishes.
                let origin = Origin::Portal(handle.clone());
                send(self.portal.close(&handle));
                if self.open.as_ref().is_some_and(|open| open.origin == origin) {
                    self.fade_out(cx);
                }
            }
            Message::SessionLocking => self.close_instantly(cx),
            Message::LockedHint(locked) => {
                self.lock.locked = locked;
                if locked {
                    self.close_instantly(cx);
                }
            }
            Message::NotificationAction { key, path } => {
                let connection = self.service.connection.clone();
                let locale = self.locale;
                cx.background_spawn(async move {
                    let result = if key == "show-in-files" {
                        service::show_in_files(&connection, &path)
                    } else {
                        crate::open::open_file(
                            &connection,
                            &path,
                            crate::open::mime_of(&path),
                            locale,
                        )
                    };
                    if let Err(error) = result {
                        eprintln!(
                            "better-screenshot: could not open {}: {error}",
                            path.display()
                        );
                    }
                })
                .detach();
            }
        }
    }

    /// Answers the portal request `origin` belongs to, if it is still
    /// pending; the person's own captures answer nothing.
    fn answer(&mut self, origin: &Origin, outcome: Outcome) {
        send(self.portal.answer(origin, outcome));
    }

    /// The interface ended without handing a picture over: its request, if
    /// still pending, hears "cancelled" (`Requests::interface_ended`).
    fn interface_ended(&mut self) {
        send(self.portal.interface_ended());
    }

    /// Print, or the recording key, while the interface fades out: it comes
    /// back, as GNOME's `open` does from "in process of closing" (1579). An
    /// interface that was a portal's is not the person's to reopen: it
    /// finishes closing and the person's opens over the screen as it is now.
    fn reopen(&mut self, purpose: Purpose, cx: &mut Context<Self>) {
        let theirs = self
            .open
            .as_ref()
            .is_some_and(|open| open.origin == Origin::Person && open.using == Use::Capture);
        if !theirs {
            self.close_instantly(cx);
            self.open_interface(Use::Capture, purpose, Origin::Person, cx);
            return;
        }
        self.fade_generation += 1;
        self.portal.interface_opening(Origin::Person);
        if let Some(open) = &mut self.open {
            open.closing = false;
            if purpose == Purpose::Recording {
                open.interface.set_mode(Mode::Recording);
            }
        }
        cx.notify();
    }

    /// Copies the screen and opens the interface over it (spec O1, O2).
    pub fn open_interface(
        &mut self,
        using: Use,
        purpose: Purpose,
        origin: Origin,
        cx: &mut Context<Self>,
    ) {
        if self.open.is_some() || self.opening {
            return;
        }
        self.opening = true;
        self.portal.interface_opening(origin.clone());
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
                // Ended, closed, or locked meanwhile: nothing opens, and a
                // request still waiting hears "cancelled".
                if !this.portal.may_show(&origin, this.lock.locked) {
                    if this.portal.interface_owner() == Some(&origin) {
                        this.interface_ended();
                    }
                    return;
                }
                match snapshot {
                    Ok(snapshot) => this.show(snapshot, using, purpose, origin, cx),
                    Err(error) => {
                        let person = origin == Origin::Person;
                        this.answer(&origin, Outcome::Refused(error.to_string()));
                        this.interface_ended();
                        if person {
                            this.notify(
                                &NoticeKind::CaptureFailed(error.to_string()),
                                None,
                                NoticeImage::None,
                                cx,
                            );
                        }
                    }
                }
            });
        })
        .detach();
    }

    fn show(
        &mut self,
        snapshot: Snapshot,
        using: Use,
        purpose: Purpose,
        origin: Origin,
        cx: &mut Context<Self>,
    ) {
        for skipped in &snapshot.skipped {
            eprintln!("better-screenshot: an output is left out: {skipped}");
        }
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
            self.answer(&origin, Outcome::Refused("no outputs".into()));
            self.interface_ended();
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
                .map(|(info, shot)| window_size(info.logical_width, &shot.plain, scale))
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
        let focused = windows
            .iter()
            .position(|window| window.activated)
            .or((!windows.is_empty()).then_some(0));

        let mut area = match self.remembered.area {
            Some(rect) => AreaSelector::remembered(rect, 1.0),
            None => AreaSelector::new(1.0),
        };
        area.reset(
            stage,
            outputs.get(panel_output).map(|output| output.logical),
        );
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
            origin,
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
            closing: false,
        });

        let shooter = cx.entity();
        let keys: Vec<String> = self
            .open
            .as_ref()
            .map(|open| {
                open.outputs
                    .iter()
                    .map(|output| display_key(&output.name))
                    .collect()
            })
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
                        if let Some(open) = this.open.take() {
                            this.answer(
                                &open.origin,
                                Outcome::Refused("the interface could not open".into()),
                            );
                            this.interface_ended();
                        }
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
        self.interface_ended();
        self.fade_out(cx);
        cx.notify();
    }

    /// Closes without the fade, as GNOME's `close(true)` does when the
    /// outputs change or a recording starts.
    /// An interface still opening is ended too: its screen copy, when it
    /// comes back, opens nothing. A picture already handed to be stored is
    /// answered by the store, not here.
    pub fn close_instantly(&mut self, cx: &mut Context<Self>) {
        if let Some(mut open) = self.open.take() {
            self.remember(&open);
            if let Some(surfaces) = open.surfaces.take() {
                surfaces.close_all(cx);
            }
        }
        self.interface_ended();
        cx.notify();
    }

    /// Fades the interface out over [`FADE`] and then closes its surfaces
    /// (spec O5). It takes no input meanwhile.
    fn fade_out(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else { return };
        if open.closing {
            return;
        }
        open.closing = true;
        self.fade_generation += 1;
        let generation = self.fade_generation;
        let area = open.area.geometry();
        let kept = open.interface.kept();
        self.remembered.area = Some(area);
        self.remembered.kept = kept;
        platform::save_remembered(self.state_path.as_deref(), &self.remembered);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FADE).await;
            let _ = this.update(cx, |this, cx| {
                // Reopened meanwhile (Print during the fade): it stays.
                if this.fade_generation != generation
                    || !this.open.as_ref().is_some_and(|open| open.closing)
                {
                    return;
                }
                if let Some(mut open) = this.open.take()
                    && let Some(surfaces) = open.surfaces.take()
                {
                    surfaces.close_all(cx);
                }
                this.interface_ended();
                cx.notify();
            });
        })
        .detach();
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
                // The picture is the frozen one, taken now; the interface
                // fades out after it (`_onCaptureButtonClicked` 1879–1882).
                let picture = open.picture();
                // Handed over: from here the store answers the request, and
                // a close during the fade out no longer says "cancelled".
                let origin = self.portal.handed_over();
                self.fade_out(cx);
                match picture {
                    Some(frame) => self.store(frame, origin, cx),
                    None => self.answer(&origin, Outcome::Refused("nothing chosen".into())),
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
        self.answer(&open.origin, outcome);
        self.interface_ended();
        cx.notify();
    }

    /// Shift+Print, Alt+Print, and the portal's non-interactive call: no
    /// interface (spec S8, A2).
    fn take_at_once(&mut self, window: bool, origin: Origin, cx: &mut Context<Self>) {
        let portal = origin != Origin::Person;
        // The portal's instant picture is a file and nothing else, so it is
        // refused outright while saving is disabled (or not known to be
        // allowed); the keys still copy (spec S6, A5).
        if portal {
            let saving = platform::saving();
            if allowed(Asked::PortalInstantPicture, saving.disabled()) == Allowed::Refused {
                let reason = self.saving_refusal(&saving);
                self.answer(&origin, Outcome::Refused(reason));
                return;
            }
        }
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
                // Closed while it was taken (`Request.Close`, the caller
                // gone): dropped, nothing written.
                if !this.portal.wants(&origin) {
                    return;
                }
                let snapshot = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        this.answer(&origin, Outcome::Refused(error.to_string()));
                        if !portal {
                            this.notify(
                                &NoticeKind::CaptureFailed(error.to_string()),
                                None,
                                NoticeImage::None,
                                cx,
                            );
                        }
                        return;
                    }
                };
                for skipped in &snapshot.skipped {
                    eprintln!("better-screenshot: an output is left out: {skipped}");
                }
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
                if portal {
                    this.flash(
                        snapshot
                            .outputs
                            .iter()
                            .map(|(info, _)| info.name.clone())
                            .collect(),
                        cx,
                    );
                }
                match (frame, portal) {
                    (Some(frame), false) => this.store(frame, origin, cx),
                    (Some(frame), true) => this.store_for_portal(frame, origin, cx),
                    (None, _) => this.answer(&origin, Outcome::Refused("no picture".into())),
                }
            });
        })
        .detach();
    }

    /// The portal's instant picture: GNOME's portal backend asks the shell
    /// for the file `Screenshot.png` (or `Screenshot-N.png`) in Pictures, or
    /// the home folder, and that is all: no clipboard, no notification, no
    /// recently used file (spec A2; `xdg-desktop-portal-gnome` 46
    /// `screenshotdialog.c` 249–252, `js/ui/screenshot.js` 2497–2515).
    fn store_for_portal(&mut self, frame: Frame, origin: Origin, cx: &mut Context<Self>) {
        platform::play_shutter();
        self.saving += 1;
        let task = cx.background_spawn(async move {
            let png = screenshot_capture::png::encode(&frame).map_err(|error| error.to_string())?;
            let home = platform::user_folder(files_platform::UserDirectory::Home)
                .ok_or_else(|| "there is no home folder".to_string())?;
            let pictures = platform::user_folder(files_platform::UserDirectory::Pictures);
            let folder = portal_folder(pictures.as_deref(), &home);
            save_unique(&folder, PORTAL_STEM, "png", &png).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |this, _| {
                this.saving -= 1;
                match saved {
                    Ok(path) => {
                        // Closed meanwhile: the file is not the caller's.
                        if this.portal.wants(&origin) {
                            this.answer(
                                &origin,
                                Outcome::Picture(better_core::uri::file_uri(&path)),
                            );
                        } else {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                    Err(error) => this.answer(&origin, Outcome::Refused(error)),
                }
            });
        })
        .detach();
    }

    /// The sentence that says why nothing is saved.
    fn saving_refusal(&self, saving: &Saving) -> String {
        match saving {
            Saving::Unknown(reason) => self.words.saving_unknown.replace("{}", reason),
            _ => self.words.saving_disabled.to_string(),
        }
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
    fn store(&mut self, frame: Frame, origin: Origin, cx: &mut Context<Self>) {
        platform::play_shutter();
        self.saving += 1;
        let locale = self.locale;
        // Saving to disk disabled, or not known to be allowed: the clipboard
        // and the notification only, with the picture in it (spec S6).
        let saving = platform::saving();
        let save = allowed(Asked::Picture, saving.disabled()) == Allowed::Everything;
        let reason = match &saving {
            Saving::Unknown(_) => Some(self.saving_refusal(&saving)),
            _ => None,
        };
        let refusal = self.saving_refusal(&saving);
        let thumbnail = (!save).then(|| frame.thumbnail(256));
        let task = cx.background_spawn(async move {
            let png = screenshot_capture::png::encode(&frame).map_err(|error| error.to_string())?;
            let hold = clipboard::hold_png(png.clone()).map_err(|error| error.to_string());
            if !save {
                return Ok::<_, String>((hold, None));
            }
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
            Ok::<_, String>((hold, Some(saved)))
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
                            None => {
                                this.answer(&origin, Outcome::Refused(refusal));
                                let image = match &thumbnail {
                                    Some(frame) => NoticeImage::Pixels(frame),
                                    None => NoticeImage::None,
                                };
                                this.notify(
                                    &NoticeKind::PictureCopiedOnly(reason),
                                    None,
                                    image,
                                    cx,
                                );
                            }
                            Some(Ok(path)) => {
                                this.answer(
                                    &origin,
                                    Outcome::Picture(better_core::uri::file_uri(&path)),
                                );
                                this.notify(
                                    &NoticeKind::PictureSaved,
                                    Some(&path),
                                    NoticeImage::Path(&path),
                                    cx,
                                );
                            }
                            Some(Err(error)) => {
                                this.answer(&origin, Outcome::Refused(error.clone()));
                                this.notify(
                                    &NoticeKind::PictureNotSaved(error),
                                    None,
                                    NoticeImage::None,
                                    cx,
                                );
                            }
                        }
                    }
                    Err(error) => {
                        this.answer(&origin, Outcome::Refused(error.clone()));
                        this.notify(
                            &NoticeKind::CaptureFailed(error),
                            None,
                            NoticeImage::None,
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }

    fn notify(
        &self,
        kind: &NoticeKind,
        file: Option<&Path>,
        image: NoticeImage<'_>,
        _cx: &mut Context<Self>,
    ) {
        let notice = Notice::of(kind, self.locale);
        eprintln!("better-screenshot: {} — {}", notice.title, notice.body);
        self.service.notify(&notice, file, image);
    }

    /// Starts recording what the interface chose (spec R1, R2).
    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else { return };
        let Some(area) = open.recorded_area() else {
            return;
        };
        let pointer = open.interface.show_pointer;
        // Closed at once, so no fade is recorded.
        let mut open = self.open.take().expect("checked above");
        self.remember(&open);
        if let Some(surfaces) = open.surfaces.take() {
            surfaces.close_all(cx);
        }
        // Only the person's interface records; it answers nobody.
        self.interface_ended();
        let saving = platform::saving();
        if allowed(Asked::Recording, saving.disabled()) == Allowed::Refused {
            let reason = self.saving_refusal(&saving);
            self.notify(
                &NoticeKind::RecordingFailedToStart(reason),
                None,
                NoticeImage::None,
                cx,
            );
            return;
        }
        if self.lock.locked {
            return;
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
                self.notify(
                    &NoticeKind::RecordingFailedToStart(error),
                    None,
                    NoticeImage::None,
                    cx,
                );
                return;
            }
        };
        self.run = Some(Run {
            phase: Phase::Starting { cancelled: false },
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
            let _ = this.update(cx, |this, cx| {
                let started_as = this.run.as_mut().map(|run| run.phase.started());
                match (started, started_as) {
                    (Ok(recording), Some(Started::Show)) => {
                        if let Some(run) = &mut this.run {
                            run.recording = Some(recording);
                            run.started = Instant::now();
                        }
                        this.show_recording(cx);
                    }
                    // Stopped while it was starting: the recorder is stopped
                    // and waited for, its file deleted, and nothing ever said
                    // it was recording.
                    (Ok(recording), _) => {
                        this.run = None;
                        this.saving += 1;
                        let path = path.clone();
                        let discard = cx.background_spawn(async move {
                            let _ = recording.stop();
                            let _ = std::fs::remove_file(&path);
                        });
                        cx.spawn(async move |this, cx| {
                            discard.await;
                            let _ = this.update(cx, |this, _| this.saving -= 1);
                        })
                        .detach();
                    }
                    (Err(ending), started_as) => {
                        this.run = None;
                        let _ = std::fs::remove_file(&path);
                        if started_as == Some(Started::Show) {
                            this.notify(&ending.notice(), None, NoticeImage::None, cx);
                        }
                        this.service.set_recording(false, 0, "");
                    }
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
        // Still starting: it is remembered as stopped, and the start's
        // completion discards it (the `Recording` is not lost).
        if let Some(run) = &mut self.run
            && run.phase.stop() == Stop::CancelStart
        {
            self.service.set_recording(false, 0, "");
            cx.notify();
            return;
        }
        let Some(mut run) = self.run.take() else {
            return;
        };
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
                    this.notify(&ending.notice(), Some(&path), NoticeImage::None, cx);
                } else {
                    let _ = std::fs::remove_file(&path);
                    this.notify(&ending.notice(), None, NoticeImage::None, cx);
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
}

/// Sends an answer to the portal call it belongs to.
fn send(answer: Option<Answer<smol::channel::Sender<Outcome>>>) {
    if let Some((reply, outcome)) = answer {
        let _ = reply.try_send(outcome);
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

#[cfg(test)]
mod tests {
    use super::{Open, OutputPlace, Use, WindowPlace, display_key, render_image, window_size};
    use screenshot_core::area::{AreaSelector, Grip};
    use screenshot_core::compose::Frame;
    use screenshot_core::geometry::{Point, Rect};
    use screenshot_core::interface::{Interface, Kept, Kind, Mode, Purpose};

    const RED: [u8; 4] = [0, 0, 255, 255];
    const BLUE: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];

    fn output(name: &str, logical: Rect, scale: u32, colour: [u8; 4]) -> OutputPlace {
        let frame = Frame::filled(
            logical.width as u32 * scale,
            logical.height as u32 * scale,
            colour,
        );
        let with = Frame::filled(frame.width, frame.height, GREEN);
        OutputPlace {
            name: name.into(),
            logical,
            plain: render_image(&frame),
            pointer: render_image(&with),
            frame,
            frame_pointer: Some(with),
        }
    }

    fn open() -> Open {
        let window_frame = Frame::filled(30, 20, BLUE);
        Open {
            using: Use::Capture,
            origin: screenshot_core::portal::Origin::Person,
            outputs: vec![
                output("A", Rect::new(0, 0, 100, 100), 1, RED),
                output("B", Rect::new(100, 0, 100, 100), 2, BLUE),
            ],
            windows: vec![WindowPlace {
                title: "fixture".into(),
                output: 0,
                rect: Rect::new(10, 10, 30, 20),
                image: render_image(&window_frame),
                image_pointer: render_image(&window_frame),
                frame: window_frame,
                frame_pointer: None,
                activated: true,
            }],
            interface: Interface::open(Kept::default(), Purpose::Picture, true, true, 0, Some(0)),
            area: AreaSelector::remembered(Rect::new(50, 0, 100, 50), 1.0),
            stage: Rect::new(0, 0, 200, 100),
            panel_output: 0,
            grip: Grip::Crosshair,
            hover_screen: None,
            hover_window: None,
            pointer_at: None,
            surfaces: None,
            closing: false,
        }
    }

    #[test]
    fn a_window_is_laid_out_at_its_logical_size_whatever_its_buffer() {
        // A window drawn at scale 1 on a 150 % output: its buffer is its
        // logical size; the compositor's width says so.
        let buffer = Frame::filled(720, 420, RED);
        assert_eq!(window_size(Some(720), &buffer, 1.5), (720, 420));
        // Drawn at scale 2 on the same output.
        let sharp = Frame::filled(1440, 840, RED);
        assert_eq!(window_size(Some(720), &sharp, 1.5), (720, 420));
        // Without the compositor's width, the output's scale.
        assert_eq!(window_size(None, &sharp, 2.0), (720, 420));
        assert_eq!(window_size(Some(0), &buffer, 1.0), (720, 420));
    }

    #[test]
    fn gpui_names_an_output_by_the_uuid_of_its_name() {
        // `gpui_linux` `wayland/display.rs`: Uuid::new_v5(NAMESPACE_DNS, name).
        assert_eq!(
            display_key("WINIT-0"),
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, b"WINIT-0").to_string()
        );
        assert_ne!(display_key("eDP-1"), display_key("HDMI-A-1"));
    }

    #[test]
    fn each_type_takes_its_own_picture_and_the_pointer_switch_picks_the_copy() {
        let mut open = open();
        // An area across both outputs, at the higher scale.
        let area = open.picture().unwrap();
        assert_eq!((area.width, area.height), (200, 100));
        assert_eq!(area.pixel(0, 0), RED);
        assert_eq!(area.pixel(199, 0), BLUE);

        open.interface.choose(Kind::Screen);
        open.interface.screen = 1;
        let screen = open.picture().unwrap();
        assert_eq!((screen.width, screen.height), (200, 200));
        open.interface.show_pointer = true;
        assert_eq!(open.picture().unwrap().pixel(0, 0), GREEN);

        open.interface.show_pointer = false;
        open.interface.choose(Kind::Window);
        let window = open.picture().unwrap();
        assert_eq!((window.width, window.height), (30, 20));
        // A window with no pointer copy keeps its own picture.
        open.interface.show_pointer = true;
        assert_eq!(open.picture().unwrap().pixel(0, 0), BLUE);
    }

    #[test]
    fn a_recording_covers_the_area_or_the_screen_never_a_window() {
        let mut open = open();
        assert_eq!(open.recorded_area(), Some(Rect::new(50, 0, 100, 50)));
        open.interface.choose(Kind::Screen);
        open.interface.screen = 1;
        assert_eq!(open.recorded_area(), Some(Rect::new(100, 0, 100, 100)));
        open.interface.choose(Kind::Window);
        assert_eq!(open.recorded_area(), None);
        assert!(open.interface.set_mode(Mode::Recording));
        assert_eq!(open.interface.kind, Kind::Selection);
    }

    #[test]
    fn a_picked_colour_is_the_frozen_pixel_under_the_pointer() {
        let open = open();
        assert_eq!(open.pixel_at(Point::new(10.0, 10.0)), Some(RED));
        assert_eq!(open.pixel_at(Point::new(150.5, 99.5)), Some(BLUE));
        assert_eq!(open.pixel_at(Point::new(250.0, 10.0)), None);
        assert_eq!(open.output_at(Point::new(100.0, 0.0)), Some(1));
    }

    #[test]
    fn the_arrow_keys_choose_among_screens_or_windows() {
        let mut open = open();
        assert!(open.choices().is_empty());
        open.interface.choose(Kind::Screen);
        assert_eq!(
            open.choices(),
            vec![Rect::new(0, 0, 100, 100), Rect::new(100, 0, 100, 100)]
        );
        open.interface.choose(Kind::Window);
        assert_eq!(open.choices(), vec![Rect::new(10, 10, 30, 20)]);
    }
}

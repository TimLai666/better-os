//! The capture over Wayland: `ext-image-copy-capture-v1` with
//! `ext-image-capture-source-v1` for outputs and windows, the window list
//! from `ext-foreign-toplevel-list-v1` and `zcosmic_toplevel_info_v1`, the
//! current workspaces from `ext-workspace-v1`, and outputs from `wl_output`
//! and `xdg_output`. Every copy is into a `wl_shm` buffer in a memfd.
//!
//! The pointer: each output and window is copied once without and once with
//! `paint_cursors`, so the pointer switch picks a copy rather than drawing a
//! pointer image of its own, and a pointer cursor session on each output says
//! which output the pointer is on.

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use screenshot_core::compose::{Alpha, Frame, Transform};
use screenshot_core::geometry::Rect;
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
    ext_output_image_capture_source_manager_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_cursor_session_v1, ext_image_copy_capture_frame_v1,
    ext_image_copy_capture_manager_v1, ext_image_copy_capture_session_v1,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1, ext_workspace_manager_v1,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

use crate::protocols::toplevel_info_v1::{zcosmic_toplevel_handle_v1, zcosmic_toplevel_info_v1};
use crate::{Capture, CaptureError, OutputInfo, Shot, Snapshot, Want, WindowInfo};

/// How long a copy may take before it counts as refused.
pub const COPY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long COSMIC's window state may take to arrive.
pub const WINDOW_STATE_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub(crate) struct OutputState {
    pub(crate) output: Option<wl_output::WlOutput>,
    pub(crate) name: Option<String>,
    pub(crate) logical_position: Option<(i32, i32)>,
    pub(crate) logical_size: Option<(i32, i32)>,
    pub(crate) mode: Option<(i32, i32)>,
}

impl OutputState {
    fn info(&self) -> Option<OutputInfo> {
        let (x, y) = self.logical_position.unwrap_or((0, 0));
        let (width, height) = self.logical_size.or(self.mode)?;
        Some(OutputInfo {
            name: self.name.clone().unwrap_or_default(),
            logical: Rect::new(x, y, width, height),
        })
    }
}

#[derive(Debug, Default)]
struct ToplevelState {
    handle: Option<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1>,
    title: String,
    app_id: String,
    closed: bool,
    outputs: Vec<ObjectId>,
    workspaces: Vec<ObjectId>,
    activated: bool,
    minimized: bool,
    sticky: bool,
    cosmic_done: bool,
}

#[derive(Debug, Default)]
struct WorkspaceState {
    active: bool,
}

/// One copy in flight.
#[derive(Debug, Default)]
pub(crate) struct Job {
    pub(crate) buffer_size: Option<(u32, u32)>,
    pub(crate) formats: Vec<wl_shm::Format>,
    pub(crate) constraints_done: bool,
    pub(crate) stopped: bool,
    pub(crate) frame_started: bool,
    pub(crate) ready: bool,
    pub(crate) failed: Option<String>,
    /// The buffer's turn relative to the screen.
    pub(crate) transform: Transform,
    pub(crate) memory: Option<File>,
    pub(crate) chosen: Option<(wl_shm::Format, u32)>,
    pub(crate) buffer: Option<wl_buffer::WlBuffer>,
    pub(crate) session: Option<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1>,
    pub(crate) frame: Option<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1>,
}

impl Job {
    pub(crate) fn finished(&self) -> bool {
        self.ready || self.failed.is_some() || self.stopped
    }
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) outputs: Vec<OutputState>,
    toplevels: Vec<ToplevelState>,
    workspaces: HashMap<ObjectId, WorkspaceState>,
    pub(crate) jobs: Vec<Job>,
    pointer_on: Option<usize>,
    list_finished: bool,
    /// `zcosmic_toplevel_info_v1.done`: every handle asked for has its state.
    cosmic_done: bool,
}

/// The globals Better Screenshot binds.
pub(crate) struct Globals {
    pub(crate) shm: wl_shm::WlShm,
    pub(crate) copy: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    pub(crate) output_sources:
        ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
    window_sources: Option<
        ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    >,
    toplevel_list: Option<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1>,
    toplevel_info: Option<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1>,
    seat: Option<wl_seat::WlSeat>,
}

/// A connection with its queue, state, and globals.
pub(crate) struct Client {
    pub(crate) connection: Connection,
    pub(crate) queue: EventQueue<State>,
    pub(crate) handle: QueueHandle<State>,
    pub(crate) state: State,
    pub(crate) globals: Globals,
    _list: GlobalList,
}

impl Client {
    /// Connects to `WAYLAND_DISPLAY` and reads the outputs.
    pub(crate) fn connect(want_windows: bool) -> Result<Client, CaptureError> {
        let connection =
            Connection::connect_to_env().map_err(|error| CaptureError::Connect(error.to_string()))?;
        let (list, queue) = registry_queue_init::<State>(&connection)
            .map_err(|error| CaptureError::Protocol(error.to_string()))?;
        let handle = queue.handle();
        let shm = list
            .bind::<wl_shm::WlShm, _, _>(&handle, 1..=1, ())
            .map_err(|_| CaptureError::Missing("wl_shm"))?;
        let copy = list
            .bind::<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1, _, _>(
                &handle,
                1..=1,
                (),
            )
            .map_err(|_| CaptureError::Missing("ext_image_copy_capture_manager_v1"))?;
        let output_sources = list
            .bind::<ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1, _, _>(
                &handle,
                1..=1,
                (),
            )
            .map_err(|_| CaptureError::Missing("ext_output_image_capture_source_manager_v1"))?;
        let xdg_outputs = list
            .bind::<zxdg_output_manager_v1::ZxdgOutputManagerV1, _, _>(&handle, 1..=3, ())
            .ok();
        let seat = list.bind::<wl_seat::WlSeat, _, _>(&handle, 1..=5, ()).ok();
        let (window_sources, toplevel_list, toplevel_info) = if want_windows {
            (
                list.bind::<ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1, _, _>(&handle, 1..=1, ()).ok(),
                list.bind::<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, _, _>(&handle, 1..=1, ()).ok(),
                list.bind::<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, _, _>(&handle, 2..=3, ()).ok(),
            )
        } else {
            (None, None, None)
        };
        if want_windows {
            let _ = list.bind::<ext_workspace_manager_v1::ExtWorkspaceManagerV1, _, _>(
                &handle,
                1..=1,
                (),
            );
        }

        let mut state = State::default();
        let outputs: Vec<_> = list.contents().with_list(|globals| {
            globals
                .iter()
                .filter(|global| global.interface == wl_output::WlOutput::interface().name)
                .map(|global| (global.name, global.version))
                .collect()
        });
        for (index, (name, version)) in outputs.into_iter().enumerate() {
            let output = list.registry().bind::<wl_output::WlOutput, _, _>(
                name,
                version.min(4),
                &handle,
                index,
            );
            if let Some(manager) = &xdg_outputs {
                manager.get_xdg_output(&output, &handle, index);
            }
            state.outputs.push(OutputState {
                output: Some(output),
                ..OutputState::default()
            });
        }

        let mut client = Client {
            connection,
            queue,
            handle,
            state,
            globals: Globals {
                shm,
                copy,
                output_sources,
                window_sources,
                toplevel_list,
                toplevel_info,
                seat,
            },
            _list: list,
        };
        client.roundtrip()?;
        client.roundtrip()?;
        Ok(client)
    }

    pub(crate) fn roundtrip(&mut self) -> Result<(), CaptureError> {
        self.queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|error| CaptureError::Protocol(error.to_string()))
    }

    /// Dispatches until `done` says so or the deadline passes.
    pub(crate) fn dispatch_until(
        &mut self,
        deadline: Instant,
        done: impl Fn(&State) -> bool,
    ) -> Result<(), CaptureError> {
        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|error| CaptureError::Protocol(error.to_string()))?;
            if done(&self.state) {
                return Ok(());
            }
            self.queue
                .flush()
                .map_err(|error| CaptureError::Protocol(error.to_string()))?;
            let now = Instant::now();
            if now >= deadline {
                return Err(CaptureError::Timeout);
            }
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let fd = guard.connection_fd().as_raw_fd();
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let wait = (deadline - now).as_millis().min(i32::MAX as u128) as i32;
            // SAFETY: one valid pollfd for the duration of the call.
            let ready = unsafe { libc::poll(&mut poll, 1, wait) };
            if ready > 0 {
                match guard.read() {
                    Ok(_) => {}
                    // Another reader took the bytes first: nothing to do.
                    Err(wayland_client::backend::WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(CaptureError::Protocol(error.to_string())),
                }
            } else {
                drop(guard);
            }
        }
    }

    pub(crate) fn output_infos(&self) -> Vec<Option<OutputInfo>> {
        self.state.outputs.iter().map(OutputState::info).collect()
    }

    /// Starts copying a source into a new job and returns its index.
    pub(crate) fn start_job(
        &mut self,
        source: &ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        pointer: bool,
    ) -> usize {
        let index = self.state.jobs.len();
        let options = if pointer {
            ext_image_copy_capture_manager_v1::Options::PaintCursors
        } else {
            ext_image_copy_capture_manager_v1::Options::empty()
        };
        let session = self
            .globals
            .copy
            .create_session(source, options, &self.handle, index);
        self.state.jobs.push(Job {
            session: Some(session),
            ..Job::default()
        });
        index
    }

    /// Attaches a buffer to every job whose constraints arrived and asks
    /// for its frame.
    pub(crate) fn request_frames(&mut self) -> Result<(), CaptureError> {
        for index in 0..self.state.jobs.len() {
            let job = &self.state.jobs[index];
            if job.frame_started || !job.constraints_done || job.finished() {
                continue;
            }
            let Some((width, height)) = job.buffer_size else {
                self.state.jobs[index].failed = Some("no buffer size".into());
                continue;
            };
            let Some(format) = choose_format(&job.formats) else {
                self.state.jobs[index].failed = Some(format!(
                    "no shm format this reads among {:?}",
                    job.formats
                ));
                continue;
            };
            let stride = width * 4;
            let size = (stride * height) as usize;
            let memory = memfd(size).map_err(|error| CaptureError::Failed(error.to_string()))?;
            let pool = self
                .globals
                .shm
                .create_pool(memory.as_fd(), size as i32, &self.handle, ());
            let buffer = pool.create_buffer(
                0,
                width as i32,
                height as i32,
                stride as i32,
                format,
                &self.handle,
                (),
            );
            pool.destroy();
            let session = self.state.jobs[index]
                .session
                .clone()
                .expect("a job has its session");
            let frame = session.create_frame(&self.handle, index);
            frame.attach_buffer(&buffer);
            frame.damage_buffer(0, 0, width as i32, height as i32);
            frame.capture();
            let job = &mut self.state.jobs[index];
            job.frame_started = true;
            job.memory = Some(memory);
            job.chosen = Some((format, stride));
            job.buffer = Some(buffer);
            job.frame = Some(frame);
        }
        Ok(())
    }

    /// The pixels in a job's buffer now, blue-green-red-alpha whatever the
    /// format was. The job keeps its session and buffer.
    pub(crate) fn read_job_frame(&self, index: usize) -> Result<Frame, CaptureError> {
        let job = &self.state.jobs[index];
        let (width, height) = job.buffer_size.unwrap_or((0, 0));
        let Some((format, stride)) = job.chosen else {
            return Err(CaptureError::Failed("no buffer".into()));
        };
        let mut data = vec![0u8; (stride * height) as usize];
        job.memory
            .as_ref()
            .ok_or_else(|| CaptureError::Failed("no buffer".into()))?
            .read_exact_at(&mut data, 0)
            .map_err(|error| CaptureError::Failed(error.to_string()))?;
        if matches!(format, wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888) {
            // Red first in memory: swap to blue first.
            for pixel in data.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        let frame = Frame {
            width,
            height,
            stride,
            alpha: if matches!(format, wl_shm::Format::Argb8888 | wl_shm::Format::Abgr8888) {
                Alpha::Straight
            } else {
                Alpha::Ignored
            },
            data,
        };
        Ok(frame.upright(job.transform))
    }

    /// The finished job's pixels, and frees its objects.
    pub(crate) fn take_frame(&mut self, index: usize) -> Result<Frame, CaptureError> {
        let job = &mut self.state.jobs[index];
        let result = if let Some(reason) = job.failed.take() {
            Err(CaptureError::Failed(reason))
        } else if job.stopped && !job.ready {
            Err(CaptureError::Failed("the compositor stopped the copy".into()))
        } else if !job.ready {
            Err(CaptureError::Timeout)
        } else {
            self.read_job_frame(index)
        };
        let job = &mut self.state.jobs[index];
        if let Some(frame) = job.frame.take() {
            frame.destroy();
        }
        if let Some(buffer) = job.buffer.take() {
            buffer.destroy();
        }
        if let Some(session) = job.session.take() {
            session.destroy();
        }
        job.memory = None;
        result
    }

    /// Runs jobs to the end, all at once.
    pub(crate) fn run_jobs(&mut self, jobs: &[usize]) -> Result<(), CaptureError> {
        let deadline = Instant::now() + COPY_TIMEOUT;
        let jobs = jobs.to_vec();
        loop {
            self.request_frames()?;
            let all_done = jobs.iter().all(|index| self.state.jobs[*index].finished());
            if all_done {
                return Ok(());
            }
            let pending = jobs.clone();
            let result = self.dispatch_until(deadline, move |state| {
                pending.iter().all(|index| {
                    let job = &state.jobs[*index];
                    job.finished() || (job.constraints_done && !job.frame_started)
                })
            });
            match result {
                Ok(()) => continue,
                // What did not finish is reported per job.
                Err(CaptureError::Timeout) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }
}

/// The 8-bit formats with four bytes a pixel: `xrgb8888` and `argb8888`
/// (blue first in memory, read as they are) before `xbgr8888` and
/// `abgr8888` (red first, swapped on reading), which are the only ones
/// `cosmic-comp` offers (its GLES renderer; seen 2026-11-21: `Abgr8888,
/// Xbgr8888, Abgr2101010, Xbgr2101010`).
fn choose_format(formats: &[wl_shm::Format]) -> Option<wl_shm::Format> {
    [
        wl_shm::Format::Xrgb8888,
        wl_shm::Format::Argb8888,
        wl_shm::Format::Xbgr8888,
        wl_shm::Format::Abgr8888,
    ]
        .into_iter()
        .find(|format| formats.contains(format))
}

fn memfd(size: usize) -> std::io::Result<File> {
    let name = c"better-screenshot";
    // SAFETY: a valid C string; the result is checked.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a new descriptor this process owns.
    let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    file.set_len(size as u64)?;
    Ok(file)
}

/// The capture of the session's compositor.
#[derive(Debug, Default)]
pub struct WaylandCapture;

impl Capture for WaylandCapture {
    fn snapshot(&mut self, want: Want) -> Result<Snapshot, CaptureError> {
        let windows_wanted = want.windows || want.focused_window_only;
        let mut client = Client::connect(windows_wanted)?;
        let infos = client.output_infos();
        if infos.iter().all(Option::is_none) {
            return Err(CaptureError::NoOutputs);
        }

        // Which output the pointer is on: a cursor session per output says
        // `enter` on the one it is over.
        let pointer = client.globals.seat.as_ref().map(|seat| seat.get_pointer(&client.handle, ()));
        let mut cursor_sessions = Vec::new();

        let mut output_jobs = Vec::new();
        if !want.focused_window_only {
            for index in 0..client.state.outputs.len() {
                let Some(output) = client.state.outputs[index].output.clone() else {
                    continue;
                };
                if infos[index].is_none() {
                    continue;
                }
                let source = client
                    .globals
                    .output_sources
                    .create_source(&output, &client.handle, ());
                let plain = client.start_job(&source, false);
                let with_pointer = client.start_job(&source, true);
                if let Some(pointer) = &pointer {
                    cursor_sessions.push(client.globals.copy.create_pointer_cursor_session(
                        &source,
                        pointer,
                        &client.handle,
                        index,
                    ));
                }
                output_jobs.push((index, plain, with_pointer, source));
            }
        }

        let mut window_jobs = Vec::new();
        if windows_wanted {
            client.query_windows()?;
            let active_workspaces: Vec<ObjectId> = client
                .state
                .workspaces
                .iter()
                .filter(|(_, workspace)| workspace.active)
                .map(|(id, _)| id.clone())
                .collect();
            let output_ids: Vec<Option<ObjectId>> = client
                .state
                .outputs
                .iter()
                .map(|output| output.output.as_ref().map(Proxy::id))
                .collect();
            for index in 0..client.state.toplevels.len() {
                let toplevel = &client.state.toplevels[index];
                if toplevel.closed || toplevel.minimized {
                    continue;
                }
                // On a current workspace (or sticky), when the compositor
                // says which: GNOME's `located_on_workspace(active)`.
                let on_current = toplevel.sticky
                    || toplevel.workspaces.is_empty()
                    || active_workspaces.is_empty()
                    || toplevel
                        .workspaces
                        .iter()
                        .any(|workspace| active_workspaces.contains(workspace));
                if !on_current {
                    continue;
                }
                if want.focused_window_only && !toplevel.activated {
                    continue;
                }
                let output = toplevel.outputs.first().and_then(|id| {
                    output_ids
                        .iter()
                        .position(|candidate| candidate.as_ref() == Some(id))
                        .and_then(|position| client.state.outputs[position].name.clone())
                });
                let info = WindowInfo {
                    title: toplevel.title.clone(),
                    app_id: toplevel.app_id.clone(),
                    output,
                    activated: toplevel.activated,
                };
                let (Some(handle), Some(sources)) =
                    (toplevel.handle.clone(), client.globals.window_sources.clone())
                else {
                    continue;
                };
                let source = sources.create_source(&handle, &client.handle, ());
                let plain = client.start_job(&source, false);
                let with_pointer = client.start_job(&source, true);
                window_jobs.push((info, plain, with_pointer, source));
            }
            if want.focused_window_only && window_jobs.is_empty() {
                return Err(CaptureError::Failed("no window has the keyboard".into()));
            }
        }

        let all: Vec<usize> = output_jobs
            .iter()
            .flat_map(|(_, plain, with, _)| [*plain, *with])
            .chain(window_jobs.iter().flat_map(|(_, plain, with, _)| [*plain, *with]))
            .collect();
        client.run_jobs(&all)?;
        // The cursor sessions' `enter` arrives with the first frames.
        let _ = client.roundtrip();

        let mut snapshot = Snapshot::default();
        for (index, plain, with_pointer, source) in output_jobs {
            let plain = client.take_frame(plain)?;
            let with_pointer = client.take_frame(with_pointer).ok();
            source.destroy();
            let info = infos[index].clone().expect("checked above");
            if client.state.pointer_on == Some(index) {
                snapshot.pointer_output = Some(snapshot.outputs.len());
            }
            snapshot.outputs.push((info, Shot { plain, with_pointer }));
        }
        for (info, plain, with_pointer, source) in window_jobs {
            let plain = client.take_frame(plain);
            let with_pointer = client.take_frame(with_pointer).ok();
            source.destroy();
            // A window that would not copy is left out, not drawn empty.
            if let Ok(plain) = plain {
                snapshot.windows.push((info, Shot { plain, with_pointer }));
            }
        }
        for session in cursor_sessions {
            session.destroy();
        }
        if let Some(pointer) = pointer {
            pointer.release();
        }
        let _ = client.connection.flush();
        Ok(snapshot)
    }
}

// --- dispatch ---------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Mode { width, height, .. } => output.mode = Some((width, height)),
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical_position = Some((x, y));
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_size = Some((width, height));
            }
            zxdg_output_v1::Event::Name { name } => {
                output.name.get_or_insert(name);
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(job) = state.jobs.get_mut(*index) else {
            return;
        };
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => job.buffer_size = Some((width, height)),
            Event::ShmFormat {
                format: WEnum::Value(format),
            } => job.formats.push(format),
            Event::Done => job.constraints_done = true,
            Event::Stopped => job.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(job) = state.jobs.get_mut(*index) else {
            return;
        };
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Transform { transform } => {
                job.transform = Transform::from_protocol(match transform {
                    WEnum::Value(value) => value as u32,
                    WEnum::Unknown(value) => value,
                });
            }
            Event::Ready => job.ready = true,
            Event::Failed { reason } => {
                job.failed = Some(match reason {
                    WEnum::Value(reason) => format!("{reason:?}"),
                    WEnum::Unknown(code) => format!("reason {code}"),
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_cursor_session_v1::ExtImageCopyCaptureCursorSessionV1, usize>
    for State
{
    fn event(
        state: &mut Self,
        _: &ext_image_copy_capture_cursor_session_v1::ExtImageCopyCaptureCursorSessionV1,
        event: ext_image_copy_capture_cursor_session_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_cursor_session_v1::Event;
        match event {
            Event::Enter => state.pointer_on = Some(*index),
            Event::Leave if state.pointer_on == Some(*index) => state.pointer_on = None,
            _ => {}
        }
    }
}

impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                state.toplevels.push(ToplevelState {
                    handle: Some(toplevel),
                    ..ToplevelState::default()
                });
            }
            ext_foreign_toplevel_list_v1::Event::Finished => state.list_finished = true,
            _ => {}
        }
    }

    event_created_child!(State, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let Some(index) = state
            .toplevels
            .iter()
            .position(|toplevel| toplevel.handle.as_ref() == Some(handle))
        else {
            return;
        };
        use ext_foreign_toplevel_handle_v1::Event;
        match event {
            Event::Title { title } => state.toplevels[index].title = title,
            Event::AppId { app_id } => state.toplevels[index].app_id = app_id,
            Event::Closed => state.toplevels[index].closed = true,
            _ => {}
        }
        let _ = queue;
    }
}

/// Asks COSMIC for the extra state of every listed window that has none yet.
/// Called from the list's `done`, so new handles get it in the same pass.
fn ask_cosmic(state: &State, info: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, queue: &QueueHandle<State>) {
    for (index, toplevel) in state.toplevels.iter().enumerate() {
        if let Some(handle) = &toplevel.handle {
            info.get_cosmic_toplevel(handle, queue, index);
        }
    }
}

impl Dispatch<zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Done = event {
            state.cosmic_done = true;
        }
    }

    event_created_child!(State, zcosmic_toplevel_info_v1::ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, usize::MAX),
    ]);
}

impl Dispatch<zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(toplevel) = state.toplevels.get_mut(*index) else {
            return;
        };
        use zcosmic_toplevel_handle_v1::Event;
        match event {
            Event::OutputEnter { output } => {
                if !toplevel.outputs.contains(&output.id()) {
                    toplevel.outputs.push(output.id());
                }
            }
            Event::OutputLeave { output } => toplevel.outputs.retain(|id| *id != output.id()),
            Event::ExtWorkspaceEnter { workspace } => {
                if !toplevel.workspaces.contains(&workspace.id()) {
                    toplevel.workspaces.push(workspace.id());
                }
            }
            Event::ExtWorkspaceLeave { workspace } => {
                toplevel.workspaces.retain(|id| *id != workspace.id());
            }
            Event::State { state: states } => {
                let values: Vec<u32> = states
                    .chunks_exact(4)
                    .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .collect();
                toplevel.activated =
                    values.contains(&(zcosmic_toplevel_handle_v1::State::Activated as u32));
                toplevel.minimized =
                    values.contains(&(zcosmic_toplevel_handle_v1::State::Minimized as u32));
                toplevel.sticky =
                    values.contains(&(zcosmic_toplevel_handle_v1::State::Sticky as u32));
            }
            Event::Done => toplevel.cosmic_done = true,
            Event::Closed => toplevel.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_workspace_manager_v1::ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_workspace_manager_v1::ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_workspace_manager_v1::Event::Workspace { workspace } = event {
            state.workspaces.entry(workspace.id()).or_default();
        }
    }

    event_created_child!(State, ext_workspace_manager_v1::ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ext_workspace_handle_v1::ExtWorkspaceHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        workspace: &ext_workspace_handle_v1::ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::State { state: flags } => {
                let active = match flags {
                    WEnum::Value(flags) => flags.contains(ext_workspace_handle_v1::State::Active),
                    WEnum::Unknown(bits) => bits & 1 != 0,
                };
                state.workspaces.entry(workspace.id()).or_default().active = active;
            }
            ext_workspace_handle_v1::Event::Removed => {
                state.workspaces.remove(&workspace.id());
            }
            _ => {}
        }
    }
}

macro_rules! ignore_events {
    ($($interface:ty => $data:ty),* $(,)?) => {
        $(
            impl Dispatch<$interface, $data> for State {
                fn event(
                    _: &mut Self,
                    _: &$interface,
                    _: <$interface as Proxy>::Event,
                    _: &$data,
                    _: &Connection,
                    _: &QueueHandle<Self>,
                ) {
                }
            }
        )*
    };
}

ignore_events!(
    wl_shm::WlShm => (),
    wl_shm_pool::WlShmPool => (),
    wl_buffer::WlBuffer => (),
    wl_seat::WlSeat => (),
    wl_pointer::WlPointer => (),
    zxdg_output_manager_v1::ZxdgOutputManagerV1 => (),
    ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1 => (),
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1 => (),
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1 => (),
    ext_image_capture_source_v1::ExtImageCaptureSourceV1 => (),
    ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1 => (),
);

impl Client {
    /// Asks for COSMIC's window state once the list has named the windows.
    pub(crate) fn query_windows(&mut self) -> Result<(), CaptureError> {
        self.roundtrip()?;
        if let Some(info) = self.globals.toplevel_info.clone() {
            ask_cosmic(&self.state, &info, &self.handle);
            // cosmic-comp sends a new handle's state on its next refresh,
            // not in answer to the request, so a roundtrip is not enough.
            self.state.cosmic_done = false;
            let waited = self.dispatch_until(Instant::now() + WINDOW_STATE_WAIT, |state| {
                state.cosmic_done
            });
            match waited {
                Ok(()) | Err(CaptureError::Timeout) => {}
                Err(error) => return Err(error),
            }
        }
        if std::env::var_os("BETTER_SCREENSHOT_DEBUG").is_some() {
            eprintln!(
                "toplevel info bound: {:?}",
                self.globals.toplevel_info.as_ref().map(Proxy::version)
            );
            for toplevel in &self.state.toplevels {
                eprintln!(
                    "toplevel app={:?} title={:?} outputs={:?} workspaces={:?} activated={} minimized={} done={}",
                    toplevel.app_id,
                    toplevel.title,
                    toplevel.outputs,
                    toplevel.workspaces,
                    toplevel.activated,
                    toplevel.minimized,
                    toplevel.cosmic_done
                );
            }
            eprintln!("workspaces {:?}", self.state.workspaces);
        }
        Ok(())
    }
}

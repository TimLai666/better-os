//! `better-wl-probe`: the nested test compositor's eyes and hands, as a plain
//! Wayland client of whatever `WAYLAND_DISPLAY` names.
//!
//!   better-wl-probe globals              every global and its version
//!   better-wl-probe outputs              every output: name, mode, scale
//!   better-wl-probe shot FILE.png [--output NAME]
//!                                        a wlr-screencopy of one output
//!   better-wl-probe find FILE.png RRGGBB where a colour is in a screenshot
//!   better-wl-probe keys TEXT            types TEXT on a virtual keyboard
//!   better-wl-probe click X Y W H [--output NAME]
//!                                        a left click at X,Y of a W by H
//!                                        output, on a virtual pointer
//!
//! `shot` needs `zwlr_screencopy_manager_v1` and `click`
//! `zwlr_virtual_pointer_manager_v1`: sway has both, cosmic-comp neither, so
//! the nested setup photographs and clicks the outer sway, which shows
//! cosmic-comp's window and passes the pointer on. `keys` needs
//! `zwp_virtual_keyboard_manager_v1` (both have it).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, FromRawFd};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use layer_check::keys::{self, LEFT_SHIFT};
use layer_check::pixels::{self, Format};
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

#[derive(Default)]
struct OutputState {
    name: Option<String>,
    mode: Option<(i32, i32)>,
    scale: Option<i32>,
    position: Option<(i32, i32)>,
}

#[derive(Default)]
struct FrameState {
    buffer: Option<(u32, u32, u32, u32)>,
    buffer_done: bool,
    y_invert: bool,
    ready: bool,
    failed: bool,
}

#[derive(Default)]
struct State {
    outputs: BTreeMap<u32, OutputState>,
    frame: FrameState,
}

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

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let output = state.outputs.entry(*name).or_default();
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Mode { width, height, .. } => output.mode = Some((width, height)),
            wl_output::Event::Scale { factor } => output.scale = Some(factor),
            wl_output::Event::Geometry { x, y, .. } => output.position = Some((x, y)),
            _ => {}
        }
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let code = match format {
                    WEnum::Value(format) => format as u32,
                    WEnum::Unknown(code) => code,
                };
                // The first shm format offered that the probe can read.
                if state.frame.buffer.is_none() && Format::from_shm(code).is_some() {
                    state.frame.buffer = Some((code, width, height, stride));
                }
            }
            Event::BufferDone => state.frame.buffer_done = true,
            Event::Flags {
                flags: WEnum::Value(flags),
            } => {
                state.frame.y_invert = flags.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
            }
            Event::Ready { .. } => state.frame.ready = true,
            Event::Failed => state.frame.failed = true,
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(State: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(State: ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);
wayland_client::delegate_noop!(State: ignore zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(State: ignore zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1);
wayland_client::delegate_noop!(State: ignore zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(State: ignore zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

fn connect() -> Result<(Connection, GlobalList, EventQueue<State>), String> {
    let connection = Connection::connect_to_env().map_err(|error| format!("connect: {error}"))?;
    let (globals, queue) =
        registry_queue_init::<State>(&connection).map_err(|error| format!("registry: {error}"))?;
    Ok((connection, globals, queue))
}

fn bind_outputs(
    globals: &GlobalList,
    queue: &mut EventQueue<State>,
    state: &mut State,
) -> Vec<(u32, wl_output::WlOutput)> {
    let qh = queue.handle();
    let outputs: Vec<(u32, wl_output::WlOutput)> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|global| global.interface == "wl_output")
        .map(|global| {
            let output = globals.registry().bind::<wl_output::WlOutput, _, _>(
                global.name,
                global.version.min(4),
                &qh,
                global.name,
            );
            (global.name, output)
        })
        .collect();
    let _ = queue.roundtrip(state);
    let _ = queue.roundtrip(state);
    outputs
}

fn memfd(name: &str, size: usize) -> Result<File, String> {
    let name = CString::new(name).map_err(|error| error.to_string())?;
    // SAFETY: memfd_create takes a NUL-terminated name and flags; a negative
    // return is an error, otherwise the descriptor is ours to own.
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!("memfd_create: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: fd was just returned by memfd_create and is owned by nobody else.
    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(size as u64)
        .map_err(|error| error.to_string())?;
    Ok(file)
}

fn globals_command() -> Result<(), String> {
    let (_connection, globals, _queue) = connect()?;
    let mut list = globals.contents().clone_list();
    list.sort_by(|a, b| a.interface.cmp(&b.interface));
    for global in list {
        println!("{} {}", global.interface, global.version);
    }
    Ok(())
}

fn outputs_command() -> Result<(), String> {
    let (_connection, globals, mut queue) = connect()?;
    let mut state = State::default();
    bind_outputs(&globals, &mut queue, &mut state);
    for output in state.outputs.values() {
        println!(
            "{} mode={} scale={} position={}",
            output.name.as_deref().unwrap_or("(no name)"),
            output
                .mode
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_else(|| "(none)".into()),
            output
                .scale
                .map(|scale| scale.to_string())
                .unwrap_or_else(|| "(none)".into()),
            output
                .position
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "(none)".into()),
        );
    }
    Ok(())
}

fn shot_command(path: &str, wanted: Option<&str>) -> Result<(), String> {
    let (_connection, globals, mut queue) = connect()?;
    let qh = queue.handle();
    let mut state = State::default();
    let outputs = bind_outputs(&globals, &mut queue, &mut state);
    let output = match wanted {
        Some(name) => outputs
            .iter()
            .find(|(id, _)| state.outputs.get(id).and_then(|o| o.name.as_deref()) == Some(name))
            .map(|(_, output)| output.clone())
            .ok_or_else(|| format!("no output named {name}"))?,
        None => outputs
            .first()
            .map(|(_, output)| output.clone())
            .ok_or("no output")?,
    };
    let manager: zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .map_err(|_| "this compositor has no zwlr_screencopy_manager_v1".to_string())?;
    let shm: wl_shm::WlShm = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "no wl_shm".to_string())?;
    let frame = manager.capture_output(0, &output, &qh, ());

    let deadline = Instant::now() + Duration::from_secs(5);
    let version = manager.version();
    while !(state.frame.buffer_done || (version < 3 && state.frame.buffer.is_some())) {
        if state.frame.failed || Instant::now() > deadline {
            return Err("the compositor offered no buffer".into());
        }
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| e.to_string())?;
    }
    let (code, width, height, stride) = state
        .frame
        .buffer
        .ok_or("no wl_shm format the probe can read was offered")?;
    let size = (stride * height) as usize;
    let mut file = memfd("better-wl-probe-shot", size)?;
    let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
    let format = WEnum::<wl_shm::Format>::from(code)
        .into_result()
        .map_err(|_| format!("format {code:#x}"))?;
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        format,
        &qh,
        (),
    );
    frame.copy(&buffer);
    while !state.frame.ready {
        if state.frame.failed || Instant::now() > deadline {
            return Err("the copy failed".into());
        }
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| e.to_string())?;
    }
    let mut data = vec![0u8; size];
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.read_exact(&mut data).map_err(|e| e.to_string())?;
    let rgba = pixels::to_rgba(
        Format::from_shm(code).ok_or("format")?,
        &data,
        width as usize,
        height as usize,
        stride as usize,
        state.frame.y_invert,
    )
    .ok_or("the buffer is smaller than it said")?;
    image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8)
        .map_err(|error| format!("{path}: {error}"))?;
    println!("{path} {width}x{height}");
    buffer.destroy();
    pool.destroy();
    frame.destroy();
    Ok(())
}

fn find_command(path: &str, color: &str) -> Result<(), String> {
    let value = u32::from_str_radix(color.trim_start_matches('#'), 16)
        .map_err(|_| format!("{color} is not RRGGBB"))?;
    let rgb = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
    let image = image::open(path)
        .map_err(|error| format!("{path}: {error}"))?
        .to_rgba8();
    match pixels::bounding_box(image.as_raw(), image.width() as usize, rgb, 6) {
        Some((x, y, w, h)) => {
            println!(
                "{color} at x={x} y={y} width={w} height={h} in {}x{}",
                image.width(),
                image.height()
            );
            Ok(())
        }
        None => Err(format!("{color} is not in {path}")),
    }
}

fn keys_command(text: &str) -> Result<(), String> {
    let strokes = keys::plan(text)?;
    let (_connection, globals, mut queue) = connect()?;
    let qh = queue.handle();
    let mut state = State::default();
    let seat: wl_seat::WlSeat = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "no wl_seat".to_string())?;
    let manager: zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "this compositor has no zwp_virtual_keyboard_manager_v1".to_string())?;
    let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());

    let context = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let keymap = xkbcommon::xkb::Keymap::new_from_names(
        &context,
        "",
        "",
        "us",
        "",
        None,
        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or("no US keymap from xkbcommon")?;
    let mut text = keymap.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
    text.push('\0');
    let mut file = memfd("better-wl-probe-keymap", text.len())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    keyboard.keymap(1, file.as_fd(), text.len() as u32);
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;

    let start = Instant::now();
    let time = || start.elapsed().as_millis() as u32;
    let pause = || std::thread::sleep(Duration::from_millis(30));
    for stroke in strokes {
        if stroke.shift {
            keyboard.key(time(), LEFT_SHIFT, 1);
            keyboard.modifiers(1, 0, 0, 0);
        }
        keyboard.key(time(), stroke.code, 1);
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        pause();
        keyboard.key(time(), stroke.code, 0);
        if stroke.shift {
            keyboard.key(time(), LEFT_SHIFT, 0);
            keyboard.modifiers(0, 0, 0, 0);
        }
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        pause();
    }
    keyboard.destroy();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    Ok(())
}

fn click_command(position: [&str; 4], wanted: Option<&str>) -> Result<(), String> {
    let number = |text: &str| {
        text.parse::<u32>()
            .map_err(|_| format!("{text} is not a whole number of pixels"))
    };
    let [x, y, width, height] = [
        number(position[0])?,
        number(position[1])?,
        number(position[2])?,
        number(position[3])?,
    ];
    if x >= width || y >= height {
        return Err(format!("{x},{y} is outside {width}x{height}"));
    }
    let (_connection, globals, mut queue) = connect()?;
    let qh = queue.handle();
    let mut state = State::default();
    let outputs = bind_outputs(&globals, &mut queue, &mut state);
    let output = match wanted {
        Some(name) => Some(
            outputs
                .iter()
                .find(|(id, _)| state.outputs.get(id).and_then(|o| o.name.as_deref()) == Some(name))
                .map(|(_, output)| output.clone())
                .ok_or_else(|| format!("no output named {name}"))?,
        ),
        None => None,
    };
    let seat: wl_seat::WlSeat = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "no wl_seat".to_string())?;
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .map_err(|_| "this compositor has no zwlr_virtual_pointer_manager_v1".to_string())?;
    let pointer = if manager.version() >= 2 {
        manager.create_virtual_pointer_with_output(Some(&seat), output.as_ref(), &qh, ())
    } else {
        manager.create_virtual_pointer(Some(&seat), &qh, ())
    };
    // A new pointer device takes the compositor a moment to announce.
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(150));
    let start = Instant::now();
    let time = || start.elapsed().as_millis() as u32;
    const BTN_LEFT: u32 = 0x110;
    pointer.motion_absolute(time(), x, y, width, height);
    pointer.frame();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(80));
    pointer.button(
        time(),
        BTN_LEFT,
        wayland_client::protocol::wl_pointer::ButtonState::Pressed,
    );
    pointer.frame();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(60));
    pointer.button(
        time(),
        BTN_LEFT,
        wayland_client::protocol::wl_pointer::ButtonState::Released,
    );
    pointer.frame();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(60));
    pointer.destroy();
    queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["globals"] => globals_command(),
        ["outputs"] => outputs_command(),
        ["shot", path] => shot_command(path, None),
        ["shot", path, "--output", name] => shot_command(path, Some(name)),
        ["find", path, color] => find_command(path, color),
        ["keys", text] => keys_command(text),
        ["click", x, y, w, h] => click_command([x, y, w, h], None),
        ["click", x, y, w, h, "--output", name] => click_command([x, y, w, h], Some(name)),
        _ => {
            eprintln!(
                "usage: better-wl-probe globals | outputs | shot FILE.png [--output NAME] | find FILE.png RRGGBB | keys TEXT | click X Y W H [--output NAME]"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("better-wl-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

//! Putting a picture on the clipboard (spec S3) without a focused window:
//! `ext-data-control-v1` on a connection of its own, served from a thread for
//! as long as the clipboard is ours. A Wayland clipboard lives only as long
//! as the client that offers it, so the process stays while
//! [`ClipboardHold::is_held`] says so, and leaves when another program takes
//! the clipboard (`cancelled`).

use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1, ext_data_control_manager_v1, ext_data_control_offer_v1,
    ext_data_control_source_v1,
};

use crate::CaptureError;

/// The type a picture is offered as (GNOME's `St.Clipboard` `image/png`).
pub const PNG: &str = "image/png";

/// The clipboard, while it holds our picture.
#[derive(Debug)]
pub struct ClipboardHold {
    held: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ClipboardHold {
    /// Whether the clipboard still holds the picture.
    pub fn is_held(&self) -> bool {
        self.held.load(Ordering::SeqCst)
    }

    /// Waits for the clipboard to be taken by another program.
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    bytes: Arc<Vec<u8>>,
    cancelled: bool,
}

/// Offers `png` as the clipboard's content, and serves it until another
/// program takes the clipboard.
pub fn hold_png(png: Vec<u8>) -> Result<ClipboardHold, CaptureError> {
    let connection =
        Connection::connect_to_env().map_err(|error| CaptureError::Connect(error.to_string()))?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)
        .map_err(|error| CaptureError::Protocol(error.to_string()))?;
    let handle = queue.handle();
    let seat = globals
        .bind::<wl_seat::WlSeat, _, _>(&handle, 1..=5, ())
        .map_err(|_| CaptureError::Missing("wl_seat"))?;
    let manager = globals
        .bind::<ext_data_control_manager_v1::ExtDataControlManagerV1, _, _>(&handle, 1..=1, ())
        .map_err(|_| CaptureError::Missing("ext_data_control_manager_v1"))?;
    let device = manager.get_data_device(&seat, &handle, ());
    let source = manager.create_data_source(&handle, ());
    source.offer(PNG.to_string());
    device.set_selection(Some(&source));

    let mut state = State {
        bytes: Arc::new(png),
        cancelled: false,
    };
    queue
        .roundtrip(&mut state)
        .map_err(|error| CaptureError::Protocol(error.to_string()))?;
    if state.cancelled {
        return Err(CaptureError::Failed("the clipboard was refused".into()));
    }

    let held = Arc::new(AtomicBool::new(true));
    let flag = held.clone();
    let thread = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            while !state.cancelled {
                if queue.blocking_dispatch(&mut state).is_err() {
                    break;
                }
            }
            source.destroy();
            device.destroy();
            let _ = connection.flush();
            flag.store(false, Ordering::SeqCst);
        })
        .map_err(|error| CaptureError::Failed(error.to_string()))?;
    Ok(ClipboardHold {
        held,
        thread: Some(thread),
    })
}

fn send(bytes: Arc<Vec<u8>>, fd: OwnedFd) {
    // A reader that reads slowly must not stall the clipboard for anyone
    // else, so each request is written on its own thread.
    let _ = std::thread::Builder::new()
        .name("clipboard-send".into())
        .spawn(move || {
            let mut file = std::fs::File::from(fd);
            let _ = file.write_all(&bytes);
        });
}

impl Dispatch<ext_data_control_source_v1::ExtDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_data_control_source_v1::ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } if mime_type == PNG => {
                send(state.bytes.clone(), fd);
            }
            ext_data_control_source_v1::Event::Cancelled => state.cancelled = true,
            _ => {}
        }
    }
}

impl Dispatch<ext_data_control_device_v1::ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_data_control_device_v1::ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_device_v1::Event::Finished = event {
            state.cancelled = true;
        }
    }

    event_created_child!(State, ext_data_control_device_v1::ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ext_data_control_offer_v1::ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ext_data_control_offer_v1::ExtDataControlOfferV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ext_data_control_offer_v1::ExtDataControlOfferV1,
        _: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
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

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_data_control_manager_v1::ExtDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ext_data_control_manager_v1::ExtDataControlManagerV1,
        _: ext_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

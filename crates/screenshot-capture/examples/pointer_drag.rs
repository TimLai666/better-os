//! `pointer-drag W H X Y [X Y ...] [--right]`: presses at the first point of
//! a W × H output of the compositor `WAYLAND_DISPLAY` names, moves through
//! the others in small steps, and releases at the last, on a
//! `zwlr_virtual_pointer_v1`; with one point it only moves there. The nested
//! test compositor's `click` cannot drag, and ticket 253's area selector is
//! dragged (engineering/specs/screenshot.md C3–C8). A development tool for
//! the outer headless sway, as `better-wl-probe click` is.

use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

struct State;

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
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(State: ignore zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let right = args.iter().any(|arg| arg == "--right");
    let numbers: Vec<u32> = args
        .iter()
        .filter(|arg| !arg.starts_with("--"))
        .map(|arg| arg.parse().expect("whole pixels"))
        .collect();
    assert!(numbers.len() >= 4 && numbers.len() % 2 == 0, "pointer-drag W H X Y [X Y ...]");
    let (width, height) = (numbers[0], numbers[1]);
    let points: Vec<(u32, u32)> = numbers[2..].chunks(2).map(|p| (p[0], p[1])).collect();

    let connection = Connection::connect_to_env().expect("a compositor");
    let (globals, mut queue) = registry_queue_init::<State>(&connection).expect("globals");
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).expect("a seat");
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 =
        globals.bind(&qh, 1..=2, ()).expect("zwlr_virtual_pointer_manager_v1");
    let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
    let _ = manager.version();
    queue.roundtrip(&mut State).expect("roundtrip");
    std::thread::sleep(Duration::from_millis(150));
    let start = Instant::now();
    let time = || start.elapsed().as_millis() as u32;
    let button = if right { 0x111 } else { 0x110 };
    let flush = |queue: &mut wayland_client::EventQueue<State>, wait: u64| {
        queue.roundtrip(&mut State).expect("roundtrip");
        std::thread::sleep(Duration::from_millis(wait));
    };

    let (x, y) = points[0];
    pointer.motion_absolute(time(), x, y, width, height);
    pointer.frame();
    flush(&mut queue, 80);
    if points.len() == 1 {
        pointer.destroy();
        flush(&mut queue, 20);
        return;
    }
    pointer.button(time(), button, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    flush(&mut queue, 60);
    let mut last = points[0];
    for &(tx, ty) in &points[1..] {
        let steps = 12;
        for step in 1..=steps {
            let fx = f64::from(last.0) + (f64::from(tx) - f64::from(last.0)) * f64::from(step) / f64::from(steps);
            let fy = f64::from(last.1) + (f64::from(ty) - f64::from(last.1)) * f64::from(step) / f64::from(steps);
            pointer.motion_absolute(time(), fx.round() as u32, fy.round() as u32, width, height);
            pointer.frame();
            flush(&mut queue, 15);
        }
        last = (tx, ty);
    }
    pointer.button(time(), button, wl_pointer::ButtonState::Released);
    pointer.frame();
    flush(&mut queue, 60);
    pointer.destroy();
    flush(&mut queue, 20);
}

//! `capture-probe OUT_DIR [--record SECONDS] [--clipboard]`: runs the real
//! capture against the compositor `WAYLAND_DISPLAY` names and writes what it
//! got, so ticket 253's facts about `cosmic-comp` can be read again
//! (engineering/specs/screenshot.md, "The compositor"). A development tool.

use std::path::PathBuf;
use std::time::Duration;

use screenshot_capture::wayland::WaylandCapture;
use screenshot_capture::{Capture, Want, clipboard, png, recorder};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("capture-probe OUT_DIR [--record S] [--clipboard]"));
    let rest: Vec<String> = args.collect();
    std::fs::create_dir_all(&out).expect("the output folder");

    let mut capture = WaylandCapture;
    let started = std::time::Instant::now();
    let snapshot = match capture.snapshot(Want {
        windows: true,
        ..Want::default()
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("snapshot error: {error}");
            std::process::exit(1);
        }
    };
    println!("snapshot took {:?}", started.elapsed());
    println!("pointer on output {:?}", snapshot.pointer_output);
    for (index, (info, shot)) in snapshot.outputs.iter().enumerate() {
        println!(
            "output {index} {} logical {:?} frame {}x{} alpha {:?} with_pointer {}",
            info.name,
            info.logical,
            shot.plain.width,
            shot.plain.height,
            shot.plain.alpha,
            shot.with_pointer.is_some()
        );
        let differ = shot.with_pointer.as_ref().map(|with| with.data != shot.plain.data);
        println!("  with-pointer copy differs from plain: {differ:?}");
        std::fs::write(out.join(format!("output-{index}.png")), png::encode(&shot.plain).unwrap()).unwrap();
        if let Some(with) = &shot.with_pointer {
            std::fs::write(out.join(format!("output-{index}-pointer.png")), png::encode(with).unwrap()).unwrap();
        }
    }
    for (index, (info, shot)) in snapshot.windows.iter().enumerate() {
        println!(
            "window {index} title {:?} app {:?} output {:?} activated {} frame {}x{}",
            info.title, info.app_id, info.output, info.activated, shot.plain.width, shot.plain.height
        );
        std::fs::write(out.join(format!("window-{index}.png")), png::encode(&shot.plain).unwrap()).unwrap();
    }

    let focused = capture.snapshot(Want {
        focused_window_only: true,
        ..Want::default()
    });
    match focused {
        Ok(snapshot) => println!(
            "focused window: {:?}",
            snapshot.windows.first().map(|(info, _)| info.title.clone())
        ),
        Err(error) => println!("focused window: {error}"),
    }

    if rest.iter().any(|arg| arg == "--clipboard") {
        let bytes = png::encode(&snapshot.outputs[0].1.plain).unwrap();
        match clipboard::hold_png(bytes) {
            Ok(hold) => {
                println!("clipboard held: {}", hold.is_held());
                std::thread::sleep(Duration::from_secs(8));
                println!("clipboard still held after 8 s: {}", hold.is_held());
            }
            Err(error) => println!("clipboard error: {error}"),
        }
    }

    if let Some(position) = rest.iter().position(|arg| arg == "--record") {
        let seconds: u64 = rest.get(position + 1).and_then(|s| s.parse().ok()).unwrap_or(3);
        println!("recording supported: {:?}", recorder::supported());
        let area = snapshot.outputs[0].0.logical;
        let path = out.join("recording.webm");
        std::fs::write(&path, b"").unwrap();
        match recorder::start(recorder::RecordingRequest {
            area,
            pointer: true,
            path: path.clone(),
        }) {
            Ok(recording) => {
                std::thread::sleep(Duration::from_secs(seconds));
                let elapsed = recording.elapsed();
                let ending = recording.stop();
                println!(
                    "recording ended {ending:?} after {elapsed:?}, {} bytes",
                    std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
                );
            }
            Err(ending) => println!("recording did not start: {ending:?}"),
        }
    }
}

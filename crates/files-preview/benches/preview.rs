//! What a preview costs, per kind.
//!
//! The number that matters for a preview pane is not throughput, it is whether
//! one selection change fits in a frame. So everything here is reported as the
//! median of repeated single previews rather than as a rate, and the folder
//! summary is measured at two sizes because it is the one whose cost grows with
//! something the user chose.
//!
//! No harness dependency, matching `files-core` and `files-gui`.
//!
//! Run with `cargo bench -p files-preview`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use files_preview::{CancelToken, PreviewEngine, PreviewLimits, PreviewRequest, PreviewService};

fn report(label: &str, duration: Duration, detail: &str) {
    println!(
        "{label:<44} {:>10.3} ms   {detail}",
        duration.as_secs_f64() * 1000.0
    );
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// A grey PNG of the requested size, written through the encoder so the decode
/// benchmark reads a real file rather than a hand-made one.
fn write_png(path: &Path, width: u32, height: u32) {
    let mut buffer = image::RgbaImage::new(width, height);
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
    }
    buffer.save(path).expect("write png");
}

fn write_text(path: &Path, lines: usize) {
    let mut text = String::new();
    for index in 0..lines {
        text.push_str(&format!("line {index:06}: some source code goes here\n"));
    }
    fs::write(path, text).expect("write text");
}

fn main() {
    let iterations = if std::env::args().any(|a| a == "--test") {
        1
    } else {
        11
    };
    let dir = tempfile::tempdir().expect("benchmark directory");
    let engine = PreviewEngine::default();
    let cancel = CancelToken::new();

    println!("files-preview — preview generation, median of {iterations}");
    println!();

    // --- Images ---------------------------------------------------------
    for (label, width, height) in [
        ("image preview: 512x512 PNG", 512u32, 512u32),
        ("image preview: 2048x1536 PNG", 2048, 1536),
    ] {
        let path = dir.path().join(format!("{width}x{height}.png"));
        write_png(&path, width, height);
        let request = PreviewRequest::file(&path);
        let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut samples = Vec::new();
        for _ in 0..iterations {
            let clock = Instant::now();
            let preview = engine.preview(&request, &cancel).expect("preview");
            samples.push(clock.elapsed());
            std::hint::black_box(preview);
        }
        report(label, median(samples), &format!("{bytes} bytes on disk"));
    }

    // An image whose header is over the limit must cost the header read only.
    let path = dir.path().join("2048x1536.png");
    let refused = PreviewRequest::file(&path).with_limits(PreviewLimits {
        max_image_pixels: 1024,
        ..PreviewLimits::default()
    });
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        let preview = engine.preview(&refused, &cancel).expect("preview");
        samples.push(clock.elapsed());
        std::hint::black_box(preview);
    }
    report(
        "image refused by the pixel limit",
        median(samples),
        "header read only, no decode",
    );

    // --- Text -----------------------------------------------------------
    println!();
    let path = dir.path().join("source.rs");
    write_text(&path, 200_000);
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let request = PreviewRequest::file(&path);
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        let preview = engine.preview(&request, &cancel).expect("preview");
        samples.push(clock.elapsed());
        std::hint::black_box(preview);
    }
    report(
        "text preview: 128 KiB of a large file",
        median(samples),
        &format!("{bytes} bytes on disk, bounded read"),
    );

    // --- Folders --------------------------------------------------------
    println!();
    for count in [1_000usize, 20_000] {
        let folder = dir.path().join(format!("folder{count}"));
        fs::create_dir_all(&folder).expect("folder");
        for index in 0..count {
            fs::write(folder.join(format!("entry{index:06}")), b"x").expect("entry");
        }
        let request = PreviewRequest::directory(&folder);
        let mut samples = Vec::new();
        for _ in 0..iterations {
            let clock = Instant::now();
            let preview = engine.preview(&request, &cancel).expect("preview");
            samples.push(clock.elapsed());
            std::hint::black_box(preview);
        }
        report(
            &format!("folder summary: {count} entries"),
            median(samples),
            "immediate children only",
        );
    }

    // --- The service ----------------------------------------------------
    println!();
    let service = PreviewService::default();
    let path = dir.path().join("source.rs");
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        service.request(PreviewRequest::file(&path));
        samples.push(clock.elapsed());
    }
    report(
        "request from the render thread",
        median(samples),
        "cancel the old one and queue, no I/O",
    );
    // Drain so the worker is not still decoding when the directory is removed.
    while service.poll().is_empty() {
        std::thread::yield_now();
    }
}

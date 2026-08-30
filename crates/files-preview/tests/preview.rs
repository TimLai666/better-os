//! What the preview interface promises, asserted against real files.
//!
//! Every fixture is built here rather than checked in. A PNG is 70 bytes of
//! hand-assembled chunks, so the image tests decode a real file through the
//! real decoder without the repository carrying a binary.

use std::fs;
use std::io::Write;
use std::path::Path;

use app_catalog_core::MimeType;
use files_preview::text::{TextEncoding, decode, is_binary};
use files_preview::{
    CancelToken, DegradeReason, FolderProvider, ImageProvider, Preview, PreviewEngine,
    PreviewLimits, PreviewProvider, PreviewRequest, PreviewService, TextProvider,
};

fn mime(value: &str) -> Option<MimeType> {
    MimeType::parse(value)
}

// --- Fixtures -----------------------------------------------------------

/// A minimal valid PNG: 2x2, 8-bit RGB, one IDAT with a stored deflate block.
fn write_png(path: &Path) {
    fn chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(payload);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&2u32.to_be_bytes()); // width
    ihdr.extend_from_slice(&2u32.to_be_bytes()); // height
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // depth, RGB, deflate, no filter, no interlace

    // Two scanlines of two RGB pixels, each prefixed by filter byte 0.
    let raw: Vec<u8> = vec![
        0, 255, 0, 0, 0, 255, 0, //
        0, 0, 0, 255, 255, 255, 0,
    ];
    let mut idat = vec![0x78, 0x01]; // zlib header
    idat.push(0x01); // final stored block
    idat.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    idat.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    idat.extend_from_slice(&raw);
    idat.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&chunk(b"IHDR", &ihdr));
    png.extend_from_slice(&chunk(b"IDAT", &idat));
    png.extend_from_slice(&chunk(b"IEND", &[]));
    fs::write(path, png).expect("write png");
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// --- The interface ------------------------------------------------------

#[test]
fn the_size_limit_is_applied_before_any_parser_sees_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    fs::write(&path, vec![b'a'; 4096]).unwrap();

    let engine = PreviewEngine::default();
    let request = PreviewRequest::file(&path)
        .with_mime(mime("text/plain"))
        .with_limits(PreviewLimits {
            max_source_bytes: 1024,
            ..PreviewLimits::default()
        });
    let preview = engine.preview(&request, &CancelToken::new()).unwrap();
    assert_eq!(
        preview.degrade_reason(),
        Some(&DegradeReason::TooLarge {
            limit: 1024,
            actual: 4096
        })
    );
}

#[test]
fn a_cancelled_request_returns_nothing_rather_than_a_partial_preview() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, "hello").unwrap();

    let engine = PreviewEngine::default();
    let result = engine.preview(&PreviewRequest::file(&path), &CancelToken::cancelled());
    assert!(
        result.is_err(),
        "a cancelled generation produces no preview"
    );
}

#[test]
fn every_refusal_degrades_to_metadata_carrying_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.tar");
    fs::write(&path, [0u8; 32]).unwrap();

    let engine = PreviewEngine::default();
    let request = PreviewRequest::file(&path).with_mime(mime("application/x-tar"));
    let preview = engine.preview(&request, &CancelToken::new()).unwrap();
    assert!(preview.is_metadata_only());
    assert_eq!(preview.degrade_reason(), Some(&DegradeReason::NoProvider));
    let Preview::Metadata(meta) = preview else {
        unreachable!()
    };
    assert_eq!(meta.size_bytes, Some(32));
    assert_eq!(meta.mime.as_deref(), Some("application/x-tar"));
}

#[test]
fn a_panicking_parser_loses_one_preview_and_nothing_else() {
    struct Exploding;
    impl PreviewProvider for Exploding {
        fn id(&self) -> &'static str {
            "exploding"
        }
        fn handles(&self, _request: &PreviewRequest) -> bool {
            true
        }
        fn generate(
            &self,
            _request: &PreviewRequest,
            _cancel: &CancelToken,
        ) -> Result<Preview, files_preview::PreviewError> {
            panic!("a malformed header");
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hostile.bin");
    fs::write(&path, b"whatever").unwrap();

    // The panic message would otherwise be printed by the default hook, which
    // makes a passing test look like a failing one.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let engine = PreviewEngine::new(vec![Box::new(Exploding)]);
    let preview = engine
        .preview(&PreviewRequest::file(&path), &CancelToken::new())
        .unwrap();
    std::panic::set_hook(previous);

    assert_eq!(
        preview.degrade_reason(),
        Some(&DegradeReason::ParserFaulted),
        "the boundary catches the parser rather than the process dying"
    );
}

#[test]
fn an_engine_with_no_providers_still_answers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, "hello").unwrap();
    let engine = PreviewEngine::metadata_only();
    assert!(
        engine
            .preview(&PreviewRequest::file(&path), &CancelToken::new())
            .unwrap()
            .is_metadata_only()
    );
    assert!(engine.provider_ids().is_empty());
}

// --- Text ---------------------------------------------------------------

#[test]
fn text_is_read_within_the_limit_and_reports_being_cut_short() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    let mut file = fs::File::create(&path).unwrap();
    for _ in 0..1000 {
        writeln!(file, "a line of a log file").unwrap();
    }
    drop(file);

    let request = PreviewRequest::file(&path)
        .with_mime(mime("text/plain"))
        .with_limits(PreviewLimits {
            max_text_bytes: 256,
            ..PreviewLimits::default()
        });
    let preview = TextProvider
        .generate(&request, &CancelToken::new())
        .unwrap();
    let Preview::Text(text) = preview else {
        panic!("expected text")
    };
    assert!(text.truncated);
    assert_eq!(text.text.len(), 256);
    assert_eq!(text.encoding, TextEncoding::Utf8);
    assert_eq!(text.source_bytes, 21_000);
}

#[test]
fn a_file_with_a_nul_byte_is_binary_and_never_rendered_as_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.txt");
    fs::write(&path, b"header\0\x01\x02binary").unwrap();

    let request = PreviewRequest::file(&path).with_mime(mime("text/plain"));
    let error = TextProvider
        .generate(&request, &CancelToken::new())
        .unwrap_err();
    assert_eq!(
        error,
        files_preview::PreviewError::Degraded(DegradeReason::Binary)
    );

    assert!(is_binary(b"a\0b"));
    assert!(!is_binary(b"plain text"));
    assert!(!is_binary(b""));
    // UTF-16 is half NUL bytes and is not binary.
    assert!(!is_binary(&[0xFF, 0xFE, b'h', 0, b'i', 0]));
}

#[test]
fn encoding_detection_says_which_reading_it_is_showing() {
    assert_eq!(decode(b"plain"), ("plain".to_string(), TextEncoding::Utf8));
    assert_eq!(
        decode("\u{feff}marked".as_bytes()),
        ("marked".to_string(), TextEncoding::Utf8),
        "a UTF-8 byte-order mark is stripped, not shown"
    );
    assert_eq!(
        decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]),
        ("hi".to_string(), TextEncoding::Utf16Le)
    );
    assert_eq!(
        decode(&[0xFE, 0xFF, 0, b'h', 0, b'i']),
        ("hi".to_string(), TextEncoding::Utf16Be)
    );
    // 0xE9 is "é" in Latin-1 and invalid on its own in UTF-8.
    assert_eq!(
        decode(b"caf\xE9 au lait"),
        ("café au lait".to_string(), TextEncoding::Latin1)
    );
    // A UTF-8 file cut mid-character stays UTF-8; the partial character goes.
    let mut cut = "naïve".as_bytes().to_vec();
    cut.truncate(3);
    assert_eq!(decode(&cut), ("na".to_string(), TextEncoding::Utf8));
}

#[test]
fn the_text_provider_claims_source_files_and_declines_unknown_ones() {
    let claim = |name: &str, mime_value: Option<&str>| {
        TextProvider.handles(
            &PreviewRequest::file(format!("/tmp/{name}"))
                .with_mime(mime_value.and_then(MimeType::parse)),
        )
    };
    assert!(claim("main.rs", None));
    assert!(claim("config.json", None));
    assert!(claim("anything", Some("text/x-python")));
    assert!(claim("script", Some("application/x-shellscript")));
    assert!(!claim("photo.png", Some("image/png")));
    assert!(
        !claim("mystery", None),
        "no type and no known extension is not claimed on a hunch"
    );
    assert!(!TextProvider.handles(&PreviewRequest::directory("/tmp")));
}

// --- Images -------------------------------------------------------------

#[test]
fn a_real_png_decodes_to_rgba_with_its_source_dimensions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pixel.png");
    write_png(&path);

    let request = PreviewRequest::file(&path).with_mime(mime("image/png"));
    let preview = ImageProvider
        .generate(&request, &CancelToken::new())
        .unwrap();
    let Preview::Image(image) = preview else {
        panic!("expected an image")
    };
    assert_eq!((image.source_width, image.source_height), (2, 2));
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.format, "PNG");
    assert_eq!(image.rgba.len(), 2 * 2 * 4);
    assert_eq!(&image.rgba[0..4], &[255, 0, 0, 255], "first pixel is red");
}

#[test]
fn an_image_over_the_pixel_limit_is_refused_from_its_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pixel.png");
    write_png(&path);

    let request = PreviewRequest::file(&path)
        .with_mime(mime("image/png"))
        .with_limits(PreviewLimits {
            max_image_pixels: 2,
            ..PreviewLimits::default()
        });
    let error = ImageProvider
        .generate(&request, &CancelToken::new())
        .unwrap_err();
    assert_eq!(
        error,
        files_preview::PreviewError::Degraded(DegradeReason::TooLarge {
            limit: 2,
            actual: 4
        })
    );
}

#[test]
fn a_file_that_claims_to_be_an_image_and_is_not_degrades_rather_than_misreads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lying.png");
    fs::write(&path, b"this is not a png at all, not even close").unwrap();

    let request = PreviewRequest::file(&path).with_mime(mime("image/png"));
    let error = ImageProvider
        .generate(&request, &CancelToken::new())
        .unwrap_err();
    let files_preview::PreviewError::Degraded(DegradeReason::DecodeFailed(_)) = error else {
        panic!("expected a decode failure, got {error:?}")
    };
}

#[test]
fn a_large_image_is_downscaled_to_the_thumbnail_edge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pixel.png");
    write_png(&path);

    let request = PreviewRequest::file(&path)
        .with_mime(mime("image/png"))
        .with_limits(PreviewLimits {
            thumbnail_edge: 1,
            ..PreviewLimits::default()
        });
    let Preview::Image(image) = ImageProvider
        .generate(&request, &CancelToken::new())
        .unwrap()
    else {
        panic!("expected an image")
    };
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(
        (image.source_width, image.source_height),
        (2, 2),
        "the file's own dimensions survive the downscale"
    );
}

// --- Folders ------------------------------------------------------------

#[test]
fn a_folder_summary_counts_immediate_children_and_their_size() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"1234567890").unwrap();
    fs::write(dir.path().join("b.txt"), b"12345").unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::write(dir.path().join("nested/deep.txt"), vec![b'x'; 1000]).unwrap();

    let Preview::Folder(summary) = FolderProvider
        .generate(&PreviewRequest::directory(dir.path()), &CancelToken::new())
        .unwrap()
    else {
        panic!("expected a folder summary")
    };
    assert_eq!(summary.files, 2);
    assert_eq!(summary.directories, 1);
    assert_eq!(
        summary.immediate_bytes, 15,
        "the nested file is not counted; a recursive size is not a preview"
    );
    assert!(!summary.truncated);
}

#[test]
fn a_folder_summary_stops_at_the_limit_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..20 {
        fs::write(dir.path().join(format!("f{index}")), b"x").unwrap();
    }
    let request = PreviewRequest::directory(dir.path()).with_limits(PreviewLimits {
        max_folder_entries: 5,
        ..PreviewLimits::default()
    });
    let Preview::Folder(summary) = FolderProvider
        .generate(&request, &CancelToken::new())
        .unwrap()
    else {
        panic!("expected a folder summary")
    };
    assert!(summary.truncated);
    assert_eq!(summary.files, 5);
}

#[test]
fn the_engine_routes_a_directory_to_the_folder_provider() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let engine = PreviewEngine::default();
    let preview = engine
        .preview(&PreviewRequest::directory(dir.path()), &CancelToken::new())
        .unwrap();
    assert!(matches!(preview, Preview::Folder(_)));
    assert_eq!(engine.provider_ids(), ["folder", "image", "text"]);
}

// --- The service --------------------------------------------------------

#[test]
fn the_service_answers_off_the_calling_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, "one\ntwo\nthree").unwrap();

    let service = PreviewService::default();
    let id = service.request(PreviewRequest::file(&path).with_mime(mime("text/plain")));
    let outcome = service.wait().expect("an outcome");
    assert_eq!(outcome.id, id);
    let Some(Preview::Text(text)) = outcome.preview else {
        panic!("expected text, got {:?}", outcome.preview)
    };
    assert_eq!(text.lines, 3);
}

#[test]
fn a_superseded_request_is_cancelled_and_the_newest_one_wins() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..8 {
        fs::write(dir.path().join(format!("f{index}.txt")), "content").unwrap();
    }

    let service = PreviewService::default();
    let mut last = None;
    for index in 0..8 {
        last = Some(
            service.request(
                PreviewRequest::file(dir.path().join(format!("f{index}.txt")))
                    .with_mime(mime("text/plain")),
            ),
        );
    }
    let newest = last.unwrap();

    // Collect until the newest request has answered. Everything before it is
    // either an answer or a cancellation; nothing is lost.
    let mut outcomes = Vec::new();
    while !outcomes
        .iter()
        .any(|o: &files_preview::PreviewOutcome| o.id == newest)
    {
        if let Some(outcome) = service.wait() {
            outcomes.push(outcome);
        } else {
            break;
        }
    }
    assert_eq!(outcomes.len(), 8, "every request is accounted for");
    let final_outcome = outcomes.iter().find(|o| o.id == newest).unwrap();
    assert!(
        final_outcome.preview.is_some(),
        "the newest request is the one that produces a preview"
    );
}

#[test]
fn polling_never_blocks_when_nothing_has_finished() {
    let service = PreviewService::default();
    assert!(service.poll().is_empty());
    service.cancel_in_flight();
    assert!(service.poll().is_empty());
}

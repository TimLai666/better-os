//! The shared thumbnail cache, as the freedesktop Thumbnail Managing
//! Standard defines it and as GNOME, KDE and folio use it
//! (folio: `src/thumbnails.cpp`).
//!
//! A thumbnail of `file:///a/b.png` is `~/.cache/thumbnails/<size>/<md5 of
//! the URI>.png`, and it is only good while its `Thumb::MTime` text chunk
//! equals the file's modification time. A file no thumbnailer could read gets
//! a fail marker, `fail/<program>/<md5>.png`, so nobody tries it again until
//! it changes. Better Files reads thumbnails any program made, and makes
//! them for the image types it can decode itself, writing them where every
//! other program will find them.
//!
//! Nothing is made for a file inside the cache itself, and a file larger
//! than [`MAX_SOURCE_BYTES`] is left to the type icon.

use std::fs;
use std::io::BufWriter;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

/// The sizes the standard names, and the edge each is at most.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThumbSize {
    Normal,
    Large,
}

impl ThumbSize {
    pub fn folder(self) -> &'static str {
        match self {
            ThumbSize::Normal => "normal",
            ThumbSize::Large => "large",
        }
    }

    pub fn edge(self) -> u32 {
        match self {
            ThumbSize::Normal => 128,
            ThumbSize::Large => 256,
        }
    }
}

/// The program name fail markers are filed under.
pub const PROGRAM: &str = "better-files";

/// Larger images are not decoded for a thumbnail.
pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

/// What the cache says about a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    /// A thumbnail that is current, at this path.
    Found(PathBuf),
    /// A fail marker that is current: do not try again.
    Failed,
    /// Nothing current.
    Missing,
}

/// One cache directory.
#[derive(Clone, Debug)]
pub struct ThumbnailCache {
    root: PathBuf,
}

impl ThumbnailCache {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$XDG_CACHE_HOME/thumbnails`, or `~/.cache/thumbnails`.
    pub fn from_env() -> Option<Self> {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".cache"))
            })?;
        Some(Self::at(cache.join("thumbnails")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the thumbnail of `uri` at `size` is, made or not.
    pub fn path_for(&self, uri: &str, size: ThumbSize) -> PathBuf {
        self.root
            .join(size.folder())
            .join(format!("{}.png", hash(uri)))
    }

    /// What the cache holds for `file`: a current thumbnail at `size` (or the
    /// other size), a current fail marker from any program, or nothing.
    pub fn lookup(&self, file: &Path, size: ThumbSize) -> Lookup {
        let Ok(metadata) = fs::metadata(file) else {
            return Lookup::Missing;
        };
        let mtime = metadata.mtime();
        let uri = file_uri(file);
        let other = match size {
            ThumbSize::Normal => ThumbSize::Large,
            ThumbSize::Large => ThumbSize::Normal,
        };
        for size in [size, other] {
            let path = self.path_for(&uri, size);
            if is_current(&path, &uri, mtime) {
                return Lookup::Found(path);
            }
        }
        let name = format!("{}.png", hash(&uri));
        if let Ok(programs) = fs::read_dir(self.root.join("fail")) {
            for program in programs.flatten() {
                if is_current(&program.path().join(&name), &uri, mtime) {
                    return Lookup::Failed;
                }
            }
        }
        Lookup::Missing
    }

    /// Makes the thumbnail of an image file at `size`, writing it where the
    /// standard says, or a fail marker when the file cannot be decoded.
    /// Returns the thumbnail's path, or `None` when there is none.
    pub fn make(&self, file: &Path, size: ThumbSize) -> Option<PathBuf> {
        if file.starts_with(&self.root) {
            return None;
        }
        let metadata = fs::metadata(file).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
            return None;
        }
        let mtime = metadata.mtime();
        let uri = file_uri(file);
        let decoded = image::ImageReader::open(file)
            .ok()
            .and_then(|reader| reader.with_guessed_format().ok())
            .and_then(|reader| reader.decode().ok());
        let Some(decoded) = decoded else {
            let marker = self
                .root
                .join("fail")
                .join(PROGRAM)
                .join(format!("{}.png", hash(&uri)));
            let pixel = image::RgbaImage::new(1, 1);
            let _ = write_png(&marker, &pixel, &uri, mtime);
            return None;
        };
        let edge = size.edge();
        let small = if decoded.width() > edge || decoded.height() > edge {
            decoded.thumbnail(edge, edge)
        } else {
            decoded
        };
        let path = self.path_for(&uri, size);
        write_png(&path, &small.to_rgba8(), &uri, mtime).ok()?;
        Some(path)
    }
}

/// `file://` and the path, with every byte outside the unreserved set and
/// `/` escaped, as the standard's examples are.
pub fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        let keep = byte.is_ascii_alphanumeric() || b"-._~/".contains(byte);
        if keep {
            uri.push(*byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The MD5 of the URI, in lowercase hexadecimal.
pub fn hash(uri: &str) -> String {
    let digest = Md5::digest(uri.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether the PNG at `path` is a thumbnail of `uri` made at `mtime`.
fn is_current(path: &Path, uri: &str, mtime: i64) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let Ok(reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    let mut stored_mtime = None;
    let mut stored_uri = None;
    for chunk in &info.uncompressed_latin1_text {
        match chunk.keyword.as_str() {
            "Thumb::MTime" => stored_mtime = chunk.text.trim().parse::<i64>().ok(),
            "Thumb::URI" => stored_uri = Some(chunk.text.clone()),
            _ => {}
        }
    }
    for chunk in &info.utf8_text {
        if let Ok(text) = chunk.get_text() {
            match chunk.keyword.as_str() {
                "Thumb::MTime" => stored_mtime = text.trim().parse::<i64>().ok(),
                "Thumb::URI" => stored_uri = Some(text),
                _ => {}
            }
        }
    }
    stored_mtime == Some(mtime) && stored_uri.is_none_or(|stored| stored == uri)
}

/// Writes a PNG with the standard's two required text chunks, through a
/// temporary file renamed into place so another program never reads half a
/// thumbnail. The folders are private to the person (0700), the file 0600.
fn write_png(path: &Path, pixels: &image::RgbaImage, uri: &str, mtime: i64) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("/"));
    fs::create_dir_all(folder)?;
    let _ = fs::set_permissions(folder, fs::Permissions::from_mode(0o700));
    let temporary = folder.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    {
        let file = fs::File::create(&temporary)?;
        let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600));
        let mut encoder = png::Encoder::new(BufWriter::new(file), pixels.width(), pixels.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let io = |error: png::EncodingError| std::io::Error::other(error.to_string());
        encoder
            .add_text_chunk("Thumb::URI".to_string(), uri.to_string())
            .map_err(io)?;
        encoder
            .add_text_chunk("Thumb::MTime".to_string(), mtime.to_string())
            .map_err(io)?;
        encoder
            .add_text_chunk("Software".to_string(), PROGRAM.to_string())
            .map_err(io)?;
        let mut writer = encoder.write_header().map_err(io)?;
        writer.write_image_data(pixels.as_raw()).map_err(io)?;
        writer.finish().map_err(io)?;
    }
    fs::rename(&temporary, path)
}

/// One installed thumbnailer, as its `.thumbnailer` file describes it
/// (`/usr/share/thumbnailers/*.thumbnailer`, the format GNOME's and folio's
/// thumbnailing read).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Thumbnailer {
    pub try_exec: Option<String>,
    /// The command, split into words; `%u`, `%i`, `%o`, `%s` are filled in.
    pub exec: Vec<String>,
    pub mime_types: Vec<String>,
}

/// How long one thumbnailer may run.
pub const THUMBNAILER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Reads a `.thumbnailer` file. `None` for one without `Exec` or types.
pub fn parse_thumbnailer(text: &str) -> Option<Thumbnailer> {
    let mut in_entry = false;
    let mut exec = None;
    let mut try_exec = None;
    let mut mime_types = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Thumbnailer Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        match line.split_once('=') {
            Some(("Exec", value)) => exec = Some(split_words(value)),
            Some(("TryExec", value)) => try_exec = Some(value.trim().to_string()),
            Some(("MimeType", value)) => {
                mime_types = value
                    .split(';')
                    .map(str::trim)
                    .filter(|mime| !mime.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            _ => {}
        }
    }
    let exec = exec.filter(|words| !words.is_empty())?;
    (!mime_types.is_empty()).then_some(Thumbnailer {
        try_exec,
        exec,
        mime_types,
    })
}

/// Desktop-entry style words: spaces separate, double quotes group.
fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for character in value.trim().chars() {
        match character {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' | '\t' if !quoted => {
                if any {
                    words.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            other => {
                word.push(other);
                any = true;
            }
        }
    }
    if any {
        words.push(word);
    }
    words
}

/// Every thumbnailer in `<data dir>/thumbnailers` of each data directory,
/// the first directory winning for a type.
pub fn load_thumbnailers(data_dirs: &[PathBuf]) -> Vec<Thumbnailer> {
    let mut found = Vec::new();
    for dir in data_dirs {
        let Ok(entries) = fs::read_dir(dir.join("thumbnailers")) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "thumbnailer"))
            .collect();
        files.sort();
        for file in files {
            if let Some(thumbnailer) = fs::read_to_string(&file)
                .ok()
                .and_then(|text| parse_thumbnailer(&text))
            {
                found.push(thumbnailer);
            }
        }
    }
    found
}

impl Thumbnailer {
    /// The words to run for `file`, writing `output` at `size` pixels.
    pub fn command(&self, file: &Path, output: &Path, size: u32) -> Vec<String> {
        self.exec
            .iter()
            .map(|word| {
                word.replace("%u", &file_uri(file))
                    .replace("%i", &file.to_string_lossy())
                    .replace("%o", &output.to_string_lossy())
                    .replace("%s", &size.to_string())
                    .replace("%%", "%")
            })
            .collect()
    }
}

impl ThumbnailCache {
    /// Makes a thumbnail with an installed thumbnailer for `mime`: runs it
    /// (its words as a list, never a shell) into a private temporary file,
    /// stops it after [`THUMBNAILER_TIMEOUT`], then stores the picture with
    /// the standard's text chunks. A thumbnailer that fails or times out
    /// leaves a fail marker. `None` when no thumbnailer takes the type.
    pub fn make_with(
        &self,
        file: &Path,
        mime: &str,
        size: ThumbSize,
        thumbnailers: &[Thumbnailer],
    ) -> Option<PathBuf> {
        if file.starts_with(&self.root) {
            return None;
        }
        let thumbnailer = thumbnailers
            .iter()
            .find(|candidate| candidate.mime_types.iter().any(|known| known == mime))?;
        let metadata = fs::metadata(file).ok()?;
        let mtime = metadata.mtime();
        let uri = file_uri(file);
        // A folder of its own (mode 0700, a name nobody can guess), so no
        // other user can put a file or a link where the thumbnailer writes.
        // It goes, with whatever is in it, when this returns.
        let folder = tempfile::Builder::new()
            .prefix("better-files-thumb-")
            .permissions(<fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700))
            .tempdir()
            .ok()?;
        let scratch = folder.path().join("thumbnail.png");
        let words = thumbnailer.command(file, &scratch, size.edge());
        let ran = run_with_timeout(&words, THUMBNAILER_TIMEOUT);
        let picture = ran.then(|| image::open(&scratch).ok()).flatten();
        drop(folder);
        let Some(picture) = picture else {
            let marker = self
                .root
                .join("fail")
                .join(PROGRAM)
                .join(format!("{}.png", hash(&uri)));
            let _ = write_png(&marker, &image::RgbaImage::new(1, 1), &uri, mtime);
            return None;
        };
        let edge = size.edge();
        let picture = if picture.width() > edge || picture.height() > edge {
            picture.thumbnail(edge, edge)
        } else {
            picture
        };
        let path = self.path_for(&uri, size);
        write_png(&path, &picture.to_rgba8(), &uri, mtime).ok()?;
        Some(path)
    }
}

/// Runs `words` and waits at most `limit`; true when it exited with success.
fn run_with_timeout(words: &[String], limit: std::time::Duration) -> bool {
    let Some((program, arguments)) = words.split_first() else {
        return false;
    };
    use std::os::unix::process::CommandExt;
    let Ok(mut child) = std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // Its own group, so a timeout ends what it started too.
        .process_group(0)
        .spawn()
    else {
        return false;
    };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() > limit => {
                crate::viewer::end_group(&mut child);
                return false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_md5_of_the_escaped_uri() {
        // MD5 test vectors from RFC 1321.
        assert_eq!(hash(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hash("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            file_uri(Path::new("/home/a/My Photo#1.png")),
            "file:///home/a/My%20Photo%231.png"
        );
        assert_eq!(
            file_uri(Path::new("/a/相片.jpg")),
            "file:///a/%E7%9B%B8%E7%89%87.jpg"
        );
    }

    fn picture(path: &Path, width: u32, height: u32) {
        image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]))
            .save(path)
            .unwrap();
    }

    #[test]
    fn a_made_thumbnail_is_found_where_every_program_looks_and_fits_its_size() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("cache/thumbnails"));
        let photo = root.path().join("photo.png");
        picture(&photo, 600, 300);
        assert_eq!(cache.lookup(&photo, ThumbSize::Large), Lookup::Missing);

        let made = cache.make(&photo, ThumbSize::Large).expect("a thumbnail");
        assert_eq!(made, cache.path_for(&file_uri(&photo), ThumbSize::Large));
        assert_eq!(
            cache.lookup(&photo, ThumbSize::Large),
            Lookup::Found(made.clone())
        );
        // The smaller size falls back to the larger one already made.
        assert_eq!(
            cache.lookup(&photo, ThumbSize::Normal),
            Lookup::Found(made.clone())
        );
        let (width, height) = image::image_dimensions(&made).unwrap();
        assert_eq!((width, height), (256, 128));
        let mode = fs::metadata(&made).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_thumbnail_older_than_its_file_is_not_used() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("thumbnails"));
        let photo = root.path().join("photo.png");
        picture(&photo, 64, 64);
        cache.make(&photo, ThumbSize::Normal).unwrap();
        // The file changes: a different mtime.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        fs::File::options()
            .write(true)
            .open(&photo)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(cache.lookup(&photo, ThumbSize::Normal), Lookup::Missing);
    }

    #[test]
    fn a_file_that_cannot_be_decoded_gets_a_fail_marker_that_any_program_left() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("thumbnails"));
        let broken = root.path().join("broken.png");
        fs::write(&broken, b"not a png").unwrap();
        assert_eq!(cache.make(&broken, ThumbSize::Normal), None);
        assert_eq!(cache.lookup(&broken, ThumbSize::Normal), Lookup::Failed);
        assert!(
            root.path()
                .join("thumbnails/fail/better-files")
                .join(format!("{}.png", hash(&file_uri(&broken))))
                .is_file()
        );
    }

    #[test]
    fn nothing_is_made_for_a_file_inside_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("thumbnails"));
        let inside = root.path().join("thumbnails/normal/x.png");
        fs::create_dir_all(inside.parent().unwrap()).unwrap();
        picture(&inside, 10, 10);
        assert_eq!(cache.make(&inside, ThumbSize::Normal), None);
    }

    #[test]
    fn a_thumbnailer_file_reads_as_its_command_and_types() {
        let thumbnailer = parse_thumbnailer(
            "[Thumbnailer Entry]\nTryExec=evince-thumbnailer\nExec=evince-thumbnailer -s %s %u %o\nMimeType=application/pdf;image/tiff;\n",
        )
        .unwrap();
        assert_eq!(thumbnailer.mime_types, ["application/pdf", "image/tiff"]);
        let words = thumbnailer.command(Path::new("/a/b c.pdf"), Path::new("/t/o.png"), 256);
        assert_eq!(
            words,
            [
                "evince-thumbnailer",
                "-s",
                "256",
                "file:///a/b%20c.pdf",
                "/t/o.png"
            ]
        );
        assert!(parse_thumbnailer("[Other]\nExec=x\nMimeType=a/b;").is_none());
        assert!(parse_thumbnailer("[Thumbnailer Entry]\nExec=x\n").is_none());
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn an_installed_thumbnailer_fills_the_cache_and_a_slow_one_is_stopped() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("thumbnails"));
        let document = root.path().join("doc.pdf");
        fs::write(&document, b"%PDF-1.4 not really").unwrap();
        // A stand-in thumbnailer that copies a ready picture to %o.
        let ready = root.path().join("ready.png");
        picture(&ready, 300, 400);
        let good = script(
            root.path(),
            "good",
            &format!("cp '{}' \"$2\"", ready.display()),
        );
        let thumbnailers = vec![Thumbnailer {
            try_exec: None,
            exec: vec![good.display().to_string(), "%u".into(), "%o".into()],
            mime_types: vec!["application/pdf".into()],
        }];
        let made = cache
            .make_with(
                &document,
                "application/pdf",
                ThumbSize::Large,
                &thumbnailers,
            )
            .expect("a thumbnail");
        assert_eq!(
            cache.lookup(&document, ThumbSize::Large),
            Lookup::Found(made.clone())
        );
        assert_eq!(image::image_dimensions(&made).unwrap(), (192, 256));
        assert_eq!(
            cache.make_with(&document, "text/plain", ThumbSize::Large, &thumbnailers),
            None
        );

        let slow_doc = root.path().join("slow.pdf");
        fs::write(&slow_doc, b"x").unwrap();
        let slow = script(root.path(), "slow", "sleep 30");
        let slow = Thumbnailer {
            try_exec: None,
            exec: vec![slow.display().to_string(), "%o".into()],
            mime_types: vec!["application/pdf".into()],
        };
        let started = std::time::Instant::now();
        assert!(!run_with_timeout(
            &slow.command(&slow_doc, Path::new("/dev/null"), 128),
            std::time::Duration::from_millis(300)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// The thumbnailer writes into a folder only this user can enter, under
    /// a name nobody can guess, and the folder is gone afterwards.
    #[test]
    fn a_thumbnailer_writes_into_a_private_folder_that_is_removed_afterwards() {
        let root = tempfile::tempdir().unwrap();
        let cache = ThumbnailCache::at(root.path().join("thumbnails"));
        let document = root.path().join("doc.pdf");
        fs::write(&document, b"%PDF").unwrap();
        let ready = root.path().join("ready.png");
        picture(&ready, 64, 64);
        let seen = root.path().join("seen");
        let spy = script(
            root.path(),
            "spy",
            &format!(
                "echo \"$1\" > '{seen}'; stat -c %a \"$(dirname \"$1\")\" >> '{seen}'; cp '{ready}' \"$1\"",
                seen = seen.display(),
                ready = ready.display()
            ),
        );
        let thumbnailers = vec![Thumbnailer {
            try_exec: None,
            exec: vec![spy.display().to_string(), "%o".into()],
            mime_types: vec!["application/pdf".into()],
        }];
        assert!(
            cache
                .make_with(
                    &document,
                    "application/pdf",
                    ThumbSize::Normal,
                    &thumbnailers
                )
                .is_some()
        );
        let seen = fs::read_to_string(&seen).unwrap();
        let mut lines = seen.lines();
        let output = PathBuf::from(lines.next().unwrap());
        assert_eq!(lines.next().unwrap(), "700");
        assert!(!output.exists());
        assert!(!output.parent().unwrap().exists(), "the folder went too");
    }
}

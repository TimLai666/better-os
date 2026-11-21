//! Where a picture or a recording is saved and what it is called
//! (spec S4, S5, R5), the way GNOME Shell 46 does it, so a person who used
//! GNOME finds them in the same folder:
//!
//! - a picture in `<Pictures>/<Screenshots>/<Screenshot from %Y-%m-%d %H-%M-%S>.png`
//!   (`_storeScreenshot`, `js/ui/screenshot.js` 2272–2306);
//! - a recording in `<Videos>/<Screencasts>/Screencast from %Y-%m-%d %H-%M-%S.webm`
//!   (`_startScreencast` 1994–2005 and the service's `_generateFilePath`);
//! - a name already taken gets `-1`, `-2`, … and nothing is overwritten;
//! - both go into the recently used files.
//!
//! The folder and file words are GNOME's own translations
//! ([`crate::words::SaveWords`]). Pictures and Videos come from the caller,
//! which reads `xdg-user-dirs` through the one reader Better OS has
//! (`files_platform::xdg`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use better_core::local_time::{LocalTime, TimeZone};

use crate::words::SaveWords;

/// GNOME's timestamp in a file name: `%Y-%m-%d %H-%M-%S`, so the name has no
/// colon.
pub fn timestamp(time: &LocalTime) -> String {
    let civil = time.civil;
    format!(
        "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        civil.year, civil.month, civil.day, civil.hour, civil.minute, civil.second
    )
}

/// The local time now, for a file name.
pub fn now() -> LocalTime {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    TimeZone::System.at(seconds)
}

/// The folder pictures go in: the Screenshots folder under Pictures.
pub fn picture_folder(pictures: &Path, words: &SaveWords) -> PathBuf {
    pictures.join(words.screenshots_folder)
}

/// The folder recordings go in: the Screencasts folder under Videos.
pub fn recording_folder(videos: &Path, words: &SaveWords) -> PathBuf {
    videos.join(words.screencasts_folder)
}

/// A picture's name without the suffix or extension.
pub fn picture_stem(time: &LocalTime, words: &SaveWords) -> String {
    words.screenshot_from.replace("{}", &timestamp(time))
}

/// A recording's name without the suffix or extension.
pub fn recording_stem(time: &LocalTime, words: &SaveWords) -> String {
    words.screencast_from.replace("{}", &timestamp(time))
}

/// The names tried in order: `stem.ext`, `stem-1.ext`, `stem-2.ext`, …
pub fn candidate(folder: &Path, stem: &str, extension: &str, index: u32) -> PathBuf {
    if index == 0 {
        folder.join(format!("{stem}.{extension}"))
    } else {
        folder.join(format!("{stem}-{index}.{extension}"))
    }
}

/// How many suffixes are tried before giving up. GNOME tries forever; a
/// folder with a thousand pictures taken in the same second is a fault, not a
/// reason to loop.
pub const MAX_SUFFIX: u32 = 1000;

/// Creates the folder (and its parents) and a new, empty file in it with the
/// first free name, never opening one that exists. Returns the open file and
/// its path.
pub fn create_unique(folder: &Path, stem: &str, extension: &str) -> io::Result<(File, PathBuf)> {
    fs::create_dir_all(folder)?;
    for index in 0..=MAX_SUFFIX {
        let path = candidate(folder, stem, extension, index);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{stem}.{extension} and {MAX_SUFFIX} suffixed names all exist"),
    ))
}

/// Saves bytes under the first free name, and returns the path. A write that
/// fails leaves no half-written file behind.
pub fn save_unique(
    folder: &Path,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let (mut file, path) = create_unique(folder, stem, extension)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(error) = written {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

/// An entry for `$XDG_DATA_HOME/recently-used.xbel` (spec S5).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentEntry {
    /// The file's `file://` URI.
    pub uri: String,
    pub mime: &'static str,
    /// When, as `YYYY-MM-DDTHH:MM:SSZ` (UTC, as GLib writes it).
    pub stamp: String,
    /// The program's name and how to open the file, as GLib's
    /// `add_application` records them.
    pub application: &'static str,
    pub exec: &'static str,
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` for a Unix time, the form GLib's bookmark file
/// writes.
pub fn xbel_stamp(unix_seconds: i64) -> String {
    let civil = TimeZone::UTC.at(unix_seconds).civil;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        civil.year, civil.month, civil.day, civil.hour, civil.minute, civil.second
    )
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn bookmark(entry: &RecentEntry) -> String {
    let stamp = escape_attribute(&entry.stamp);
    format!(
        "  <bookmark href=\"{href}\" added=\"{stamp}\" modified=\"{stamp}\" visited=\"{stamp}\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"{mime}\"/>\n        <bookmark:applications>\n          <bookmark:application name=\"{name}\" exec=\"{exec}\" modified=\"{stamp}\" count=\"1\"/>\n        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n",
        href = escape_attribute(&entry.uri),
        mime = escape_attribute(entry.mime),
        name = escape_attribute(entry.application),
        exec = escape_attribute(entry.exec),
    )
}

/// The list with `entry` added: a new document when there was none, the
/// entry before `</xbel>` otherwise. `None` when the existing text is not a
/// list this can add to safely (no closing tag, or the namespaces it needs
/// missing), so a file it does not understand is left alone.
pub fn add_recent(existing: Option<&str>, entry: &RecentEntry) -> Option<String> {
    match existing {
        None => Some(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n>\n{}</xbel>\n",
            bookmark(entry)
        )),
        Some(text) if text.trim().is_empty() => add_recent(None, entry),
        Some(text) => {
            let close = text.rfind("</xbel>")?;
            if !text.contains("xmlns:bookmark=") || !text.contains("xmlns:mime=") {
                return None;
            }
            let mut out = String::with_capacity(text.len() + 512);
            out.push_str(&text[..close]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&bookmark(entry));
            out.push_str(&text[close..]);
            Some(out)
        }
    }
}

/// Adds `entry` to the list at `path`, replacing it whole through a
/// temporary file so a reader never sees half a list.
pub fn record_recent(path: &Path, entry: &RecentEntry) -> io::Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let Some(text) = add_recent(existing.as_deref(), entry) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a bookmark list this can add to", path.display()),
        ));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("xbel.{}.tmp", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::{
        RecentEntry, add_recent, candidate, picture_folder, picture_stem, recording_folder,
        recording_stem, save_unique, timestamp, xbel_stamp,
    };
    use crate::words::{Locale, SaveWords};
    use better_core::local_time::TimeZone;
    use std::path::Path;

    #[test]
    fn the_names_are_gnomes_in_both_languages() {
        // 2026-11-21 09:05:03 UTC.
        let time = TimeZone::UTC.at(1_795_251_903);
        assert_eq!(timestamp(&time), "2026-11-21 09-05-03");

        let english = SaveWords::of(Locale::English);
        assert_eq!(
            picture_folder(Path::new("/home/a/Pictures"), &english),
            Path::new("/home/a/Pictures/Screenshots")
        );
        assert_eq!(
            picture_stem(&time, &english),
            "Screenshot from 2026-11-21 09-05-03"
        );
        assert_eq!(
            recording_folder(Path::new("/home/a/Videos"), &english),
            Path::new("/home/a/Videos/Screencasts")
        );
        assert_eq!(
            recording_stem(&time, &english),
            "Screencast from 2026-11-21 09-05-03"
        );

        let chinese = SaveWords::of(Locale::TraditionalChinese);
        assert_eq!(
            picture_folder(Path::new("/home/a/圖片"), &chinese),
            Path::new("/home/a/圖片/螢幕快照")
        );
        assert_eq!(
            picture_stem(&time, &chinese),
            "螢幕快照 2026-11-21 09-05-03"
        );
        assert_eq!(
            recording_folder(Path::new("/home/a/影片"), &chinese),
            Path::new("/home/a/影片/螢幕錄影")
        );
        // GNOME 46's zh_TW translation of the recording name is the stale
        // one with ".webm", so GNOME writes the English name: so does this.
        assert_eq!(
            recording_stem(&time, &chinese),
            "Screencast from 2026-11-21 09-05-03"
        );
    }

    #[test]
    fn a_taken_name_gets_the_next_suffix_and_nothing_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Pictures/Screenshots");
        let first = save_unique(&folder, "Screenshot from x", "png", b"one").unwrap();
        let second = save_unique(&folder, "Screenshot from x", "png", b"two").unwrap();
        let third = save_unique(&folder, "Screenshot from x", "png", b"three").unwrap();
        assert_eq!(first, candidate(&folder, "Screenshot from x", "png", 0));
        assert_eq!(second, folder.join("Screenshot from x-1.png"));
        assert_eq!(third, folder.join("Screenshot from x-2.png"));
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&third).unwrap(), b"three");
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_an_error_not_a_guess() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("Pictures");
        std::fs::write(&blocker, b"a file where the folder should be").unwrap();
        let error = save_unique(&blocker.join("Screenshots"), "s", "png", b"x").unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::NotADirectory | std::io::ErrorKind::AlreadyExists
        ));
    }

    fn entry() -> RecentEntry {
        RecentEntry {
            uri: "file:///home/a/Pictures/Screenshots/Screenshot%20from%20x.png".into(),
            mime: "image/png",
            stamp: xbel_stamp(1_795_251_903),
            application: "Better Screenshot",
            exec: "xdg-open %u",
        }
    }

    #[test]
    fn a_recent_entry_starts_a_list_or_joins_one() {
        assert_eq!(xbel_stamp(1_795_251_903), "2026-11-21T09:05:03Z");
        let new = add_recent(None, &entry()).unwrap();
        assert!(new.starts_with("<?xml"));
        assert!(
            new.contains("href=\"file:///home/a/Pictures/Screenshots/Screenshot%20from%20x.png\"")
        );
        assert!(new.contains("<mime:mime-type type=\"image/png\"/>"));
        assert!(new.trim_end().ends_with("</xbel>"));

        let existing = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\" xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\" xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  <bookmark href=\"file:///old\"/>\n</xbel>\n";
        let joined = add_recent(Some(existing), &entry()).unwrap();
        assert!(joined.contains("file:///old"));
        assert_eq!(joined.matches("<bookmark ").count(), 2);
        assert!(joined.find("file:///old").unwrap() < joined.find("Screenshot%20from").unwrap());
        assert!(joined.trim_end().ends_with("</xbel>"));
    }

    #[test]
    fn a_list_this_does_not_understand_is_left_alone() {
        assert_eq!(add_recent(Some("<xbel>"), &entry()), None);
        assert_eq!(add_recent(Some("<xbel></xbel>"), &entry()), None);
    }

    #[test]
    fn attribute_text_is_escaped() {
        let mut odd = entry();
        odd.uri = "file:///a&b\"c".into();
        let text = add_recent(None, &odd).unwrap();
        assert!(text.contains("href=\"file:///a&amp;b&quot;c\""));
    }
}

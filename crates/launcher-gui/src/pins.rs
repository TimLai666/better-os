//! What the Start page remembers between openings: the pinned apps, in the
//! order the person arranged them, and the apps they opened recently.
//!
//! Both are plain text files of desktop IDs, written through a temporary
//! file renamed into place. Nothing else is stored: no query, no
//! arguments, no file a person opened. Issue #2 forbids persisting typed
//! queries and asks that usage storage be removable; "Remove from list" on a
//! recommended app forgets it, and deleting the file forgets everything.
//!
//! - Pinned: `$XDG_CONFIG_HOME/better-os/launcher/pinned`, one desktop ID per
//!   line. It is configuration the person made. A missing file means "never
//!   arranged", which pins a starting set chosen by category
//!   ([`default_pins`]); an empty file means "unpinned everything" and stays
//!   empty.
//! - Recent: `$XDG_STATE_HOME/better-os/launcher/recent`, one
//!   `count<TAB>last-launch<TAB>desktop-id` line per app. It is state, not
//!   configuration.
//!
//! The launch history here feeds only the Recommended section. Search
//! ranking does not read it (`launcher_core::RankingOptions` keeps usage
//! weighting off), so a frequently opened app never outranks a better name
//! match.

use std::path::{Path, PathBuf};

use app_catalog_core::DesktopId;
use better_ui::fluent::list::move_item;
use launcher_core::LauncherApplication;

/// How many apps Recommended shows: three rows of two, as Windows 11 does.
pub const RECOMMENDED: usize = 6;

/// How many launches the history keeps. Older ones are forgotten.
pub const HISTORY: usize = 64;

/// The pinned apps, in order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pins {
    ids: Vec<DesktopId>,
}

impl Pins {
    pub fn new(ids: Vec<DesktopId>) -> Self {
        let mut pins = Self::default();
        for id in ids {
            pins.pin(id);
        }
        pins
    }

    pub fn ids(&self) -> &[DesktopId] {
        &self.ids
    }

    pub fn contains(&self, id: &DesktopId) -> bool {
        self.ids.contains(id)
    }

    pub fn position(&self, id: &DesktopId) -> Option<usize> {
        self.ids.iter().position(|pinned| pinned == id)
    }

    /// Pins an app at the end. Pinning a pinned app changes nothing.
    pub fn pin(&mut self, id: DesktopId) -> bool {
        if self.contains(&id) {
            return false;
        }
        self.ids.push(id);
        true
    }

    pub fn unpin(&mut self, id: &DesktopId) -> bool {
        let before = self.ids.len();
        self.ids.retain(|pinned| pinned != id);
        before != self.ids.len()
    }

    /// Moves a pinned app so it sits where `to` is now.
    pub fn move_pin(&mut self, id: &DesktopId, to: &DesktopId) -> bool {
        match (self.position(id), self.position(to)) {
            (Some(from), Some(to)) => move_item(&mut self.ids, from, to),
            _ => false,
        }
    }

    /// "Move to front".
    pub fn move_to_front(&mut self, id: &DesktopId) -> bool {
        match self.position(id) {
            Some(from) => move_item(&mut self.ids, from, 0),
            None => false,
        }
    }

    /// The file's text. One ID per line.
    pub fn serialize(&self) -> String {
        let mut text = String::new();
        for id in &self.ids {
            text.push_str(id.as_str());
            text.push('\n');
        }
        text
    }

    /// Reads the file's text. A line that is not a desktop ID is skipped
    /// rather than failing the whole list.
    pub fn parse(text: &str) -> Self {
        Self::new(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .filter_map(|line| DesktopId::new(line).ok())
                .collect(),
        )
    }
}

/// One app in the launch history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentLaunch {
    pub id: DesktopId,
    pub count: u32,
    /// Seconds since the Unix epoch.
    pub last: u64,
}

/// The launch history, most recent first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recent {
    entries: Vec<RecentLaunch>,
}

impl Recent {
    pub fn entries(&self) -> &[RecentLaunch] {
        &self.entries
    }

    /// Records a launch at `at` and moves the app to the front.
    pub fn record(&mut self, id: &DesktopId, at: u64) {
        let count = match self.entries.iter().position(|entry| &entry.id == id) {
            Some(position) => self.entries.remove(position).count.saturating_add(1),
            None => 1,
        };
        self.entries.insert(
            0,
            RecentLaunch {
                id: id.clone(),
                count,
                last: at,
            },
        );
        self.entries.truncate(HISTORY);
    }

    /// "Remove from list": forgets the app entirely.
    pub fn forget(&mut self, id: &DesktopId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| &entry.id != id);
        before != self.entries.len()
    }

    pub fn serialize(&self) -> String {
        let mut text = String::new();
        for entry in &self.entries {
            text.push_str(&format!(
                "{}\t{}\t{}\n",
                entry.count,
                entry.last,
                entry.id.as_str()
            ));
        }
        text
    }

    /// Reads the file's text, most recent first whatever order it was in.
    pub fn parse(text: &str) -> Self {
        let mut entries: Vec<RecentLaunch> = Vec::new();
        for line in text.lines() {
            let mut fields = line.splitn(3, '\t');
            let (Some(count), Some(last), Some(id)) = (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let (Ok(count), Ok(last), Ok(id)) = (
                count.trim().parse::<u32>(),
                last.trim().parse::<u64>(),
                DesktopId::new(id.trim()),
            ) else {
                continue;
            };
            if entries.iter().any(|entry| entry.id == id) {
                continue;
            }
            entries.push(RecentLaunch { id, count, last });
        }
        entries.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.id.cmp(&b.id)));
        entries.truncate(HISTORY);
        Self { entries }
    }
}

/// The categories a first Start page is pinned from, one app each, in this
/// order: the things a person opens on a new machine. Chosen by what an app
/// declares, never by its ID, so the set follows what is installed.
pub const DEFAULT_CATEGORIES: [&str; 11] = [
    "WebBrowser",
    "FileManager",
    "Email",
    "WordProcessor",
    "Spreadsheet",
    "TextEditor",
    "TerminalEmulator",
    "Calculator",
    "Calendar",
    "Photography",
    "Player",
];

/// The starting pins for someone who has never arranged them: the first app
/// (by name) declaring each of [`DEFAULT_CATEGORIES`], without repeats.
pub fn default_pins(applications: &[LauncherApplication]) -> Pins {
    let mut pins = Pins::default();
    for category in DEFAULT_CATEGORIES {
        if let Some(application) = applications.iter().find(|application| {
            !pins.contains(&application.desktop_id)
                && application
                    .categories
                    .iter()
                    .any(|declared| declared == category)
        }) {
            pins.pin(application.desktop_id.clone());
        }
    }
    pins
}

/// Where the two files live.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartStore {
    pub pinned: Option<PathBuf>,
    pub recent: Option<PathBuf>,
}

impl StartStore {
    /// The XDG locations, or none when neither `XDG_*` nor `HOME` names an
    /// absolute directory: then nothing is remembered rather than written
    /// somewhere unexpected.
    pub fn from_environment() -> Self {
        Self::from_values(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("XDG_STATE_HOME"),
            std::env::var_os("HOME"),
        )
    }

    pub fn from_values(
        config_home: Option<std::ffi::OsString>,
        state_home: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    ) -> Self {
        let absolute = |value: Option<std::ffi::OsString>| {
            value.map(PathBuf::from).filter(|path| path.is_absolute())
        };
        let home = absolute(home);
        let config = absolute(config_home).or_else(|| home.as_ref().map(|h| h.join(".config")));
        let state = absolute(state_home).or_else(|| home.as_ref().map(|h| h.join(".local/state")));
        let launcher = |base: PathBuf| base.join("better-os").join("launcher");
        Self {
            pinned: config.map(|base| launcher(base).join("pinned")),
            recent: state.map(|base| launcher(base).join("recent")),
        }
    }

    /// The pins, or `None` when the file does not exist yet (never
    /// arranged). An unreadable file reads as never arranged too, so a
    /// broken file cannot leave the Start page empty.
    /// The pins. Only a file that does not exist means "never arranged"
    /// (`Ok(None)`). A file with bytes that are not UTF-8 is read as far as
    /// its lines are desktop IDs. A file that exists and cannot be read is an
    /// error: the caller must leave it alone rather than overwrite a
    /// person's arrangement with the starting set.
    pub fn load_pins(&self) -> Result<Option<Pins>, String> {
        let Some(path) = &self.pinned else {
            return Ok(None);
        };
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(Pins::parse(&String::from_utf8_lossy(&bytes)))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    pub fn load_recent(&self) -> Recent {
        self.recent
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| Recent::parse(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// The same store with pins no longer written, for when the pins file
    /// could not be read and must not be replaced.
    pub fn without_pins(&self) -> Self {
        Self {
            pinned: None,
            recent: self.recent.clone(),
        }
    }

    pub fn save_pins(&self, pins: &Pins) -> std::io::Result<()> {
        match &self.pinned {
            Some(path) => write_atomically(path, &pins.serialize()),
            None => Ok(()),
        }
    }

    pub fn save_recent(&self, recent: &Recent) -> std::io::Result<()> {
        match &self.recent {
            Some(path) => write_atomically(path, &recent.serialize()),
            None => Ok(()),
        }
    }
}

/// Writes through a uniquely named temporary file in the same folder, synced
/// to disk before it is renamed over the old file, so a crash or a second
/// launcher writing at the same moment leaves either the old list or the new
/// one, never half of one.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Seconds since the Unix epoch, for a launch record.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> DesktopId {
        DesktopId::new(value).unwrap()
    }

    fn ids(pins: &Pins) -> Vec<&str> {
        pins.ids().iter().map(DesktopId::as_str).collect()
    }

    #[test]
    fn pinning_unpinning_and_reordering() {
        let mut pins = Pins::default();
        assert!(pins.pin(id("a.desktop")));
        assert!(pins.pin(id("b.desktop")));
        assert!(pins.pin(id("c.desktop")));
        assert!(!pins.pin(id("a.desktop")), "pinned twice is pinned once");
        assert_eq!(ids(&pins), ["a.desktop", "b.desktop", "c.desktop"]);

        // Dragging c onto a puts it where a was.
        assert!(pins.move_pin(&id("c.desktop"), &id("a.desktop")));
        assert_eq!(ids(&pins), ["c.desktop", "a.desktop", "b.desktop"]);
        assert!(pins.move_to_front(&id("b.desktop")));
        assert_eq!(ids(&pins), ["b.desktop", "c.desktop", "a.desktop"]);
        assert!(!pins.move_to_front(&id("b.desktop")));
        assert!(!pins.move_pin(&id("zzz.desktop"), &id("a.desktop")));

        assert!(pins.unpin(&id("c.desktop")));
        assert!(!pins.unpin(&id("c.desktop")));
        assert_eq!(ids(&pins), ["b.desktop", "a.desktop"]);
    }

    #[test]
    fn pins_survive_a_round_trip_and_bad_lines_are_skipped() {
        let pins = Pins::new(vec![id("b.desktop"), id("a.desktop")]);
        assert_eq!(Pins::parse(&pins.serialize()), pins);
        let parsed =
            Pins::parse("# comment\n\nb.desktop\nnot a desktop id\nb.desktop\na.desktop\n");
        assert_eq!(ids(&parsed), ["b.desktop", "a.desktop"]);
    }

    #[test]
    fn recent_launches_move_to_the_front_and_count() {
        let mut recent = Recent::default();
        recent.record(&id("a.desktop"), 10);
        recent.record(&id("b.desktop"), 20);
        recent.record(&id("a.desktop"), 30);
        let order: Vec<(&str, u32, u64)> = recent
            .entries()
            .iter()
            .map(|entry| (entry.id.as_str(), entry.count, entry.last))
            .collect();
        assert_eq!(order, [("a.desktop", 2, 30), ("b.desktop", 1, 20)]);
        assert_eq!(Recent::parse(&recent.serialize()), recent);
        assert!(recent.forget(&id("a.desktop")));
        assert!(!recent.forget(&id("a.desktop")));
        assert_eq!(recent.entries().len(), 1);
    }

    #[test]
    fn the_history_is_bounded_and_a_damaged_file_is_read_as_far_as_it_goes() {
        let mut recent = Recent::default();
        for at in 0..(HISTORY as u64 + 10) {
            recent.record(&id(&format!("app{at}.desktop")), at);
        }
        assert_eq!(recent.entries().len(), HISTORY);
        assert_eq!(
            recent.entries()[0].id.as_str(),
            format!("app{}.desktop", HISTORY + 9)
        );
        let parsed =
            Recent::parse("1\t5\told.desktop\ngarbage\n2\t9\tnew.desktop\nx\t1\tbad.desktop\n");
        let order: Vec<&str> = parsed.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(order, ["new.desktop", "old.desktop"]);
    }

    fn app(desktop_id: &str, categories: &[&str]) -> LauncherApplication {
        LauncherApplication {
            desktop_id: id(desktop_id),
            display_name: desktop_id.to_string(),
            generic_name: None,
            icon: None,
            categories: categories.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn a_first_start_page_is_pinned_by_category_not_by_name() {
        let applications = vec![
            app("browser.desktop", &["Network", "WebBrowser"]),
            app("files.desktop", &["System", "FileManager"]),
            app("other-browser.desktop", &["Network", "WebBrowser"]),
            app("terminal.desktop", &["System", "TerminalEmulator"]),
            app("game.desktop", &["Game"]),
            app("both.desktop", &["FileManager", "TerminalEmulator"]),
        ];
        let pins = default_pins(&applications);
        assert_eq!(
            ids(&pins),
            ["browser.desktop", "files.desktop", "terminal.desktop"],
            "one per category, the first by name, and an app is pinned once"
        );
        assert!(default_pins(&[]).ids().is_empty());
    }

    #[test]
    fn the_files_live_under_the_xdg_directories_and_round_trip() {
        let store = StartStore::from_values(None, None, Some("/home/someone".into()));
        assert_eq!(
            store.pinned.as_deref(),
            Some(Path::new("/home/someone/.config/better-os/launcher/pinned"))
        );
        assert_eq!(
            store.recent.as_deref(),
            Some(Path::new(
                "/home/someone/.local/state/better-os/launcher/recent"
            ))
        );
        let nowhere = StartStore::from_values(Some("relative".into()), None, None);
        assert_eq!(nowhere, StartStore::default());

        let directory = tempfile::tempdir().unwrap();
        let store = StartStore::from_values(
            Some(directory.path().join("config").into()),
            Some(directory.path().join("state").into()),
            None,
        );
        assert_eq!(store.load_pins(), Ok(None), "never arranged");
        assert_eq!(store.load_recent(), Recent::default());
        let pins = Pins::new(vec![id("a.desktop")]);
        store.save_pins(&pins).unwrap();
        assert_eq!(store.load_pins(), Ok(Some(pins)));
        store.save_pins(&Pins::default()).unwrap();
        assert_eq!(
            store.load_pins(),
            Ok(Some(Pins::default())),
            "unpinning everything is remembered as an empty Start page"
        );
        let mut recent = Recent::default();
        recent.record(&id("a.desktop"), 5);
        store.save_recent(&recent).unwrap();
        assert_eq!(store.load_recent(), recent);
        // Nothing but the two files, and no stray temporary file.
        let leftovers: Vec<_> =
            std::fs::read_dir(directory.path().join("config/better-os/launcher"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
        assert_eq!(leftovers, ["pinned"]);
    }

    #[test]
    fn a_damaged_pins_file_is_read_as_far_as_it_goes_and_an_unreadable_one_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pinned");
        std::fs::write(&path, b"a.desktop\n\xff\xfe broken\nb.desktop\n").unwrap();
        let store = StartStore {
            pinned: Some(path.clone()),
            recent: None,
        };
        let pins = store.load_pins().expect("a damaged file is still read");
        assert_eq!(ids(&pins.unwrap()), ["a.desktop", "b.desktop"]);

        // A folder where the file should be cannot be read: that is an
        // error, not "never arranged", so the defaults never replace it.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store.load_pins().is_err());
        assert_eq!(store.without_pins().pinned, None);
    }

    #[test]
    fn a_write_leaves_no_temporary_file_behind_and_replaces_the_old_list() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launcher/pinned");
        write_atomically(&path, "a.desktop\n").unwrap();
        write_atomically(&path, "b.desktop\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "b.desktop\n");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["pinned"]);
        // A stale fixed-name temporary file from an older build is not what
        // the write goes through, so it cannot be clobbered or reused.
        std::fs::write(path.with_extension("tmp"), "stale").unwrap();
        write_atomically(&path, "c.desktop\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(path.with_extension("tmp")).unwrap(),
            "stale"
        );
    }
}

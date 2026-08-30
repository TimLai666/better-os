//! A byte-faithful editor for the user's `mimeapps.list`.
//!
//! The file belongs to the user, not to Better OS. It may have been hand
//! edited, written by three other desktops, carry comments, repeat a key, use
//! CRLF line endings, or contain lines this crate has never heard of. All of
//! that is untrusted input: it is kept verbatim, and a write puts every byte
//! back except the one line the user asked to change.
//!
//! The editor understands only what it must:
//!
//! - group headers, so it can find `[Default Applications]`
//! - `key=value` lines, so it can find the one key for the selected MIME type
//!
//! Everything else is an opaque line. Nothing is reordered, reformatted,
//! deduplicated, or dropped. Where the specification says the first entry for a
//! key wins, the editor changes that first entry and leaves any later duplicate
//! exactly where the user left it, rather than "tidying up" a file it does not
//! own.

use std::collections::BTreeMap;

use app_catalog_core::{DesktopId, MimeType};
use serde::{Deserialize, Serialize};

/// The group holding the user's chosen default application per MIME type.
pub const DEFAULT_APPLICATIONS: &str = "Default Applications";
/// The group holding applications the user added for a MIME type they do not
/// declare.
pub const ADDED_ASSOCIATIONS: &str = "Added Associations";
/// The group holding applications the user removed from a MIME type they do
/// declare.
pub const REMOVED_ASSOCIATIONS: &str = "Removed Associations";

/// One line, kept with the exact terminator it was read with so a rewrite of an
/// untouched line is byte-identical.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    pub terminator: String,
}

impl Line {
    fn group_name(&self) -> Option<&str> {
        let trimmed = self.text.trim();
        let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
        Some(inner.trim())
    }

    fn key(&self) -> Option<&str> {
        let trimmed = self.text.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('[') {
            return None;
        }
        let (key, _) = self.text.split_once('=')?;
        let key = key.trim();
        (!key.is_empty()).then_some(key)
    }

    fn value(&self) -> Option<&str> {
        let _ = self.key()?;
        self.text.split_once('=').map(|(_, value)| value)
    }
}

/// What changing one default did, in enough detail to undo it exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DefaultChange {
    /// The file already said this. Nothing was touched.
    Unchanged,
    /// One existing line was rewritten. `applied` is what was written, so an
    /// undo can tell its own work from a later edit by someone else.
    Replaced {
        index: usize,
        previous: Line,
        applied: String,
    },
    /// Lines were added. `texts` is what was inserted, so a restore can verify
    /// it is removing its own work rather than a later edit by someone else.
    Inserted {
        index: usize,
        texts: Vec<String>,
        created_group: bool,
        /// The file's last line had no newline and one was added so the
        /// insertion did not concatenate onto it.
        fixed_final_newline: bool,
    },
}

/// The associations a file declares, parsed into typed values. Entries this
/// crate cannot parse are absent from this view but still present in the file.
#[derive(Clone, Debug, Default)]
pub struct MimeAssociations {
    defaults: BTreeMap<MimeType, Vec<DesktopId>>,
    added: BTreeMap<MimeType, Vec<DesktopId>>,
    removed: BTreeMap<MimeType, Vec<DesktopId>>,
}

impl MimeAssociations {
    /// The user's default for a type: the first parsable entry, which is the
    /// one the specification says wins.
    pub fn default_for(&self, mime: &MimeType) -> Option<&DesktopId> {
        self.defaults.get(mime).and_then(|ids| ids.first())
    }

    /// Every entry listed for a type, in file order.
    pub fn defaults_for(&self, mime: &MimeType) -> &[DesktopId] {
        self.defaults.get(mime).map_or(&[], Vec::as_slice)
    }

    pub fn added_for(&self, mime: &MimeType) -> &[DesktopId] {
        self.added.get(mime).map_or(&[], Vec::as_slice)
    }

    pub fn removed_for(&self, mime: &MimeType) -> &[DesktopId] {
        self.removed.get(mime).map_or(&[], Vec::as_slice)
    }

    pub fn is_added(&self, mime: &MimeType, desktop_id: &DesktopId) -> bool {
        self.added_for(mime).contains(desktop_id)
    }

    pub fn is_removed(&self, mime: &MimeType, desktop_id: &DesktopId) -> bool {
        self.removed_for(mime).contains(desktop_id)
    }

    /// Every desktop ID the file mentions anywhere, which is what "the user has
    /// used this before" is inferred from when no richer history exists.
    pub fn mentioned(&self, mime: &MimeType) -> Vec<DesktopId> {
        let mut ids = Vec::new();
        for source in [self.defaults_for(mime), self.added_for(mime)] {
            for id in source {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }
}

/// A parsed `mimeapps.list`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MimeAppsFile {
    lines: Vec<Line>,
}

impl MimeAppsFile {
    /// Splits the text into lines, keeping each terminator. Nothing is
    /// interpreted at this point.
    pub fn parse(input: &str) -> Self {
        let mut lines = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            match rest.find('\n') {
                Some(index) => {
                    let (line, tail) = rest.split_at(index + 1);
                    let body = &line[..index];
                    let (text, terminator) = match body.strip_suffix('\r') {
                        Some(text) => (text, "\r\n"),
                        None => (body, "\n"),
                    };
                    lines.push(Line {
                        text: text.to_string(),
                        terminator: terminator.to_string(),
                    });
                    rest = tail;
                }
                None => {
                    lines.push(Line {
                        text: rest.to_string(),
                        terminator: String::new(),
                    });
                    rest = "";
                }
            }
        }
        Self { lines }
    }

    /// The file's exact bytes.
    pub fn render(&self) -> String {
        let mut output = String::new();
        for line in &self.lines {
            output.push_str(&line.text);
            output.push_str(&line.terminator);
        }
        output
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The terminator to use for a line this crate adds. The file's own
    /// dominant ending wins so a CRLF file stays a CRLF file.
    fn terminator(&self) -> String {
        let crlf = self
            .lines
            .iter()
            .filter(|line| line.terminator == "\r\n")
            .count();
        let lf = self
            .lines
            .iter()
            .filter(|line| line.terminator == "\n")
            .count();
        if crlf > lf { "\r\n" } else { "\n" }.to_string()
    }

    /// The group each line belongs to. A line before the first header belongs
    /// to no group, which is where junk at the top of a hand-edited file lands.
    fn groups(&self) -> Vec<Option<String>> {
        let mut current: Option<String> = None;
        self.lines
            .iter()
            .map(|line| {
                if let Some(name) = line.group_name() {
                    current = Some(name.to_string());
                }
                current.clone()
            })
            .collect()
    }

    /// The index of the first line in `group` carrying `key`.
    fn find_key(&self, group: &str, key: &str) -> Option<usize> {
        let groups = self.groups();
        self.lines.iter().enumerate().position(|(index, line)| {
            groups[index].as_deref() == Some(group)
                && line.group_name().is_none()
                && line.key() == Some(key)
        })
    }

    /// How many lines in `group` carry `key`. More than one means the file
    /// repeats an association; the extra lines are inert but are the user's.
    pub fn count_keys(&self, group: &str, key: &str) -> usize {
        let groups = self.groups();
        self.lines
            .iter()
            .enumerate()
            .filter(|(index, line)| {
                groups[*index].as_deref() == Some(group)
                    && line.group_name().is_none()
                    && line.key() == Some(key)
            })
            .count()
    }

    /// The header index of `group` and the index just past its last line.
    fn group_span(&self, group: &str) -> Option<(usize, usize)> {
        let header = self
            .lines
            .iter()
            .position(|line| line.group_name() == Some(group))?;
        let end = self.lines[header + 1..]
            .iter()
            .position(|line| line.group_name().is_some())
            .map_or(self.lines.len(), |offset| header + 1 + offset);
        Some((header, end))
    }

    /// Where a new key belongs inside a group: after its last `key=value` line,
    /// so trailing blank lines and comments keep their place.
    fn insert_position(&self, header: usize, end: usize) -> usize {
        self.lines[header + 1..end]
            .iter()
            .rposition(|line| line.key().is_some())
            .map_or(header + 1, |offset| header + 1 + offset + 1)
    }

    /// Every association the file declares.
    pub fn associations(&self) -> MimeAssociations {
        let mut associations = MimeAssociations::default();
        let groups = self.groups();
        for (index, line) in self.lines.iter().enumerate() {
            if line.group_name().is_some() {
                continue;
            }
            let target = match groups[index].as_deref() {
                Some(DEFAULT_APPLICATIONS) => &mut associations.defaults,
                Some(ADDED_ASSOCIATIONS) => &mut associations.added,
                Some(REMOVED_ASSOCIATIONS) => &mut associations.removed,
                _ => continue,
            };
            let (Some(key), Some(value)) = (line.key(), line.value()) else {
                continue;
            };
            let Some(mime) = MimeType::parse(key) else {
                continue;
            };
            let ids: Vec<DesktopId> = value
                .split(';')
                .filter_map(|entry| DesktopId::new(entry.trim()).ok())
                .collect();
            // A repeated key is not merged: the first one wins, exactly as the
            // specification says, and the later one keeps its own line.
            target.entry(mime).or_insert(ids);
        }
        associations
    }

    /// Points the default for `mime` at `desktop_id`, changing at most one
    /// existing line.
    pub fn set_default(&mut self, mime: &MimeType, desktop_id: &DesktopId) -> DefaultChange {
        let desired = format!("{}={}", mime.as_str(), desktop_id.as_str());
        if let Some(index) = self.find_key(DEFAULT_APPLICATIONS, mime.as_str()) {
            if self.lines[index].text == desired {
                return DefaultChange::Unchanged;
            }
            let previous = self.lines[index].clone();
            self.lines[index].text = desired.clone();
            return DefaultChange::Replaced {
                index,
                previous,
                applied: desired,
            };
        }

        let terminator = self.terminator();
        if let Some((header, end)) = self.group_span(DEFAULT_APPLICATIONS) {
            let index = self.insert_position(header, end);
            let fixed_final_newline = self.ensure_final_newline(index, &terminator);
            self.lines.insert(
                index,
                Line {
                    text: desired.clone(),
                    terminator,
                },
            );
            return DefaultChange::Inserted {
                index,
                texts: vec![desired],
                created_group: false,
                fixed_final_newline,
            };
        }

        let index = self.lines.len();
        let fixed_final_newline = self.ensure_final_newline(index, &terminator);
        let mut texts = Vec::new();
        if self
            .lines
            .last()
            .is_some_and(|line| !line.text.trim().is_empty())
        {
            texts.push(String::new());
        }
        texts.push(format!("[{DEFAULT_APPLICATIONS}]"));
        texts.push(desired);
        for text in &texts {
            self.lines.push(Line {
                text: text.clone(),
                terminator: terminator.clone(),
            });
        }
        DefaultChange::Inserted {
            index,
            texts,
            created_group: true,
            fixed_final_newline,
        }
    }

    /// Gives the line before `index` a terminator when it has none, so an
    /// insertion does not run onto the end of it. Reports whether it had to.
    fn ensure_final_newline(&mut self, index: usize, terminator: &str) -> bool {
        if index == 0 {
            return false;
        }
        let previous = &mut self.lines[index - 1];
        if previous.terminator.is_empty() {
            previous.terminator = terminator.to_string();
            true
        } else {
            false
        }
    }

    /// Undoes a [`DefaultChange`]. Returns whether the file was restored to its
    /// previous shape; `false` means the change no longer matches the file and
    /// nothing was touched.
    pub fn undo(&mut self, change: &DefaultChange) -> bool {
        match change {
            DefaultChange::Unchanged => true,
            DefaultChange::Replaced {
                index,
                previous,
                applied,
            } => {
                if self.lines.get(*index).map(|line| &line.text) != Some(applied) {
                    return false;
                }
                self.lines[*index] = previous.clone();
                true
            }
            DefaultChange::Inserted {
                index,
                texts,
                fixed_final_newline,
                ..
            } => {
                let end = index + texts.len();
                if end > self.lines.len() {
                    return false;
                }
                let matches = self.lines[*index..end]
                    .iter()
                    .zip(texts)
                    .all(|(line, text)| &line.text == text);
                if !matches {
                    return false;
                }
                self.lines.drain(*index..end);
                if *fixed_final_newline && *index > 0 {
                    if let Some(line) = self.lines.get_mut(index - 1) {
                        line.terminator.clear();
                    }
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mime(value: &str) -> MimeType {
        MimeType::parse(value).expect("valid mime type")
    }

    fn id(value: &str) -> DesktopId {
        DesktopId::new(value).expect("valid desktop id")
    }

    const HAND_EDITED: &str = "# written by hand, do not tidy\n\
         \n\
         [Added Associations]\n\
         text/plain=vim.desktop;nano.desktop;\n\
         \n\
         [Default Applications]\n\
         image/png=eog.desktop\n\
         text/html=firefox.desktop\n\
         \n\
         # a stray comment inside the group\n\
         [Removed Associations]\n\
         application/pdf=evince.desktop;\n";

    #[test]
    fn parsing_and_rendering_round_trips_byte_for_byte() {
        for input in [
            HAND_EDITED,
            "",
            "no trailing newline",
            "[Default Applications]\r\ntext/plain=vim.desktop\r\n",
            "\n\n\n",
            "junk before any group\n[Default Applications]\n",
        ] {
            assert_eq!(MimeAppsFile::parse(input).render(), input);
        }
    }

    #[test]
    fn associations_are_read_per_group() {
        let associations = MimeAppsFile::parse(HAND_EDITED).associations();
        assert_eq!(
            associations.default_for(&mime("image/png")),
            Some(&id("eog.desktop"))
        );
        assert_eq!(
            associations.added_for(&mime("text/plain")),
            &[id("vim.desktop"), id("nano.desktop")]
        );
        assert!(associations.is_removed(&mime("application/pdf"), &id("evince.desktop")));
        assert!(associations.default_for(&mime("text/plain")).is_none());
    }

    #[test]
    fn a_repeated_key_keeps_the_first_value() {
        let file = MimeAppsFile::parse(
            "[Default Applications]\ntext/plain=first.desktop\ntext/plain=second.desktop\n",
        );
        assert_eq!(
            file.associations().default_for(&mime("text/plain")),
            Some(&id("first.desktop"))
        );
    }

    #[test]
    fn changing_one_default_changes_exactly_one_line() {
        let mut file = MimeAppsFile::parse(HAND_EDITED);
        let change = file.set_default(&mime("image/png"), &id("gimp.desktop"));
        assert!(matches!(change, DefaultChange::Replaced { .. }));
        let before: Vec<&str> = HAND_EDITED.lines().collect();
        let rendered = file.render();
        let after: Vec<&str> = rendered.lines().collect();
        assert_eq!(before.len(), after.len());
        let differing: Vec<usize> = before
            .iter()
            .zip(&after)
            .enumerate()
            .filter(|(_, (left, right))| left != right)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(differing.len(), 1);
        assert_eq!(after[differing[0]], "image/png=gimp.desktop");
    }

    #[test]
    fn undoing_a_replacement_restores_the_exact_bytes() {
        let mut file = MimeAppsFile::parse(HAND_EDITED);
        let change = file.set_default(&mime("image/png"), &id("gimp.desktop"));
        assert!(file.undo(&change));
        assert_eq!(file.render(), HAND_EDITED);
    }

    #[test]
    fn a_new_type_is_inserted_after_the_last_key_in_its_group() {
        let mut file = MimeAppsFile::parse(HAND_EDITED);
        let change = file.set_default(&mime("text/x-rust"), &id("zed.desktop"));
        assert!(matches!(
            change,
            DefaultChange::Inserted {
                created_group: false,
                ..
            }
        ));
        let rendered = file.render();
        assert!(rendered.contains("text/html=firefox.desktop\ntext/x-rust=zed.desktop\n"));
        // The stray comment and the following group keep their place.
        assert!(rendered.contains("# a stray comment inside the group\n[Removed Associations]"));
        let mut file = MimeAppsFile::parse(&rendered);
        assert!(file.undo(&change));
        assert_eq!(file.render(), HAND_EDITED);
    }

    #[test]
    fn a_missing_group_is_created_at_the_end() {
        let original = "[Added Associations]\ntext/plain=vim.desktop\n";
        let mut file = MimeAppsFile::parse(original);
        let change = file.set_default(&mime("text/plain"), &id("nano.desktop"));
        assert!(matches!(
            change,
            DefaultChange::Inserted {
                created_group: true,
                ..
            }
        ));
        assert_eq!(
            file.render(),
            "[Added Associations]\ntext/plain=vim.desktop\n\n[Default Applications]\ntext/plain=nano.desktop\n"
        );
        assert!(file.undo(&change));
        assert_eq!(file.render(), original);
    }

    #[test]
    fn an_empty_file_gains_only_the_group_and_the_key() {
        let mut file = MimeAppsFile::parse("");
        let change = file.set_default(&mime("text/plain"), &id("nano.desktop"));
        assert_eq!(
            file.render(),
            "[Default Applications]\ntext/plain=nano.desktop\n"
        );
        assert!(file.undo(&change));
        assert_eq!(file.render(), "");
    }

    #[test]
    fn a_file_without_a_final_newline_is_repaired_and_the_repair_is_undone() {
        let original = "[Default Applications]\nimage/png=eog.desktop";
        let mut file = MimeAppsFile::parse(original);
        let change = file.set_default(&mime("text/plain"), &id("nano.desktop"));
        assert_eq!(
            file.render(),
            "[Default Applications]\nimage/png=eog.desktop\ntext/plain=nano.desktop\n"
        );
        assert!(file.undo(&change));
        assert_eq!(file.render(), original);
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let original = "[Default Applications]\r\nimage/png=eog.desktop\r\n";
        let mut file = MimeAppsFile::parse(original);
        file.set_default(&mime("text/plain"), &id("nano.desktop"));
        assert_eq!(
            file.render(),
            "[Default Applications]\r\nimage/png=eog.desktop\r\ntext/plain=nano.desktop\r\n"
        );
    }

    #[test]
    fn setting_the_value_that_is_already_there_changes_nothing() {
        let original = "[Default Applications]\nimage/png=eog.desktop\n";
        let mut file = MimeAppsFile::parse(original);
        assert_eq!(
            file.set_default(&mime("image/png"), &id("eog.desktop")),
            DefaultChange::Unchanged
        );
        assert_eq!(file.render(), original);
    }

    #[test]
    fn a_duplicate_key_is_left_where_the_user_left_it() {
        let mut file = MimeAppsFile::parse(
            "[Default Applications]\ntext/plain=first.desktop\ntext/plain=second.desktop\n",
        );
        file.set_default(&mime("text/plain"), &id("third.desktop"));
        assert_eq!(
            file.render(),
            "[Default Applications]\ntext/plain=third.desktop\ntext/plain=second.desktop\n"
        );
    }

    #[test]
    fn junk_and_unknown_groups_survive_a_write() {
        let original = "]not a header[\n\
             [Some Other Desktop]\n\
             whatever=keep me\n\
             [Default Applications]\n\
             image/png = eog.desktop \n\
             = no key\n\
             text/plain\n";
        let mut file = MimeAppsFile::parse(original);
        file.set_default(&mime("image/png"), &id("gimp.desktop"));
        let rendered = file.render();
        for kept in [
            "]not a header[",
            "whatever=keep me",
            "= no key",
            "text/plain",
        ] {
            assert!(rendered.contains(kept), "lost {kept}");
        }
        assert!(rendered.contains("image/png=gimp.desktop"));
        assert!(!rendered.contains("image/png = eog.desktop"));
    }

    #[test]
    fn a_key_in_another_group_is_not_mistaken_for_a_default() {
        let mut file = MimeAppsFile::parse(
            "[Added Associations]\nimage/png=eog.desktop\n\n[Default Applications]\ntext/html=firefox.desktop\n",
        );
        file.set_default(&mime("image/png"), &id("gimp.desktop"));
        assert_eq!(
            file.render(),
            "[Added Associations]\nimage/png=eog.desktop\n\n[Default Applications]\ntext/html=firefox.desktop\nimage/png=gimp.desktop\n"
        );
    }

    #[test]
    fn an_undo_that_no_longer_matches_the_file_refuses_to_guess() {
        let mut file = MimeAppsFile::parse("[Default Applications]\n");
        let change = file.set_default(&mime("text/plain"), &id("nano.desktop"));
        // Someone else edited the same line in between.
        file.set_default(&mime("text/plain"), &id("vim.desktop"));
        assert!(!file.undo(&change));
        assert!(file.render().contains("text/plain=vim.desktop"));
    }
}

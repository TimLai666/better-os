//! Hidden-entry rules.
//!
//! Issue #6 requires hidden status to come from platform rules rather than
//! filename inspection alone, and requires revealing hidden entries not to
//! trigger a blocking full reload. Both fall out of the same decision: hidden
//! is computed once, while the directory is being read, and stored on every
//! entry. The preference is then a filter over a list already in memory, so
//! `Ctrl+H` costs a re-filter rather than a second `readdir`.
//!
//! The rules implemented are the freedesktop ones a Linux file manager is
//! expected to honor: a leading dot, and membership in the directory's
//! `.hidden` file. Reading that file is I/O, so `files-platform` supplies its
//! contents and this module owns what the contents mean.

use std::collections::HashSet;

use crate::entry::{HiddenReason, HiddenState};

/// The rules that apply inside one directory.
///
/// Built per directory because `.hidden` is per directory. An empty ruleset
/// still hides dotfiles, which is the behavior for a directory with no
/// `.hidden` file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HiddenRules {
    listed: HashSet<String>,
    hide_backup_files: bool,
}

impl HiddenRules {
    /// Dotfiles only: the rules for a directory with no `.hidden` file.
    pub fn dotfiles_only() -> Self {
        Self::default()
    }

    /// Parses a `.hidden` file: one filename per line.
    ///
    /// Blank lines are skipped and no line is treated as a pattern. The
    /// freedesktop convention is literal names, and treating a name containing
    /// `*` as a glob would hide files whose names legitimately contain one.
    pub fn from_hidden_file(contents: &str) -> Self {
        let listed = contents
            .lines()
            .map(|line| line.trim_end_matches(['\r', '\n']))
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect();
        Self {
            listed,
            hide_backup_files: false,
        }
    }

    /// Also hide names ending in `~`. Off by default because a user who has
    /// not asked for it is surprised when an editor's backup vanishes.
    pub fn hiding_backup_files(mut self, hide: bool) -> Self {
        self.hide_backup_files = hide;
        self
    }

    /// The names this directory's `.hidden` file lists.
    pub fn listed_names(&self) -> impl Iterator<Item = &str> {
        self.listed.iter().map(String::as_str)
    }

    /// Decides one name.
    ///
    /// The dot rule is checked first so the reason a view reports is the one a
    /// user can act on: a dotfile listed in `.hidden` is still, first, a
    /// dotfile.
    pub fn classify(&self, name: &str) -> HiddenState {
        if name.starts_with('.') {
            return HiddenState::Hidden(HiddenReason::Dotfile);
        }
        if self.listed.contains(name) {
            return HiddenState::Hidden(HiddenReason::DirectoryHiddenFile);
        }
        if self.hide_backup_files && name.ends_with('~') {
            return HiddenState::Hidden(HiddenReason::BackupFile);
        }
        HiddenState::Visible
    }
}

/// The user's preference about hidden entries.
///
/// It is a filter, not a listing parameter. A listing always reports every
/// entry with its hidden state attached; this decides which of them a view
/// draws.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HiddenPreference {
    pub show_hidden: bool,
}

impl HiddenPreference {
    pub fn showing_hidden() -> Self {
        Self { show_hidden: true }
    }

    pub fn accepts(self, state: HiddenState) -> bool {
        self.show_hidden || !state.is_hidden()
    }

    /// Flips the preference and returns the new value, which is what the
    /// `Ctrl+H` action does.
    pub fn toggle(&mut self) -> bool {
        self.show_hidden = !self.show_hidden;
        self.show_hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_dot_hides_an_entry() {
        let rules = HiddenRules::dotfiles_only();
        assert_eq!(
            rules.classify(".bashrc"),
            HiddenState::Hidden(HiddenReason::Dotfile)
        );
        assert_eq!(rules.classify("notes.txt"), HiddenState::Visible);
    }

    #[test]
    fn a_hidden_file_hides_a_normally_named_entry_and_says_so() {
        let rules = HiddenRules::from_hidden_file("build\n\nnode_modules\n");
        assert_eq!(
            rules.classify("build"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
        assert_eq!(
            rules.classify("node_modules"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
        assert_eq!(rules.classify("src"), HiddenState::Visible);
    }

    #[test]
    fn a_hidden_file_entry_is_a_literal_name_not_a_pattern() {
        let rules = HiddenRules::from_hidden_file("*.log\n");
        assert_eq!(rules.classify("server.log"), HiddenState::Visible);
        assert_eq!(
            rules.classify("*.log"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
    }

    #[test]
    fn backup_files_are_hidden_only_when_asked_for() {
        let default_rules = HiddenRules::dotfiles_only();
        assert_eq!(default_rules.classify("draft.txt~"), HiddenState::Visible);
        let opted_in = HiddenRules::dotfiles_only().hiding_backup_files(true);
        assert_eq!(
            opted_in.classify("draft.txt~"),
            HiddenState::Hidden(HiddenReason::BackupFile)
        );
    }

    #[test]
    fn the_preference_decides_what_is_shown_without_changing_the_listing() {
        let mut preference = HiddenPreference::default();
        let hidden = HiddenState::Hidden(HiddenReason::Dotfile);
        assert!(!preference.accepts(hidden));
        assert!(preference.accepts(HiddenState::Visible));
        assert!(preference.toggle());
        assert!(preference.accepts(hidden));
    }
}

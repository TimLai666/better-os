//! Reading a directory's `.hidden` file.
//!
//! `files-core` owns what the rules mean. This is the one place that reads the
//! file, and it is deliberately forgiving: a missing, unreadable, or
//! oversized `.hidden` file leaves the dotfile rule in place rather than
//! failing the listing. A folder does not become unlistable because a stray
//! file in it could not be read.

use std::fs;
use std::path::Path;

use files_core::hidden::HiddenRules;

/// The largest `.hidden` file that will be read.
///
/// The file is a short list of names. A cap means a directory containing a
/// multi-gigabyte file that happens to be called `.hidden` cannot stall a
/// listing or exhaust memory.
pub const MAX_HIDDEN_FILE_BYTES: u64 = 64 * 1024;

/// Reads the rules for one directory.
pub fn read_hidden_rules(directory: &Path) -> HiddenRules {
    let path = directory.join(".hidden");
    let Ok(metadata) = fs::metadata(&path) else {
        return HiddenRules::dotfiles_only();
    };
    if !metadata.is_file() || metadata.len() > MAX_HIDDEN_FILE_BYTES {
        return HiddenRules::dotfiles_only();
    }
    match fs::read(&path) {
        // Lossy, so a `.hidden` file written in another encoding still hides
        // the names in it that are plain ASCII instead of hiding nothing.
        Ok(bytes) => HiddenRules::from_hidden_file(&String::from_utf8_lossy(&bytes)),
        Err(_) => HiddenRules::dotfiles_only(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::entry::{HiddenReason, HiddenState};

    #[test]
    fn a_directory_without_a_hidden_file_still_hides_dotfiles() {
        let root = tempfile::tempdir().unwrap();
        let rules = read_hidden_rules(root.path());
        assert_eq!(
            rules.classify(".ssh"),
            HiddenState::Hidden(HiddenReason::Dotfile)
        );
        assert_eq!(rules.classify("Documents"), HiddenState::Visible);
    }

    #[test]
    fn names_listed_in_the_hidden_file_are_hidden() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".hidden"), "target\nnode_modules\n").unwrap();
        let rules = read_hidden_rules(root.path());
        assert_eq!(
            rules.classify("target"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
        assert_eq!(rules.classify("src"), HiddenState::Visible);
    }

    #[test]
    fn an_oversized_hidden_file_is_ignored_rather_than_read() {
        let root = tempfile::tempdir().unwrap();
        let bloated = "x".repeat((MAX_HIDDEN_FILE_BYTES + 1) as usize);
        fs::write(root.path().join(".hidden"), &bloated).unwrap();
        let rules = read_hidden_rules(root.path());
        assert_eq!(rules.listed_names().count(), 0);
    }

    #[test]
    fn a_hidden_directory_named_dot_hidden_is_not_treated_as_a_list() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".hidden")).unwrap();
        let rules = read_hidden_rules(root.path());
        assert_eq!(rules.listed_names().count(), 0);
    }

    #[test]
    fn invalid_utf8_does_not_discard_the_readable_names() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".hidden"), b"good\n\xff\xfe\nalso-good\n").unwrap();
        let rules = read_hidden_rules(root.path());
        assert_eq!(
            rules.classify("good"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
        assert_eq!(
            rules.classify("also-good"),
            HiddenState::Hidden(HiddenReason::DirectoryHiddenFile)
        );
    }
}

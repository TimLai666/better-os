//! Whether a custom keyboard shortcut collides with a keybinding the desktop
//! already has.
//!
//! This is a **best-effort** check and says so in its own vocabulary. The only
//! keybindings any Better OS component can read are the ones recorded in the
//! user's own dconf database, and a database records what the user changed —
//! GNOME's compiled-in defaults are not in it and are not exposed anywhere else
//! to read. So there are exactly three answers, and "no conflict found" is
//! deliberately not one of them being spelled "no conflict":
//!
//! - [`ShortcutCheck::Conflicts`] — a recorded binding is the same combination.
//! - [`ShortcutCheck::NoneRecorded`] — nothing recorded matches, which is not
//!   the same claim as "nothing on this desktop uses it".
//! - [`ShortcutCheck::Unknown`] — the bindings could not be read at all.
//!
//! A recorded binding that does not parse into `better-actions`' closed key
//! table is skipped rather than guessed at, and counted, so the screen can say
//! how much of the database it could not read in this vocabulary.

use better_actions::KeyboardShortcut;

/// What checking one shortcut against the recorded keybindings found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShortcutCheck {
    /// A recorded keybinding is the same combination. `key` is the dconf key
    /// path it was read from, which is the only honest name for it.
    Conflicts { key: String },
    /// Nothing recorded uses this combination.
    NoneRecorded,
    /// The recorded keybindings could not be read.
    Unknown { reason: String },
}

impl ShortcutCheck {
    pub fn conflicts(&self) -> bool {
        matches!(self, Self::Conflicts { .. })
    }
}

/// The keybindings that could be read, in the vocabulary this crate can compare.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KnownShortcuts {
    entries: Vec<(String, KeyboardShortcut)>,
    /// Recorded bindings whose spelling is outside the fixed key table, so they
    /// could not be compared. Counted rather than dropped silently.
    unreadable: usize,
    unknown: Option<String>,
}

impl KnownShortcuts {
    /// Builds the table from `(dconf key path, binding spelling)` pairs, which
    /// is the shape `touchpad-platform`'s reader produces.
    pub fn from_bindings<I, K, B>(bindings: I) -> Self
    where
        I: IntoIterator<Item = (K, B)>,
        K: Into<String>,
        B: AsRef<str>,
    {
        let mut entries = Vec::new();
        let mut unreadable = 0;
        for (key, binding) in bindings {
            match KeyboardShortcut::parse(binding.as_ref()) {
                Ok(shortcut) => entries.push((key.into(), shortcut)),
                Err(_) => unreadable += 1,
            }
        }
        Self {
            entries,
            unreadable,
            unknown: None,
        }
    }

    /// Nothing could be read, and why.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            unreadable: 0,
            unknown: Some(reason.into()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many recorded bindings could not be compared.
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    pub fn check(&self, shortcut: &KeyboardShortcut) -> ShortcutCheck {
        if let Some(reason) = &self.unknown {
            return ShortcutCheck::Unknown {
                reason: reason.clone(),
            };
        }
        match self
            .entries
            .iter()
            .find(|(_, known)| known == shortcut)
            .map(|(key, _)| key.clone())
        {
            Some(key) => ShortcutCheck::Conflicts { key },
            None => ShortcutCheck::NoneRecorded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(text: &str) -> KeyboardShortcut {
        KeyboardShortcut::parse(text).unwrap()
    }

    #[test]
    fn a_recorded_binding_with_the_same_combination_is_reported_with_the_key_it_came_from() {
        let known = KnownShortcuts::from_bindings([
            (
                "/org/gnome/settings-daemon/plugins/media-keys/www",
                "<Super>w",
            ),
            ("/org/gnome/desktop/wm/keybindings/close", "<Alt>F4"),
        ]);
        assert_eq!(
            known.check(&shortcut("<Super>w")),
            ShortcutCheck::Conflicts {
                key: "/org/gnome/settings-daemon/plugins/media-keys/www".to_string()
            }
        );
        assert_eq!(
            known.check(&shortcut("<Super>q")),
            ShortcutCheck::NoneRecorded
        );
    }

    #[test]
    fn the_modifier_order_a_binding_was_written_in_does_not_change_the_answer() {
        let known = KnownShortcuts::from_bindings([("k", "<Shift><Super>d")]);
        assert!(known.check(&shortcut("<Super><Shift>d")).conflicts());
        // A different modifier set is a different shortcut, not a near miss.
        assert!(!known.check(&shortcut("<Super>d")).conflicts());
    }

    #[test]
    fn a_binding_outside_the_fixed_key_table_is_counted_rather_than_guessed_at() {
        let known = KnownShortcuts::from_bindings([
            ("a", "<Super>w"),
            ("b", "XF86AudioPlay"),
            ("c", "<Super>KP_Add"),
            ("d", ""),
        ]);
        assert_eq!(known.len(), 1);
        assert_eq!(known.unreadable(), 3);
        assert!(known.check(&shortcut("<Super>w")).conflicts());
    }

    #[test]
    fn bindings_that_could_not_be_read_at_all_answer_unknown_rather_than_clear() {
        let known = KnownShortcuts::unavailable("gnome.database_unreadable");
        assert_eq!(
            known.check(&shortcut("<Super>w")),
            ShortcutCheck::Unknown {
                reason: "gnome.database_unreadable".to_string()
            }
        );
        assert!(!known.check(&shortcut("<Super>w")).conflicts());
        // An empty but readable table is a different answer from an unreadable
        // one, and the two must not collapse into each other.
        assert_eq!(
            KnownShortcuts::from_bindings(Vec::<(String, String)>::new())
                .check(&shortcut("<Super>w")),
            ShortcutCheck::NoneRecorded
        );
    }
}

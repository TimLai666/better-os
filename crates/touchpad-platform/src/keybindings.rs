//! Reading the keyboard shortcuts the desktop already has.
//!
//! A custom gesture action can be a keyboard shortcut, and a shortcut that is
//! already spoken for is worth saying so before it is bound. This reads that
//! from the same place `GnomeBackend` reads a touchpad setting: the user's own
//! dconf database, through `defaults-platform`'s GVDB parser. There is one
//! dconf reader in Better OS and this is not a second one.
//!
//! **What this can and cannot see, exactly.** A dconf database holds the keys
//! the user has changed. GNOME's own defaults — `<Super>` for the overview,
//! `<Alt><Tab>` for the window switcher — are compiled into the shell and its
//! schemas and appear in no database, and the shell exposes nothing to ask. So
//! a shortcut this finds nothing for is *not* a shortcut nothing uses, and the
//! wording that reaches the screen says so rather than reporting a clear
//! result. That limit is the reason this returns an explicit "nothing recorded"
//! rather than "no conflict".
//!
//! Three key prefixes are read, and all three hold `as` lists of shortcut
//! spellings: the window manager's keybindings, the settings daemon's media-key
//! bindings, and the shell's own. An application's own accelerators are not in
//! dconf at all and are outside what any of this can promise.

use std::path::Path;

use defaults_platform::gvdb::{GVariantValue, GvdbDatabase};

/// The dconf trees that hold keyboard shortcuts.
pub const KEYBINDING_PREFIXES: &[&str] = &[
    "/org/gnome/desktop/wm/keybindings/",
    "/org/gnome/settings-daemon/plugins/media-keys/",
    "/org/gnome/shell/keybindings/",
];

/// One recorded binding: the dconf key it was read from, and its spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownBinding {
    pub key: String,
    pub binding: String,
}

/// What reading the recorded keybindings produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeybindingReading {
    /// The database was read. The list holds every recorded binding, which may
    /// legitimately be empty on a session whose shortcuts were never changed.
    Recorded(Vec<KnownBinding>),
    /// The database could not be read, and why. Distinct from an empty list.
    Unknown { reason: String, detail: String },
}

impl KeybindingReading {
    pub fn bindings(&self) -> &[KnownBinding] {
        match self {
            Self::Recorded(bindings) => bindings,
            Self::Unknown { .. } => &[],
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Self::Recorded(_))
    }
}

/// Reads every recorded keybinding from a dconf database file.
///
/// A database that is not there is [`KeybindingReading::Recorded`] with nothing
/// in it: the user has changed no shortcut, which is a definite answer. A
/// database that exists and will not parse, or cannot be opened, is
/// [`KeybindingReading::Unknown`] — the difference matters, because only one of
/// the two lets the screen say anything about a collision.
pub fn read(database: &Path) -> KeybindingReading {
    match std::fs::read(database) {
        Ok(bytes) => match GvdbDatabase::parse(&bytes) {
            Ok(database) => KeybindingReading::Recorded(collect(&database)),
            Err(error) => KeybindingReading::Unknown {
                reason: "gnome.database_unreadable".to_string(),
                detail: error.to_string(),
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            KeybindingReading::Recorded(Vec::new())
        }
        Err(error) => KeybindingReading::Unknown {
            reason: "gnome.database_not_readable".to_string(),
            detail: error.to_string(),
        },
    }
}

/// Every shortcut spelling under the keybinding prefixes, in key order.
///
/// A key holding something other than a list of strings is skipped rather than
/// coerced: a keybinding is an `as` in every schema that has one, and a key of
/// another type under the same prefix is not a binding this can read.
pub fn collect(database: &GvdbDatabase) -> Vec<KnownBinding> {
    let mut bindings = Vec::new();
    for key in database.keys() {
        if !KEYBINDING_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            continue;
        }
        if let Some(GVariantValue::TextList(spellings)) = database.get(key) {
            for spelling in spellings {
                bindings.push(KnownBinding {
                    key: key.to_string(),
                    binding: spelling.clone(),
                });
            }
        }
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `defaults-platform`'s fixture is a database `dconf` itself wrote, and it
    /// is the only real one in the workspace. Reading it from here rather than
    /// copying it keeps one file to regenerate if it is ever rebuilt.
    const USER_DB: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../defaults-platform/tests/fixtures/dconf/user"
    ));

    fn database() -> GvdbDatabase {
        GvdbDatabase::parse(USER_DB).expect("the fixture is a GVDB database")
    }

    #[test]
    fn every_recorded_binding_under_a_keybinding_prefix_is_read_with_its_key() {
        let bindings = collect(&database());
        assert!(bindings.contains(&KnownBinding {
            key: "/org/gnome/settings-daemon/plugins/media-keys/www".to_string(),
            binding: "<Super>b".to_string(),
        }));
        assert!(bindings.contains(&KnownBinding {
            key: "/org/gnome/settings-daemon/plugins/media-keys/www".to_string(),
            binding: "<Super>w".to_string(),
        }));
        assert!(bindings.contains(&KnownBinding {
            key: "/org/gnome/settings-daemon/plugins/media-keys/home".to_string(),
            binding: "<Super>e".to_string(),
        }));
    }

    #[test]
    fn a_key_outside_the_keybinding_trees_is_not_read_as_a_shortcut() {
        let bindings = collect(&database());
        assert!(
            bindings.iter().all(|binding| KEYBINDING_PREFIXES
                .iter()
                .any(|prefix| binding.key.starts_with(prefix))),
            "a key outside the keybinding trees was read as a binding"
        );
        // The fixture holds a boolean and a string outside those trees, and an
        // empty binding list inside one. None of them produces a binding.
        assert!(
            !bindings
                .iter()
                .any(|binding| binding.key.ends_with("/close")),
            "an empty binding list produced a binding"
        );
    }

    #[test]
    fn a_database_that_is_not_there_is_nothing_recorded_rather_than_unknown() {
        let reading = read(Path::new("/nonexistent/dconf/user"));
        assert_eq!(reading, KeybindingReading::Recorded(Vec::new()));
        assert!(reading.is_known());
    }

    #[test]
    fn a_file_that_is_not_a_database_is_unknown_rather_than_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("user");
        std::fs::write(&path, b"this is a text file, not a database").unwrap();
        let reading = read(&path);
        assert!(!reading.is_known());
        assert!(reading.bindings().is_empty());
        assert!(matches!(
            reading,
            KeybindingReading::Unknown { reason, .. } if reason == "gnome.database_unreadable"
        ));
    }

    #[test]
    fn the_real_database_reads_the_same_bindings_from_disk_as_from_memory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("user");
        std::fs::write(&path, USER_DB).unwrap();
        assert_eq!(
            read(&path),
            KeybindingReading::Recorded(collect(&database()))
        );
    }
}

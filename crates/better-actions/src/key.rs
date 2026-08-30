//! The custom keyboard shortcut, and the reason it is the only action in the
//! catalog that carries user text.
//!
//! A shortcut is a set of modifiers and one key, and both halves are closed
//! sets. [`Key`] wraps a `&'static str` borrowed from [`Key::ALL`]; its field is
//! private and [`Key::parse`] is the only way to obtain one, so a `Key` that is
//! not in the table cannot be constructed, deserialized, or forged. That is
//! what makes "no configuration path can produce a shell string" a property of
//! the type rather than of a validator someone has to remember to call.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ShortcutError {
    #[error("actions.shortcut.unknown_key:{0}")]
    UnknownKey(String),
    #[error("actions.shortcut.unknown_modifier:{0}")]
    UnknownModifier(String),
    #[error("actions.shortcut.no_modifier")]
    NoModifier,
    #[error("actions.shortcut.malformed:{0}")]
    Malformed(String),
}

/// The modifiers a shortcut may hold. `Super` is the key GNOME reserves for the
/// desktop itself, and it is in the set because a gesture-replacing shortcut
/// usually wants it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl Modifier {
    pub const ALL: [Self; 4] = [Self::Ctrl, Self::Alt, Self::Shift, Self::Super];

    /// The spelling GNOME's own keybinding settings use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Super => "Super",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|modifier| modifier.name().eq_ignore_ascii_case(name))
    }
}

/// Every key a Better OS shortcut may name.
///
/// The list is deliberately short of anything exotic. It covers the letters,
/// the digits, the function row, and the navigation and editing keys, which is
/// what a desktop shortcut is made of. A key that is not here is refused rather
/// than passed through, because "passed through" is how a key name becomes a
/// command line.
const KEY_NAMES: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "space",
    "Tab",
    "Return",
    "Escape",
    "BackSpace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "Page_Up",
    "Page_Down",
    "Left",
    "Right",
    "Up",
    "Down",
    "minus",
    "equal",
    "bracketleft",
    "bracketright",
    "comma",
    "period",
    "slash",
    "backslash",
    "semicolon",
    "apostrophe",
    "grave",
];

/// One key from [`KEY_NAMES`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Key(&'static str);

impl Key {
    /// Every key that exists. There is no other source of a [`Key`].
    pub fn all() -> impl Iterator<Item = Self> {
        KEY_NAMES.iter().map(|name| Self(name))
    }

    pub fn name(self) -> &'static str {
        self.0
    }

    /// The only constructor. A name outside the table is refused; nothing is
    /// normalized, trimmed, or guessed into existence.
    pub fn parse(name: &str) -> Result<Self, ShortcutError> {
        KEY_NAMES
            .iter()
            .find(|known| **known == name)
            .map(|known| Self(known))
            .ok_or_else(|| ShortcutError::UnknownKey(name.to_string()))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Key::parse(&name).map_err(D::Error::custom)
    }
}

/// A modifier set and one key.
///
/// At least one modifier is required. A gesture bound to a bare `d` would type
/// the letter into whatever is focused, which is a way to lose work rather than
/// a shortcut.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ShortcutText", into = "ShortcutText")]
pub struct KeyboardShortcut {
    modifiers: BTreeSet<Modifier>,
    key: Key,
}

impl KeyboardShortcut {
    pub fn new(
        modifiers: impl IntoIterator<Item = Modifier>,
        key: Key,
    ) -> Result<Self, ShortcutError> {
        let modifiers: BTreeSet<Modifier> = modifiers.into_iter().collect();
        if modifiers.is_empty() {
            return Err(ShortcutError::NoModifier);
        }
        Ok(Self { modifiers, key })
    }

    pub fn modifiers(&self) -> impl Iterator<Item = Modifier> + '_ {
        self.modifiers.iter().copied()
    }

    pub fn key(&self) -> Key {
        self.key
    }

    /// Reads GNOME's own keybinding spelling, `<Super><Shift>d`.
    ///
    /// Anything else — a bare key, a stray character, an unbalanced bracket,
    /// a space-separated command line — is refused with the reason.
    pub fn parse(text: &str) -> Result<Self, ShortcutError> {
        let mut rest = text.trim();
        let mut modifiers = BTreeSet::new();
        while let Some(stripped) = rest.strip_prefix('<') {
            let (name, tail) = stripped
                .split_once('>')
                .ok_or_else(|| ShortcutError::Malformed(text.to_string()))?;
            let modifier =
                Modifier::parse(name).ok_or_else(|| ShortcutError::UnknownModifier(name.into()))?;
            modifiers.insert(modifier);
            rest = tail;
        }
        if rest.is_empty() || rest.contains('<') || rest.contains('>') {
            return Err(ShortcutError::Malformed(text.to_string()));
        }
        Self::new(modifiers, Key::parse(rest)?)
    }

    /// The same spelling back again.
    pub fn to_gnome(&self) -> String {
        let mut text = String::new();
        for modifier in &self.modifiers {
            text.push('<');
            text.push_str(modifier.name());
            text.push('>');
        }
        text.push_str(self.key.name());
        text
    }
}

impl fmt::Display for KeyboardShortcut {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_gnome())
    }
}

/// The serialized form: one string in GNOME's spelling, validated on the way
/// in. Serde therefore cannot construct a shortcut the constructor would have
/// refused.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct ShortcutText(String);

impl TryFrom<ShortcutText> for KeyboardShortcut {
    type Error = ShortcutError;

    fn try_from(text: ShortcutText) -> Result<Self, Self::Error> {
        Self::parse(&text.0)
    }
}

impl From<KeyboardShortcut> for ShortcutText {
    fn from(shortcut: KeyboardShortcut) -> Self {
        Self(shortcut.to_gnome())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_round_trips_through_the_spelling_gnome_uses() {
        let shortcut = KeyboardShortcut::parse("<Super><Shift>d").unwrap();
        assert_eq!(shortcut.key(), Key::parse("d").unwrap());
        assert_eq!(
            shortcut.modifiers().collect::<Vec<_>>(),
            vec![Modifier::Shift, Modifier::Super]
        );
        assert_eq!(shortcut.to_gnome(), "<Shift><Super>d");
        assert_eq!(KeyboardShortcut::parse(&shortcut.to_gnome()), Ok(shortcut));
    }

    #[test]
    fn every_key_in_the_table_parses_back_to_itself_and_nothing_else_parses() {
        for key in Key::all() {
            assert_eq!(Key::parse(key.name()), Ok(key));
        }
        for rejected in ["", "D", "sh", "rm -rf /", "$(id)", "Super", "F13", " a"] {
            assert_eq!(
                Key::parse(rejected),
                Err(ShortcutError::UnknownKey(rejected.to_string())),
                "{rejected} was accepted as a key"
            );
        }
    }

    #[test]
    fn a_shortcut_with_no_modifier_is_refused_rather_than_typed_into_the_focused_window() {
        assert_eq!(KeyboardShortcut::parse("d"), Err(ShortcutError::NoModifier));
        assert_eq!(
            KeyboardShortcut::new([], Key::parse("d").unwrap()),
            Err(ShortcutError::NoModifier)
        );
    }

    #[test]
    fn nothing_that_looks_like_a_command_survives_parsing() {
        for attempt in [
            "<Super>d; rm -rf ~",
            "sh -c 'id'",
            "<Super>$(id)",
            "<Shell>d",
            "<Super",
            "<Super>",
            "<Ctrl><Alt>xterm",
        ] {
            assert!(
                KeyboardShortcut::parse(attempt).is_err(),
                "{attempt} was accepted as a shortcut"
            );
        }
    }

    #[test]
    fn deserializing_validates_so_a_stored_file_cannot_smuggle_one_in() {
        let good: KeyboardShortcut = serde_json::from_str("\"<Super>space\"").unwrap();
        assert_eq!(good.to_gnome(), "<Super>space");
        assert!(serde_json::from_str::<KeyboardShortcut>("\"<Super>rm\"").is_err());
        assert!(serde_json::from_str::<Key>("\"pkill\"").is_err());
        assert_eq!(serde_json::to_string(&good).unwrap(), "\"<Super>space\"");
    }
}

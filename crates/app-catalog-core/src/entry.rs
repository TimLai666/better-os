//! The Desktop Entry Specification file format, parsed as untrusted input.
//!
//! This layer knows nothing about applications. It turns bytes into groups and
//! localized keys, rejecting anything the specification does not allow, so the
//! normalization layer above it can assume well-formed structure.

use std::collections::BTreeMap;

use crate::error::{EntryError, MAX_ENTRY_BYTES, MAX_VALUE_CHARS};

/// The group every application entry must carry.
pub const DESKTOP_ENTRY_GROUP: &str = "Desktop Entry";

/// A locale as the specification models it: language, optional country,
/// optional modifier. The encoding part of a POSIX locale is discarded because
/// desktop entry keys never carry one.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Locale {
    pub language: String,
    pub country: Option<String>,
    pub modifier: Option<String>,
}

impl Locale {
    /// Parses a POSIX locale such as `zh_TW.UTF-8@traditional`. An empty or
    /// `C`/`POSIX` locale yields `None`, meaning "use the untranslated value".
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value == "C" || value == "POSIX" {
            return None;
        }
        let (rest, modifier) = match value.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier.to_string())),
            None => (value, None),
        };
        // Drop the encoding: `zh_TW.UTF-8` and `zh_TW` select the same keys.
        let rest = rest.split('.').next().unwrap_or(rest);
        let (language, country) = match rest.split_once('_') {
            Some((language, country)) => (language, Some(country.to_string())),
            None => (rest, None),
        };
        if language.is_empty() {
            return None;
        }
        Some(Self {
            language: language.to_string(),
            country,
            modifier,
        })
    }

    /// The key suffixes to try, most specific first, per the specification's
    /// localized-value matching rules.
    pub fn fallback_chain(&self) -> Vec<String> {
        let mut chain = Vec::with_capacity(4);
        match (&self.country, &self.modifier) {
            (Some(country), Some(modifier)) => {
                chain.push(format!("{}_{}@{}", self.language, country, modifier));
                chain.push(format!("{}_{}", self.language, country));
                chain.push(format!("{}@{}", self.language, modifier));
            }
            (Some(country), None) => {
                chain.push(format!("{}_{}", self.language, country));
            }
            (None, Some(modifier)) => {
                chain.push(format!("{}@{}", self.language, modifier));
            }
            (None, None) => {}
        }
        chain.push(self.language.clone());
        chain
    }
}

/// A value with its translations. The untranslated value is always present;
/// resolution falls back to it when no translation matches.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalizedText {
    default: String,
    translations: BTreeMap<String, String>,
}

impl LocalizedText {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            default: default.into(),
            translations: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, locale: impl Into<String>, value: impl Into<String>) {
        self.translations.insert(locale.into(), value.into());
    }

    /// The untranslated value, which is what the specification requires an
    /// entry to carry.
    pub fn default_value(&self) -> &str {
        &self.default
    }

    pub fn translations(&self) -> &BTreeMap<String, String> {
        &self.translations
    }

    /// The best value for `locale`, walking the specification's fallback chain
    /// before giving up on the untranslated value.
    pub fn resolve(&self, locale: Option<&Locale>) -> &str {
        let Some(locale) = locale else {
            return &self.default;
        };
        for candidate in locale.fallback_chain() {
            if let Some(value) = self.translations.get(&candidate) {
                return value;
            }
        }
        &self.default
    }
}

/// A localized list value, such as `Keywords`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalizedList {
    default: Vec<String>,
    translations: BTreeMap<String, Vec<String>>,
}

impl LocalizedList {
    pub fn new(default: Vec<String>) -> Self {
        Self {
            default,
            translations: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, locale: impl Into<String>, value: Vec<String>) {
        self.translations.insert(locale.into(), value);
    }

    pub fn default_value(&self) -> &[String] {
        &self.default
    }

    pub fn resolve(&self, locale: Option<&Locale>) -> &[String] {
        let Some(locale) = locale else {
            return &self.default;
        };
        for candidate in locale.fallback_chain() {
            if let Some(value) = self.translations.get(&candidate) {
                return value;
            }
        }
        &self.default
    }
}

/// One `key` or `key[locale]` occurrence inside a group.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Field {
    key: String,
    locale: Option<String>,
    value: String,
}

/// One `[Group]` section of a desktop entry file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group {
    name: String,
    fields: Vec<Field>,
}

impl Group {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The raw, still-escaped untranslated value of `key`.
    fn raw(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.key == key && field.locale.is_none())
            .map(|field| field.value.as_str())
    }

    /// The untranslated value of `key` with escape sequences resolved.
    pub fn value(&self, key: &str) -> Option<String> {
        self.raw(key).map(unescape)
    }

    /// The untranslated value of `key` split into a semicolon-separated list.
    pub fn list(&self, key: &str) -> Option<Vec<String>> {
        self.raw(key).map(split_list)
    }

    /// A `true`/`false` value. Any other spelling is a rejection, not a
    /// silently-false default: a hostile entry must not be able to turn
    /// `Terminal=TRUE` into "not a terminal application".
    pub fn boolean(&self, key: &'static str) -> Result<Option<bool>, EntryError> {
        match self.raw(key) {
            None => Ok(None),
            Some("true") => Ok(Some(true)),
            Some("false") => Ok(Some(false)),
            Some(_) => Err(EntryError::InvalidBoolean(key)),
        }
    }

    /// The untranslated value of `key` plus every translation of it.
    pub fn localized(&self, key: &str) -> Option<LocalizedText> {
        let default = self.value(key)?;
        let mut text = LocalizedText::new(default);
        for field in self.fields.iter().filter(|field| field.key == key) {
            if let Some(locale) = &field.locale {
                text.insert(locale.clone(), unescape(&field.value));
            }
        }
        Some(text)
    }

    /// The untranslated list value of `key` plus every translation of it.
    pub fn localized_list(&self, key: &str) -> Option<LocalizedList> {
        let default = self.list(key)?;
        let mut list = LocalizedList::new(default);
        for field in self.fields.iter().filter(|field| field.key == key) {
            if let Some(locale) = &field.locale {
                list.insert(locale.clone(), split_list(&field.value));
            }
        }
        Some(list)
    }

    pub fn has_key_prefix(&self, prefix: &str) -> bool {
        self.fields
            .iter()
            .any(|field| field.key.starts_with(prefix))
    }
}

/// A parsed desktop entry file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopFile {
    groups: Vec<Group>,
}

impl DesktopFile {
    /// Parses raw bytes. Non-UTF-8 input is rejected rather than lossily
    /// converted, because a lossy name is a wrong name.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, EntryError> {
        if bytes.len() > MAX_ENTRY_BYTES {
            return Err(EntryError::EntryTooLarge(bytes.len()));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| EntryError::InvalidEncoding)?;
        Self::parse(text)
    }

    pub fn parse(input: &str) -> Result<Self, EntryError> {
        if input.len() > MAX_ENTRY_BYTES {
            return Err(EntryError::EntryTooLarge(input.len()));
        }
        let mut groups: Vec<Group> = Vec::new();
        for (index, raw_line) in input.lines().enumerate() {
            let number = index + 1;
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('[') {
                let name = rest
                    .strip_suffix(']')
                    .ok_or(EntryError::InvalidGroupHeader(number))?;
                if name.is_empty()
                    || name
                        .chars()
                        .any(|character| character.is_control() || character == '[')
                {
                    return Err(EntryError::InvalidGroupHeader(number));
                }
                if groups.iter().any(|group| group.name == name) {
                    return Err(EntryError::DuplicateGroup(name.to_string()));
                }
                groups.push(Group {
                    name: name.to_string(),
                    fields: Vec::new(),
                });
                continue;
            }
            let Some(group) = groups.last_mut() else {
                return Err(EntryError::ContentBeforeGroup(number));
            };
            let (raw_key, value) = line
                .split_once('=')
                .ok_or(EntryError::InvalidLine(number))?;
            let (key, locale) = split_key(raw_key.trim()).ok_or(EntryError::InvalidKey(number))?;
            let value = value.trim_start_matches(' ');
            if value.chars().count() > MAX_VALUE_CHARS {
                return Err(EntryError::ValueTooLong("value"));
            }
            if value
                .chars()
                .any(|character| character.is_control() && character != '\t')
            {
                return Err(EntryError::ControlCharacter("value"));
            }
            if group
                .fields
                .iter()
                .any(|field| field.key == key && field.locale == locale)
            {
                return Err(EntryError::DuplicateKey {
                    group: group.name.clone(),
                    key: raw_key.trim().to_string(),
                });
            }
            group.fields.push(Field {
                key,
                locale,
                value: value.to_string(),
            });
        }
        if groups.is_empty() {
            return Err(EntryError::MissingDesktopEntryGroup);
        }
        Ok(Self { groups })
    }

    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }

    pub fn desktop_entry(&self) -> Result<&Group, EntryError> {
        self.group(DESKTOP_ENTRY_GROUP)
            .ok_or(EntryError::MissingDesktopEntryGroup)
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }
}

/// Splits `Name[zh_TW]` into its key and locale. Returns `None` when either
/// half uses a character the specification does not allow, which keeps a
/// crafted key such as `Name[../../etc]` out of the record model.
fn split_key(raw: &str) -> Option<(String, Option<String>)> {
    let (key, locale) = match raw.split_once('[') {
        Some((key, rest)) => {
            let locale = rest.strip_suffix(']')?;
            if locale.is_empty()
                || !locale.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || character == '_'
                        || character == '@'
                        || character == '-'
                        || character == '.'
                })
            {
                return None;
            }
            (key, Some(locale.to_string()))
        }
        None => (raw, None),
    };
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return None;
    }
    Some((key.to_string(), locale))
}

/// Resolves the escape sequences the specification defines for string values.
/// An unknown escape keeps both characters rather than swallowing the
/// backslash, so a value never silently changes meaning.
fn unescape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('s') => output.push(' '),
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

/// Splits a semicolon-separated list value. `\;` is a literal semicolon, and a
/// trailing separator does not produce an empty final element.
fn split_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            match character {
                ';' => current.push(';'),
                's' => current.push(' '),
                'n' => current.push('\n'),
                't' => current.push('\t'),
                'r' => current.push('\r'),
                '\\' => current.push('\\'),
                other => {
                    current.push('\\');
                    current.push(other);
                }
            }
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            ';' => items.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        items.push(current);
    }
    items.retain(|item| !item.trim().is_empty());
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_and_localized_keys() {
        let file = DesktopFile::parse(
            "[Desktop Entry]\nName=Text Editor\nName[zh_TW]=文字編輯器\n\n[Desktop Action New]\nName=New\n",
        )
        .unwrap();
        let entry = file.desktop_entry().unwrap();
        let name = entry.localized("Name").unwrap();
        assert_eq!(name.default_value(), "Text Editor");
        assert_eq!(
            name.resolve(Locale::parse("zh_TW.UTF-8").as_ref()),
            "文字編輯器"
        );
        assert_eq!(
            file.group("Desktop Action New").unwrap().name(),
            "Desktop Action New"
        );
    }

    #[test]
    fn localized_name_falls_back_through_the_chain() {
        let file = DesktopFile::parse(
            "[Desktop Entry]\nName=Editor\nName[zh]=編輯器\nName[de_DE]=Editor DE\n",
        )
        .unwrap();
        let name = file.desktop_entry().unwrap().localized("Name").unwrap();
        // zh_TW has no exact key, so the language-only key wins.
        assert_eq!(name.resolve(Locale::parse("zh_TW").as_ref()), "編輯器");
        // A modifier is dropped before the language-only key is tried.
        assert_eq!(
            name.resolve(Locale::parse("zh_TW@traditional").as_ref()),
            "編輯器"
        );
        // An unrelated locale falls all the way back to the untranslated value.
        assert_eq!(name.resolve(Locale::parse("fr_FR").as_ref()), "Editor");
        // The C locale never selects a translation.
        assert_eq!(name.resolve(Locale::parse("C").as_ref()), "Editor");
    }

    #[test]
    fn country_specific_key_beats_language_key() {
        let file =
            DesktopFile::parse("[Desktop Entry]\nName=A\nName[zh]=B\nName[zh_TW]=C\n").unwrap();
        let name = file.desktop_entry().unwrap().localized("Name").unwrap();
        assert_eq!(name.resolve(Locale::parse("zh_TW").as_ref()), "C");
        assert_eq!(name.resolve(Locale::parse("zh_CN").as_ref()), "B");
    }

    #[test]
    fn unescapes_string_values() {
        let file =
            DesktopFile::parse("[Desktop Entry]\nComment=one\\stwo\\nthree\\\\four\n").unwrap();
        assert_eq!(
            file.desktop_entry().unwrap().value("Comment").unwrap(),
            "one two\nthree\\four"
        );
    }

    #[test]
    fn splits_list_values_and_honors_escaped_separator() {
        let file =
            DesktopFile::parse("[Desktop Entry]\nCategories=Utility;Text\\;Editor;;Development;\n")
                .unwrap();
        assert_eq!(
            file.desktop_entry().unwrap().list("Categories").unwrap(),
            vec!["Utility", "Text;Editor", "Development"]
        );
    }

    #[test]
    fn rejects_content_before_a_group_header() {
        assert_eq!(
            DesktopFile::parse("Name=Orphan\n[Desktop Entry]\n").unwrap_err(),
            EntryError::ContentBeforeGroup(1)
        );
    }

    #[test]
    fn rejects_truncated_group_header() {
        assert_eq!(
            DesktopFile::parse("[Desktop Entry\nName=X\n").unwrap_err(),
            EntryError::InvalidGroupHeader(1)
        );
    }

    #[test]
    fn rejects_a_line_without_a_separator() {
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nName\n").unwrap_err(),
            EntryError::InvalidLine(2)
        );
    }

    #[test]
    fn rejects_duplicate_keys_and_groups() {
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nName=A\nName=B\n").unwrap_err(),
            EntryError::DuplicateKey {
                group: "Desktop Entry".to_string(),
                key: "Name".to_string(),
            }
        );
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nName=A\n[Desktop Entry]\nName=B\n").unwrap_err(),
            EntryError::DuplicateGroup("Desktop Entry".to_string())
        );
    }

    #[test]
    fn rejects_invalid_keys_and_locales() {
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nNa me=A\n").unwrap_err(),
            EntryError::InvalidKey(2)
        );
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nName[../../etc]=A\n").unwrap_err(),
            EntryError::InvalidKey(2)
        );
    }

    #[test]
    fn rejects_non_utf8_and_oversized_input() {
        assert_eq!(
            DesktopFile::parse_bytes(b"[Desktop Entry]\nName=\xff\xfe\n").unwrap_err(),
            EntryError::InvalidEncoding
        );
        let oversized = vec![b'#'; MAX_ENTRY_BYTES + 1];
        assert_eq!(
            DesktopFile::parse_bytes(&oversized).unwrap_err(),
            EntryError::EntryTooLarge(MAX_ENTRY_BYTES + 1)
        );
    }

    #[test]
    fn rejects_an_empty_file() {
        assert_eq!(
            DesktopFile::parse("\n# only a comment\n").unwrap_err(),
            EntryError::MissingDesktopEntryGroup
        );
    }

    #[test]
    fn boolean_values_must_be_spelled_exactly() {
        let file = DesktopFile::parse("[Desktop Entry]\nTerminal=TRUE\n").unwrap();
        assert_eq!(
            file.desktop_entry()
                .unwrap()
                .boolean("Terminal")
                .unwrap_err(),
            EntryError::InvalidBoolean("Terminal")
        );
        let file = DesktopFile::parse("[Desktop Entry]\nTerminal=true\n").unwrap();
        assert_eq!(
            file.desktop_entry().unwrap().boolean("Terminal").unwrap(),
            Some(true)
        );
    }

    #[test]
    fn rejects_control_characters_in_values() {
        assert_eq!(
            DesktopFile::parse("[Desktop Entry]\nName=Ev\u{0}il\n").unwrap_err(),
            EntryError::ControlCharacter("value")
        );
    }
}

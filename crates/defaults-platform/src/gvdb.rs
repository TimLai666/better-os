//! A read-only reader for the GVDB database dconf keeps the user's settings in.
//!
//! `~/.config/dconf/user` is a binary GVDB file holding serialized GVariant
//! values. Reading it directly is how a keybinding is inspected without running
//! `gsettings`, which the project forbids and which would give back a formatted
//! string rather than a typed value.
//!
//! Only reading is implemented, and deliberately so. The dconf service keeps
//! its own view of this file and rewrites it; a process that edited the bytes
//! behind the service would have its change ignored or overwritten. Writing
//! belongs to the service, and goes through it: see the `dconf_writer` module.
//!
//! The file is untrusted input. Every offset is bounds-checked, a parent chain
//! that loops is refused rather than followed, and a value type this reader
//! does not understand is reported as unsupported instead of being coerced.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

const SIGNATURE_0: u32 = u32::from_le_bytes(*b"GVar");
const SIGNATURE_1: u32 = u32::from_le_bytes(*b"iant");
const HEADER_SIZE: usize = 24;
const HASH_ITEM_SIZE: usize = 24;
/// The bottom 20 bits of the bloom word count are the count; the rest is the
/// bloom shift, which this reader does not need.
const BLOOM_WORD_MASK: u32 = (1 << 20) - 1;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum GvdbError {
    #[error("not a GVDB database")]
    NotGvdb,
    #[error("GVDB database is byte-swapped relative to this machine")]
    ByteSwapped,
    #[error("GVDB database is truncated or points outside itself")]
    Truncated,
    #[error("GVDB key chain does not terminate")]
    KeyChainLoops,
    #[error("GVDB key is not valid UTF-8")]
    KeyNotUtf8,
}

/// One value read out of the database, still typed the way GVariant typed it.
///
/// `Eq` is deliberately absent: one of the decodable types is a double, and a
/// total equality for a value that can be NaN would be a lie the compiler would
/// otherwise let this enum tell.
#[derive(Clone, Debug, PartialEq)]
pub enum GVariantValue {
    Text(String),
    TextList(Vec<String>),
    Boolean(bool),
    /// A GVariant `d`. GNOME stores pointer and scroll speeds this way, so
    /// `touchpad-platform` needs it; no defaults integration declares one.
    Double(f64),
    /// A type this reader does not decode. The signature is carried so the
    /// refusal can name what it saw.
    Unsupported {
        signature: String,
    },
    /// The value bytes did not match their own declared type.
    Malformed {
        signature: String,
    },
}

/// A parsed dconf database: full key paths to typed values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GvdbDatabase {
    values: BTreeMap<String, GVariantValue>,
    /// The keys the database locks: a system database's `.locks` table,
    /// which `dconf compile` fills from its keyfile directory's `locks/`
    /// files (dconf 0.40 `engine/dconf-engine-source.c` reads it as
    /// `gvdb_table_get_table (values, ".locks")`).
    locks: BTreeSet<String>,
}

impl GvdbDatabase {
    pub fn get(&self, key: &str) -> Option<&GVariantValue> {
        self.values.get(key)
    }

    /// Whether the database locks `key`.
    pub fn is_locked(&self, key: &str) -> bool {
        self.locks.contains(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, GvdbError> {
        let (root_start, root_end) = root(bytes)?;
        let items = hash_items(bytes, root_start, root_end)?;

        let mut values = BTreeMap::new();
        let mut locks = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            match item.item_type {
                b'v' => {
                    let key = full_key(bytes, &items, index)?;
                    let data = slice(bytes, item.value_start, item.value_end)?;
                    values.insert(key, decode_variant(data));
                }
                b'H' if full_key(bytes, &items, index)? == ".locks" => {
                    let locked = hash_items(bytes, item.value_start, item.value_end)?;
                    for (inner, entry) in locked.iter().enumerate() {
                        if entry.item_type == b'v' {
                            locks.insert(full_key(bytes, &locked, inner)?);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(Self { values, locks })
    }
}

/// The default a compiled GSettings schema gives the key at `key_path`
/// (`/org/gnome/shell/favorite-apps`), read out of one `gschemas.compiled`.
///
/// A compiled schema file is a GVDB table of schema ids, each a nested table
/// of its keys plus `.path`. A key's value is a tuple whose first member is
/// its default, overrides already applied by `glib-compile-schemas`. Only a
/// schema whose `.path` is the key's directory counts: a relocatable schema
/// has no path and no default that belongs to one place. `Ok(None)` is "no
/// schema in this file has that key", which is a fact and not a failure.
pub fn schema_default(bytes: &[u8], key_path: &str) -> Result<Option<GVariantValue>, GvdbError> {
    let Some(split) = key_path.rfind('/') else {
        return Ok(None);
    };
    let (directory, name) = (&key_path[..=split], &key_path[split + 1..]);
    if name.is_empty() {
        return Ok(None);
    }
    let (root_start, root_end) = root(bytes)?;
    let schemas = hash_items(bytes, root_start, root_end)?;
    for schema in schemas.iter().filter(|item| item.item_type == b'H') {
        let keys = hash_items(bytes, schema.value_start, schema.value_end)?;
        let mut path = None;
        let mut value = None;
        for (index, key) in keys.iter().enumerate() {
            if key.item_type != b'v' {
                continue;
            }
            let full = full_key(bytes, &keys, index)?;
            let data = slice(bytes, key.value_start, key.value_end)?;
            if full == ".path" {
                path = match decode_variant(data) {
                    GVariantValue::Text(text) => Some(text),
                    _ => None,
                };
            } else if full == name {
                value = Some(first_tuple_member(data));
            }
        }
        if path.as_deref() == Some(directory)
            && let Some(value) = value
        {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// A schema key is stored as a variant holding `(T...)`: its default, then
/// any range or choice metadata. The default is the first member.
fn first_tuple_member(data: &[u8]) -> GVariantValue {
    let malformed = |signature: &str| GVariantValue::Malformed {
        signature: signature.to_string(),
    };
    let Some(separator) = data.iter().rposition(|byte| *byte == 0) else {
        return malformed("");
    };
    let Ok(signature) = std::str::from_utf8(&data[separator + 1..]) else {
        return malformed("");
    };
    let child = &data[..separator];
    let Some(inner) = signature
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return malformed(signature);
    };
    // Only the shapes a launcher list or a setting needs: a string array,
    // a string, or a boolean as the first member.
    let (member, rest) = if let Some(rest) = inner.strip_prefix("as") {
        ("as", rest)
    } else if let Some(rest) = inner.strip_prefix('s') {
        ("s", rest)
    } else if let Some(rest) = inner.strip_prefix('b') {
        ("b", rest)
    } else {
        return GVariantValue::Unsupported {
            signature: signature.to_string(),
        };
    };
    let member_bytes = if rest.is_empty() || member == "b" {
        // The last member of a tuple has no offset: it runs to the end. A
        // boolean is fixed-width, one byte, wherever it sits.
        if member == "b" {
            child.get(..1)
        } else {
            Some(child)
        }
    } else {
        // A variable-width member that is not last ends where the tuple's
        // trailing offset table says; the first such offset is stored last.
        let width = match child.len() {
            0..=0xff => 1,
            0x100..=0xffff => 2,
            _ => 4,
        };
        child
            .len()
            .checked_sub(width)
            .and_then(|at| read_offset(child, at, width))
            .and_then(|end| child.get(..end))
    };
    let Some(member_bytes) = member_bytes else {
        return malformed(signature);
    };
    let mut variant = member_bytes.to_vec();
    variant.push(0);
    variant.extend_from_slice(member.as_bytes());
    decode_variant(&variant)
}

fn root(bytes: &[u8]) -> Result<(usize, usize), GvdbError> {
    if bytes.len() < HEADER_SIZE {
        return Err(GvdbError::Truncated);
    }
    let signature_0 = read_u32(bytes, 0)?;
    let signature_1 = read_u32(bytes, 4)?;
    if signature_0 != SIGNATURE_0 || signature_1 != SIGNATURE_1 {
        if signature_0.swap_bytes() == SIGNATURE_0 && signature_1.swap_bytes() == SIGNATURE_1 {
            return Err(GvdbError::ByteSwapped);
        }
        return Err(GvdbError::NotGvdb);
    }
    Ok((read_u32(bytes, 16)? as usize, read_u32(bytes, 20)? as usize))
}

#[derive(Clone, Copy, Debug)]
struct HashItem {
    parent: u32,
    key_start: usize,
    key_end: usize,
    item_type: u8,
    value_start: usize,
    value_end: usize,
}

fn hash_items(bytes: &[u8], start: usize, end: usize) -> Result<Vec<HashItem>, GvdbError> {
    if end < start || end > bytes.len() || end - start < 8 {
        return Err(GvdbError::Truncated);
    }
    let bloom_words = (read_u32(bytes, start)? & BLOOM_WORD_MASK) as usize;
    let buckets = read_u32(bytes, start + 4)? as usize;
    let items_start = start
        .checked_add(8)
        .and_then(|offset| offset.checked_add(bloom_words.checked_mul(4)?))
        .and_then(|offset| offset.checked_add(buckets.checked_mul(4)?))
        .ok_or(GvdbError::Truncated)?;
    if items_start > end {
        return Err(GvdbError::Truncated);
    }
    let span = end - items_start;
    if !span.is_multiple_of(HASH_ITEM_SIZE) {
        return Err(GvdbError::Truncated);
    }

    let mut items = Vec::with_capacity(span / HASH_ITEM_SIZE);
    for index in 0..span / HASH_ITEM_SIZE {
        let base = items_start + index * HASH_ITEM_SIZE;
        let parent = read_u32(bytes, base + 4)?;
        let key_start = read_u32(bytes, base + 8)? as usize;
        let key_size = read_u16(bytes, base + 12)? as usize;
        let item_type = *bytes.get(base + 14).ok_or(GvdbError::Truncated)?;
        let value_start = read_u32(bytes, base + 16)? as usize;
        let value_end = read_u32(bytes, base + 20)? as usize;
        items.push(HashItem {
            parent,
            key_start,
            key_end: key_start
                .checked_add(key_size)
                .ok_or(GvdbError::Truncated)?,
            item_type,
            value_start,
            value_end,
        });
    }
    Ok(items)
}

/// Walks the parent chain to build the full path a key is filed under. dconf
/// stores each path segment once, so `/org/gnome/...` only exists as a chain.
fn full_key(bytes: &[u8], items: &[HashItem], index: usize) -> Result<String, GvdbError> {
    let mut segments = Vec::new();
    let mut current = index;
    // The chain cannot be longer than the table without repeating an item.
    for _ in 0..=items.len() {
        let item = items.get(current).ok_or(GvdbError::Truncated)?;
        let segment = slice(bytes, item.key_start, item.key_end)?;
        segments.push(std::str::from_utf8(segment).map_err(|_| GvdbError::KeyNotUtf8)?);
        if item.parent == u32::MAX {
            segments.reverse();
            return Ok(segments.concat());
        }
        current = item.parent as usize;
    }
    Err(GvdbError::KeyChainLoops)
}

/// A GVariant `v` is the child's bytes, a zero byte, then the child's type
/// signature.
fn decode_variant(data: &[u8]) -> GVariantValue {
    let Some(separator) = data.iter().rposition(|byte| *byte == 0) else {
        return GVariantValue::Malformed {
            signature: String::new(),
        };
    };
    let signature = match std::str::from_utf8(&data[separator + 1..]) {
        Ok(signature) => signature.to_string(),
        Err(_) => {
            return GVariantValue::Malformed {
                signature: String::new(),
            };
        }
    };
    let child = &data[..separator];
    match signature.as_str() {
        "s" => match decode_string(child) {
            Some(text) => GVariantValue::Text(text),
            None => GVariantValue::Malformed { signature },
        },
        "as" => match decode_string_array(child) {
            Some(values) => GVariantValue::TextList(values),
            None => GVariantValue::Malformed { signature },
        },
        "b" => match child {
            [0] => GVariantValue::Boolean(false),
            [1] => GVariantValue::Boolean(true),
            _ => GVariantValue::Malformed { signature },
        },
        "d" => match child.try_into() {
            Ok(bytes) => GVariantValue::Double(f64::from_le_bytes(bytes)),
            Err(_) => GVariantValue::Malformed { signature },
        },
        _ => GVariantValue::Unsupported { signature },
    }
}

/// A GVariant string is its bytes followed by one zero byte.
fn decode_string(child: &[u8]) -> Option<String> {
    let text = child.strip_suffix(&[0u8]).unwrap_or(child);
    std::str::from_utf8(text).ok().map(str::to_string)
}

/// A GVariant array of variable-width elements ends with a table of the offsets
/// just past each element. The width of an offset comes from the size of the
/// whole array, which is the one piece of context the caller already has.
fn decode_string_array(child: &[u8]) -> Option<Vec<String>> {
    if child.is_empty() {
        return Some(Vec::new());
    }
    let offset_width = match child.len() {
        0 => return Some(Vec::new()),
        length if length <= 0xff => 1usize,
        length if length <= 0xffff => 2,
        length if length <= 0xffff_ffff => 4,
        _ => 8,
    };
    if child.len() < offset_width {
        return None;
    }
    let last = read_offset(child, child.len() - offset_width, offset_width)?;
    if last > child.len() || !(child.len() - last).is_multiple_of(offset_width) {
        return None;
    }
    let count = (child.len() - last) / offset_width;

    let mut values = Vec::with_capacity(count);
    let mut start = 0usize;
    for index in 0..count {
        let end = read_offset(child, last + index * offset_width, offset_width)?;
        if end < start || end > last {
            return None;
        }
        values.push(decode_string(child.get(start..end)?)?);
        start = end;
    }
    Some(values)
}

fn read_offset(bytes: &[u8], at: usize, width: usize) -> Option<usize> {
    let mut value = 0usize;
    for index in (0..width).rev() {
        value = value.checked_shl(8)? | *bytes.get(at + index)? as usize;
    }
    Some(value)
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, GvdbError> {
    bytes
        .get(at..at + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(GvdbError::Truncated)
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, GvdbError> {
    bytes
        .get(at..at + 2)
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(GvdbError::Truncated)
}

fn slice(bytes: &[u8], start: usize, end: usize) -> Result<&[u8], GvdbError> {
    if end < start {
        return Err(GvdbError::Truncated);
    }
    bytes.get(start..end).ok_or(GvdbError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_DB: &[u8] = include_bytes!("../tests/fixtures/dconf/user");

    const SCHEMAS: &[u8] = include_bytes!("../tests/fixtures/schemas/gschemas.compiled");

    /// The fixture is `tests/fixtures/schemas/org.example.shell.gschema.xml`
    /// compiled by GLib's own `glib-compile-schemas`.
    #[test]
    fn a_schema_default_is_read_by_the_key_s_path() {
        assert_eq!(
            schema_default(SCHEMAS, "/org/example/shell/favorite-apps").unwrap(),
            Some(GVariantValue::TextList(vec![
                "org.gnome.Nautilus.desktop".to_string(),
                "firefox.desktop".to_string()
            ]))
        );
        assert_eq!(
            schema_default(SCHEMAS, "/org/example/shell/enabled").unwrap(),
            Some(GVariantValue::Boolean(true))
        );
        // The same key name under another path is that schema's own default.
        assert_eq!(
            schema_default(SCHEMAS, "/org/example/other/favorite-apps").unwrap(),
            Some(GVariantValue::TextList(vec!["wrong.desktop".to_string()]))
        );
    }

    #[test]
    fn a_key_no_schema_has_is_absent_and_a_relocatable_schema_belongs_nowhere() {
        assert_eq!(
            schema_default(SCHEMAS, "/org/example/shell/missing").unwrap(),
            None
        );
        assert_eq!(
            schema_default(SCHEMAS, "/org/gnome/desktop/applications/file-manager").unwrap(),
            None
        );
        assert_eq!(schema_default(SCHEMAS, "/").unwrap(), None);
        assert_eq!(
            schema_default(b"not gvdb at all, long enough", "/a/b"),
            Err(GvdbError::NotGvdb)
        );
    }

    #[test]
    fn reads_a_database_dconf_itself_wrote() {
        let database = GvdbDatabase::parse(USER_DB).expect("the fixture is a GVDB database");

        assert_eq!(
            database.get("/org/gnome/settings-daemon/plugins/media-keys/home"),
            Some(&GVariantValue::TextList(vec!["<Super>e".to_string()]))
        );
        assert_eq!(
            database.get("/org/gnome/settings-daemon/plugins/media-keys/www"),
            Some(&GVariantValue::TextList(vec![
                "<Super>b".to_string(),
                "<Super>w".to_string()
            ]))
        );
        assert_eq!(
            database.get("/org/gnome/desktop/wm/keybindings/close"),
            Some(&GVariantValue::TextList(Vec::new()))
        );
        assert_eq!(
            database.get("/org/gnome/nautilus/preferences/show-image-thumbnails"),
            Some(&GVariantValue::Text("always".to_string()))
        );
        assert_eq!(
            database.get("/org/gnome/desktop/interface/enable-animations"),
            Some(&GVariantValue::Boolean(false))
        );
    }

    #[test]
    fn a_key_the_database_does_not_hold_is_absent_rather_than_defaulted() {
        let database = GvdbDatabase::parse(USER_DB).expect("the fixture is a GVDB database");
        assert_eq!(
            database.get("/org/gnome/desktop/wm/keybindings/minimize"),
            None
        );
    }

    #[test]
    fn refuses_input_that_is_not_a_database() {
        assert_eq!(
            GvdbDatabase::parse(b"this is a text file, not a database"),
            Err(GvdbError::NotGvdb)
        );
        assert_eq!(GvdbDatabase::parse(b""), Err(GvdbError::Truncated));
    }

    #[test]
    fn refuses_a_truncated_database_rather_than_reading_past_it() {
        for length in [HEADER_SIZE, HEADER_SIZE + 8, USER_DB.len() / 2] {
            assert!(
                GvdbDatabase::parse(&USER_DB[..length]).is_err(),
                "accepted a database truncated to {length} bytes"
            );
        }
    }

    #[test]
    fn refuses_a_byte_swapped_database() {
        let mut swapped = USER_DB.to_vec();
        swapped[0..4].reverse();
        swapped[4..8].reverse();
        assert_eq!(GvdbDatabase::parse(&swapped), Err(GvdbError::ByteSwapped));
    }

    #[test]
    fn an_unknown_value_type_is_reported_rather_than_coerced() {
        assert_eq!(
            decode_variant(&[42, 0, b'i']),
            GVariantValue::Unsupported {
                signature: "i".to_string()
            }
        );
    }

    #[test]
    fn a_double_is_decoded_rather_than_reported_as_an_unknown_type() {
        let mut data = 0.35f64.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, b'd']);
        assert_eq!(decode_variant(&data), GVariantValue::Double(0.35));
    }

    #[test]
    fn a_double_of_the_wrong_length_is_malformed_rather_than_padded() {
        assert_eq!(
            decode_variant(&[1, 2, 3, 0, b'd']),
            GVariantValue::Malformed {
                signature: "d".to_string()
            }
        );
    }

    #[test]
    fn a_value_that_does_not_match_its_type_is_reported_as_malformed() {
        assert_eq!(
            decode_variant(&[7, 0, b'b']),
            GVariantValue::Malformed {
                signature: "b".to_string()
            }
        );
    }
}

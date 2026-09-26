//! Serializing a dconf change set in GVariant encoding.
//!
//! `ca.desrt.dconf.Writer.Change` takes one argument: an array of bytes that is
//! a GVariant of type `a{smv}` — full key paths to optional values. An absent
//! value is a reset: it removes the key rather than writing something in its
//! place, which is exactly what restoring a setting the user had never touched
//! has to do.
//!
//! The type was taken off the wire rather than out of a header. dconf's own
//! change-set type is `(sa{smv})`, a prefix plus relative names, and sending
//! that shape is accepted by the service and writes nothing — the reply carries
//! a change tag and no key moves. Watching what `dconf write` itself sends is
//! what settles it: absolute paths, no prefix member.
//!
//! There is no GLib here, and no `gsettings` process. The encoding is written
//! out by hand because linking GLib into a Better OS crate to build ninety
//! bytes would be a large dependency for a small, fully specified format.
//!
//! The rules that matter, all of which the tests pin against bytes GLib itself
//! produced:
//!
//! - A variable-width container stores framing offsets at its end. The width of
//!   an offset comes from the container's own total size, offsets included.
//! - Array offsets are the end position of each element, in order. Tuple
//!   offsets are the end position of each non-final variable-width member, in
//!   reverse order.
//! - `mv` — a maybe holding a variant — is zero bytes for Nothing, and the
//!   variant's bytes plus one zero byte for Just.
//! - `v` is the child's bytes, a zero byte, then the child's type signature.
//! - A dictionary entry aligns to 8, because a variant does.
//! - `as` is each string with its terminator, back to back, then one offset per
//!   string. An empty array is no bytes at all, which is still a value: GNOME
//!   disables a keybinding by storing `@as []`, and a reset would restore it.

use std::collections::BTreeMap;

use thiserror::Error;

/// A value a change set can write: the three GVariant types the GNOME touchpad
/// and mouse schemas use, and the string array every GNOME keybinding is.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeValue {
    Boolean(bool),
    Double(f64),
    Text(String),
    TextList(Vec<String>),
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ChangesetError {
    #[error("a dconf path prefix must start and end with '/', not {0:?}")]
    BadPrefix(String),
    #[error("a dconf key name must not be empty or contain '/' or a nul byte, unlike {0:?}")]
    BadKey(String),
    #[error("{0:?} is not a dconf key under this change set's prefix")]
    BadPath(String),
    #[error("a dconf string value must not contain a nul byte")]
    NulInValue,
    #[error("a dconf value must be a real number, not {0}")]
    NotFinite(f64),
}

/// A set of changes under one path prefix.
///
/// Entries are kept sorted by full path, which is what the dconf client's own
/// tree-backed change set produces, so the same set of changes always
/// serializes to the same bytes. The prefix `/` admits any key, which is how
/// one change set spans several directories.
#[derive(Clone, Debug, PartialEq)]
pub struct Changeset {
    prefix: String,
    entries: BTreeMap<String, Option<ChangeValue>>,
}

impl Changeset {
    /// A change set under `prefix`, which must be an absolute dconf directory
    /// path — leading and trailing `/`.
    pub fn new(prefix: impl Into<String>) -> Result<Self, ChangesetError> {
        let prefix = prefix.into();
        if !prefix.starts_with('/') || !prefix.ends_with('/') || prefix.contains('\0') {
            return Err(ChangesetError::BadPrefix(prefix));
        }
        Ok(Self {
            prefix,
            entries: BTreeMap::new(),
        })
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The full paths this change set would write, for a caller that has to
    /// report what it is about to touch.
    pub fn paths(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    pub fn set(&mut self, key: &str, value: ChangeValue) -> Result<(), ChangesetError> {
        check_key(key)?;
        check_value(&value)?;
        self.entries
            .insert(format!("{}{key}", self.prefix), Some(value));
        Ok(())
    }

    /// Removes the key, so the session's own default applies again.
    pub fn reset(&mut self, key: &str) -> Result<(), ChangesetError> {
        check_key(key)?;
        self.entries.insert(format!("{}{key}", self.prefix), None);
        Ok(())
    }

    /// Writes a key named by its full path, which must lie under the prefix.
    pub fn set_path(&mut self, path: &str, value: ChangeValue) -> Result<(), ChangesetError> {
        self.check_path(path)?;
        check_value(&value)?;
        self.entries.insert(path.to_string(), Some(value));
        Ok(())
    }

    /// Resets a key named by its full path, which must lie under the prefix.
    pub fn reset_path(&mut self, path: &str) -> Result<(), ChangesetError> {
        self.check_path(path)?;
        self.entries.insert(path.to_string(), None);
        Ok(())
    }

    /// A dconf key: absolute, under the prefix, not a directory, and with no
    /// empty path segment.
    fn check_path(&self, path: &str) -> Result<(), ChangesetError> {
        let valid = path
            .strip_prefix(&self.prefix)
            .is_some_and(|relative| !relative.is_empty() && !relative.ends_with('/'))
            && !path.contains("//")
            && !path.contains('\0');
        if valid {
            Ok(())
        } else {
            Err(ChangesetError::BadPath(path.to_string()))
        }
    }

    /// The bytes `ca.desrt.dconf.Writer.Change` takes.
    ///
    /// An empty change set serializes to no bytes at all, which is what an
    /// empty GVariant array is.
    pub fn serialise(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let mut ends = Vec::with_capacity(self.entries.len());
        for (path, value) in &self.entries {
            pad_to_eight(&mut data);
            data.extend_from_slice(&entry_bytes(path, value.as_ref()));
            ends.push(data.len());
        }
        let width = offset_width(data.len(), ends.len());
        for end in ends {
            push_offset(&mut data, end, width);
        }
        data
    }
}

fn check_key(key: &str) -> Result<(), ChangesetError> {
    if key.is_empty() || key.contains('/') || key.contains('\0') {
        return Err(ChangesetError::BadKey(key.to_string()));
    }
    Ok(())
}

fn check_value(value: &ChangeValue) -> Result<(), ChangesetError> {
    match value {
        ChangeValue::Text(text) if text.contains('\0') => Err(ChangesetError::NulInValue),
        ChangeValue::TextList(values) if values.iter().any(|value| value.contains('\0')) => {
            Err(ChangesetError::NulInValue)
        }
        ChangeValue::Double(number) if !number.is_finite() => {
            Err(ChangesetError::NotFinite(*number))
        }
        _ => Ok(()),
    }
}

/// One `{smv}`: the key string, then the maybe-variant, then the offset that
/// says where the key ended.
fn entry_bytes(key: &str, value: Option<&ChangeValue>) -> Vec<u8> {
    let mut body = key.as_bytes().to_vec();
    body.push(0);
    let key_end = body.len();
    // A variant aligns to 8, so the maybe holding one does too — and the
    // padding is written even when the maybe is Nothing and takes no space.
    pad_to_eight(&mut body);
    if let Some(value) = value {
        body.extend_from_slice(&variant_bytes(value));
        // The trailing zero byte that distinguishes Just from Nothing.
        body.push(0);
    }
    let width = offset_width(body.len(), 1);
    push_offset(&mut body, key_end, width);
    body
}

/// One `v`: the child's bytes, a zero byte, then the child's signature.
fn variant_bytes(value: &ChangeValue) -> Vec<u8> {
    match value {
        ChangeValue::Boolean(value) => vec![u8::from(*value), 0, b'b'],
        ChangeValue::Double(value) => {
            let mut bytes = value.to_le_bytes().to_vec();
            bytes.extend_from_slice(&[0, b'd']);
            bytes
        }
        ChangeValue::Text(value) => {
            let mut bytes = value.as_bytes().to_vec();
            // The string's own terminator, then the variant separator.
            bytes.extend_from_slice(&[0, 0, b's']);
            bytes
        }
        ChangeValue::TextList(values) => {
            let mut bytes = string_array_bytes(values);
            bytes.extend_from_slice(&[0, b'a', b's']);
            bytes
        }
    }
}

/// One `as`: every string with its terminator, then the end of each in order.
/// A string aligns to 1, so there is no padding between them.
fn string_array_bytes(values: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut ends = Vec::with_capacity(values.len());
    for value in values {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
        ends.push(bytes.len());
    }
    let width = offset_width(bytes.len(), ends.len());
    for end in ends {
        push_offset(&mut bytes, end, width);
    }
    bytes
}

fn pad_to_eight(bytes: &mut Vec<u8>) {
    while bytes.len() % 8 != 0 {
        bytes.push(0);
    }
}

/// How wide each framing offset is. The width depends on the total size, which
/// includes the offsets, so the smallest width that still fits is the answer.
fn offset_width(body: usize, count: usize) -> usize {
    if body + count <= 0xff {
        1
    } else if body + 2 * count <= 0xffff {
        2
    } else if body + 4 * count <= 0xffff_ffff {
        4
    } else {
        8
    }
}

fn push_offset(bytes: &mut Vec<u8>, value: usize, width: usize) {
    for index in 0..width {
        bytes.push((value >> (8 * index)) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOUCHPAD: &str = "/org/gnome/desktop/peripherals/touchpad/";

    /// The expected bytes come from GLib itself:
    ///
    /// ```text
    /// python3 -c "from gi.repository import GLib; print(GLib.Variant(
    ///     'a{smv}', entries).get_data_as_bytes().get_data().hex())"
    /// ```
    ///
    /// Hand-rolled encoders drift from the specification in ways that only
    /// show up as a service accepting a call and writing nothing, so these are
    /// pinned against the reference implementation rather than against this
    /// one.
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    const PATH: &str =
        "2f6f72672f676e6f6d652f6465736b746f702f7065726970686572616c732f746f7563687061642f";

    #[test]
    fn a_single_double_matches_the_bytes_glib_produces() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset.set("speed", ChangeValue::Double(0.35)).unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            format!("{PATH}7370656564000000666666666666d63f0064002e3c")
        );
    }

    #[test]
    fn a_single_boolean_matches_the_bytes_glib_produces() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset
            .set("tap-to-click", ChangeValue::Boolean(false))
            .unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            format!("{PATH}7461702d746f2d636c69636b0000000000006200353d")
        );
    }

    #[test]
    fn a_reset_serializes_as_nothing_rather_than_as_an_empty_value() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset.reset("speed").unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            format!("{PATH}73706565640000002e31")
        );
    }

    #[test]
    fn a_mixed_change_set_matches_the_bytes_glib_produces() {
        // Four entries push the change set past 255 bytes, so every framing
        // offset in it widens to two bytes. That is the boundary an encoder
        // that assumed one byte falls off.
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset
            .set("natural-scroll", ChangeValue::Boolean(true))
            .unwrap();
        changeset.set("speed", ChangeValue::Double(-0.25)).unwrap();
        changeset
            .set("click-method", ChangeValue::Text("fingers".to_string()))
            .unwrap();
        changeset.reset("tap-to-click").unwrap();

        let bytes = changeset.serialise();
        assert_eq!(bytes.len(), 265);
        assert_eq!(
            hex(&bytes),
            "2f6f72672f676e6f6d652f6465736b746f702f7065726970686572616c732f746f7563687061642f\
636c69636b2d6d6574686f640000000066696e676572730000730035000000002f6f72672f676e6f\
6d652f6465736b746f702f7065726970686572616c732f746f7563687061642f6e61747572616c2d\
7363726f6c6c000001006200370000002f6f72672f676e6f6d652f6465736b746f702f7065726970\
686572616c732f746f7563687061642f7370656564000000000000000000d0bf0064002e00000000\
2f6f72672f676e6f6d652f6465736b746f702f7065726970686572616c732f746f7563687061642f\
7461702d746f2d636c69636b000000003544008500c4000101"
        );
    }

    #[test]
    fn an_empty_change_set_serializes_to_no_bytes_at_all() {
        let changeset = Changeset::new(TOUCHPAD).unwrap();
        assert!(changeset.is_empty());
        assert!(changeset.serialise().is_empty());
    }

    #[test]
    fn entries_serialize_in_key_order_however_they_were_added() {
        let mut forwards = Changeset::new(TOUCHPAD).unwrap();
        forwards.set("a-key", ChangeValue::Boolean(true)).unwrap();
        forwards.set("z-key", ChangeValue::Boolean(false)).unwrap();

        let mut backwards = Changeset::new(TOUCHPAD).unwrap();
        backwards.set("z-key", ChangeValue::Boolean(false)).unwrap();
        backwards.set("a-key", ChangeValue::Boolean(true)).unwrap();

        assert_eq!(forwards.serialise(), backwards.serialise());
    }

    #[test]
    fn a_later_write_of_the_same_key_replaces_the_earlier_one() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset.set("speed", ChangeValue::Double(0.1)).unwrap();
        changeset.reset("speed").unwrap();
        assert_eq!(changeset.len(), 1);
        assert_eq!(
            changeset.serialise(),
            Changeset::new(TOUCHPAD)
                .map(|mut other| {
                    other.reset("speed").unwrap();
                    other.serialise()
                })
                .unwrap()
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_dconf_directory_is_refused() {
        assert!(matches!(
            Changeset::new("org/gnome/"),
            Err(ChangesetError::BadPrefix(_))
        ));
        assert!(matches!(
            Changeset::new("/org/gnome"),
            Err(ChangesetError::BadPrefix(_))
        ));
    }

    #[test]
    fn a_key_that_could_escape_the_prefix_is_refused() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        assert!(matches!(
            changeset.set("../../escape", ChangeValue::Boolean(true)),
            Err(ChangesetError::BadKey(_))
        ));
        assert!(matches!(
            changeset.reset(""),
            Err(ChangesetError::BadKey(_))
        ));
        assert!(changeset.is_empty());
    }

    #[test]
    fn a_value_that_cannot_be_encoded_is_refused_rather_than_truncated() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        assert_eq!(
            changeset.set("click-method", ChangeValue::Text("fin\0gers".to_string())),
            Err(ChangesetError::NulInValue)
        );
        // NaN is not equal to itself, so the refusal is matched rather than
        // compared — which is the whole reason it has to be refused.
        assert!(matches!(
            changeset.set("speed", ChangeValue::Double(f64::NAN)),
            Err(ChangesetError::NotFinite(_))
        ));
        assert!(matches!(
            changeset.set("speed", ChangeValue::Double(f64::INFINITY)),
            Err(ChangesetError::NotFinite(_))
        ));
        assert!(changeset.is_empty());
    }

    #[test]
    fn a_change_set_large_enough_to_need_wider_offsets_still_encodes() {
        // Forty entries take the change set past 2 KiB. GLib produces 2,317
        // bytes for this set, ending in the last four two-byte offsets.
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        for index in 0..40 {
            changeset
                .set(
                    &format!("key-{index:03}"),
                    ChangeValue::Boolean(index % 2 == 0),
                )
                .unwrap();
        }
        let bytes = changeset.serialise();
        assert_eq!(bytes.len(), 2_317);
        assert_eq!(hex(&bytes[bytes.len() - 8..]), "15084d088508bd08");
    }

    #[test]
    fn the_paths_a_change_set_would_write_are_reportable_before_it_is_sent() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        changeset.set("speed", ChangeValue::Double(0.0)).unwrap();
        changeset.reset("tap-to-click").unwrap();
        assert_eq!(
            changeset.paths(),
            vec![
                "/org/gnome/desktop/peripherals/touchpad/speed".to_string(),
                "/org/gnome/desktop/peripherals/touchpad/tap-to-click".to_string(),
            ]
        );
    }

    // The string-array encodings below were produced the same way, with
    // `GLib.Variant('as', [...])` as the value, on GLib 2.80.

    const CLOSE: &str = "/org/gnome/desktop/wm/keybindings/close";
    const CLOSE_HEX: &str =
        "2f6f72672f676e6f6d652f6465736b746f702f776d2f6b657962696e64696e67732f636c6f7365";

    fn text_list(values: &[&str]) -> ChangeValue {
        ChangeValue::TextList(values.iter().map(|value| value.to_string()).collect())
    }

    #[test]
    fn a_string_array_matches_the_bytes_glib_produces() {
        let mut changeset = Changeset::new("/").unwrap();
        changeset
            .set_path(CLOSE, text_list(&["<Super>q", "<Alt>F4"]))
            .unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            format!("{CLOSE_HEX}003c53757065723e71003c416c743e4634000911006173002840")
        );
    }

    #[test]
    fn an_empty_string_array_is_a_value_rather_than_a_reset() {
        // `@as []` is how GNOME disables a keybinding, so an empty list has to
        // survive as a value: a reset would put the default binding back.
        let mut changeset = Changeset::new("/").unwrap();
        changeset.set_path(CLOSE, text_list(&[])).unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            format!("{CLOSE_HEX}0000617300282d")
        );
    }

    #[test]
    fn a_string_array_long_enough_to_widen_its_own_offsets_matches_glib() {
        // Twenty entries take the array itself past 255 bytes, so the offsets
        // inside it widen to two bytes as well as the ones around it.
        let values: Vec<String> = (1..=20)
            .map(|index| format!("<Super><Shift>F{index}"))
            .collect();
        let mut changeset = Changeset::new("/").unwrap();
        changeset
            .set_path(CLOSE, ChangeValue::TextList(values))
            .unwrap();
        let bytes = changeset.serialise();
        assert_eq!(bytes.len(), 439);
        assert_eq!(
            hex(&bytes[bytes.len() - 48..]),
            "110022003300440055006600770088009900ab00bd00cf00e100f3000501170129013b014d015f01\
006173002800b501"
        );
    }

    #[test]
    fn one_change_set_can_span_several_directories_and_matches_glib() {
        // A declaration may name keys in different directories, and they are
        // written in one call so the service applies all of them or none.
        let mut changeset = Changeset::new("/").unwrap();
        changeset
            .set_path(
                "/org/gnome/settings-daemon/plugins/media-keys/home",
                ChangeValue::Boolean(false),
            )
            .unwrap();
        changeset
            .reset_path("/org/gnome/settings-daemon/plugins/media-keys/home")
            .unwrap();
        changeset.set_path(CLOSE, text_list(&["<Super>q"])).unwrap();
        changeset
            .set_path(
                "/org/gnome/nautilus/preferences/show-image-thumbnails",
                ChangeValue::Text("never".to_string()),
            )
            .unwrap();
        changeset
            .set_path(
                "/org/gnome/desktop/interface/enable-animations",
                ChangeValue::Boolean(true),
            )
            .unwrap();
        assert_eq!(
            hex(&changeset.serialise()),
            "2f6f72672f676e6f6d652f6465736b746f702f696e746572666163652f656e61626c652d616e696d\
6174696f6e730000010062002f0000002f6f72672f676e6f6d652f6465736b746f702f776d2f6b65\
7962696e64696e67732f636c6f7365003c53757065723e7100090061730028002f6f72672f676e6f\
6d652f6e617574696c75732f707265666572656e6365732f73686f772d696d6167652d7468756d62\
6e61696c730000006e6576657200007300360000000000002f6f72672f676e6f6d652f7365747469\
6e67732d6461656d6f6e2f706c7567696e732f6d656469612d6b6579732f686f6d65000000000000\
33356fb2f1"
        );
    }

    #[test]
    fn a_full_path_outside_the_prefix_or_not_a_key_is_refused() {
        let mut changeset = Changeset::new(TOUCHPAD).unwrap();
        for path in [
            "/org/gnome/desktop/wm/keybindings/close",
            "/org/gnome/desktop/peripherals/touchpad/",
            "/org/gnome/desktop/peripherals/touchpad//speed",
            "org/gnome/desktop/peripherals/touchpad/speed",
            "/org/gnome/desktop/peripherals/touchpad/sp\0eed",
        ] {
            assert!(
                matches!(
                    changeset.set_path(path, ChangeValue::Boolean(true)),
                    Err(ChangesetError::BadPath(_))
                ),
                "{path:?} was accepted"
            );
            assert!(matches!(
                changeset.reset_path(path),
                Err(ChangesetError::BadPath(_))
            ));
        }
        assert!(changeset.is_empty());
        changeset
            .set_path(
                "/org/gnome/desktop/peripherals/touchpad/speed",
                ChangeValue::Double(0.35),
            )
            .unwrap();
        let mut relative = Changeset::new(TOUCHPAD).unwrap();
        relative.set("speed", ChangeValue::Double(0.35)).unwrap();
        assert_eq!(changeset.serialise(), relative.serialise());
    }

    #[test]
    fn a_string_array_holding_a_nul_byte_is_refused() {
        let mut changeset = Changeset::new("/").unwrap();
        assert_eq!(
            changeset.set_path(CLOSE, text_list(&["<Super>q", "<Alt>\0F4"])),
            Err(ChangesetError::NulInValue)
        );
        assert!(changeset.is_empty());
    }
}

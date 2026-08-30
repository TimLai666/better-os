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

use std::collections::BTreeMap;

use thiserror::Error;

/// A value a change set can write. These are the three GVariant types the GNOME
/// touchpad and mouse schemas actually use.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeValue {
    Boolean(bool),
    Double(f64),
    Text(String),
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ChangesetError {
    #[error("a dconf path prefix must start and end with '/', not {0:?}")]
    BadPrefix(String),
    #[error("a dconf key name must not be empty or contain '/' or a nul byte, unlike {0:?}")]
    BadKey(String),
    #[error("a dconf string value must not contain a nul byte")]
    NulInValue,
    #[error("a dconf value must be a real number, not {0}")]
    NotFinite(f64),
}

/// A set of changes under one path prefix.
///
/// Entries are kept sorted, which is what the dconf client's own tree-backed
/// change set produces, so the same set of changes always serializes to the
/// same bytes.
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
        self.entries
            .keys()
            .map(|key| format!("{}{key}", self.prefix))
            .collect()
    }

    pub fn set(&mut self, key: &str, value: ChangeValue) -> Result<(), ChangesetError> {
        check_key(key)?;
        match &value {
            ChangeValue::Text(text) if text.contains('\0') => {
                return Err(ChangesetError::NulInValue);
            }
            ChangeValue::Double(number) if !number.is_finite() => {
                return Err(ChangesetError::NotFinite(*number));
            }
            _ => {}
        }
        self.entries.insert(key.to_string(), Some(value));
        Ok(())
    }

    /// Removes the key, so the session's own default applies again.
    pub fn reset(&mut self, key: &str) -> Result<(), ChangesetError> {
        check_key(key)?;
        self.entries.insert(key.to_string(), None);
        Ok(())
    }

    /// The bytes `ca.desrt.dconf.Writer.Change` takes.
    ///
    /// An empty change set serializes to no bytes at all, which is what an
    /// empty GVariant array is.
    pub fn serialise(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let mut ends = Vec::with_capacity(self.entries.len());
        for (key, value) in &self.entries {
            pad_to_eight(&mut data);
            data.extend_from_slice(&entry_bytes(
                &format!("{}{key}", self.prefix),
                value.as_ref(),
            ));
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
    }
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
}

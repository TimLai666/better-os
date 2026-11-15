//! Better OS's udev rules file for removable drives turned off (ADR 0032):
//! one line per drive, setting UDisks2's `UDISKS_AUTO` hint to 0 so the
//! desktop does not mount it when it arrives. The whole file is Better OS's;
//! one that holds anything else is refused.

use crate::ids::valid_uuid;

/// Where the file lives.
pub const RULES_PATH: &str = "/etc/udev/rules.d/90-better-os-drives.rules";

const HEADER: &str = "# Removable drives Better Manager was asked not to mount automatically.\n\
# Written by better-manager-daemon; removed with it. Do not edit.\n";

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RulesError {
    #[error("the file is not text")]
    NotText,
    #[error("line {0} is not a line Better OS wrote")]
    Foreign(usize),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rules {
    uuids: Vec<String>,
}

fn rule(uuid: &str) -> String {
    format!("ENV{{ID_FS_UUID}}==\"{uuid}\", ENV{{UDISKS_AUTO}}=\"0\"")
}

impl Rules {
    pub fn parse(text: &str) -> Result<Self, RulesError> {
        if text.contains('\0') {
            return Err(RulesError::NotText);
        }
        let mut uuids = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let uuid = line
                .strip_prefix("ENV{ID_FS_UUID}==\"")
                .and_then(|rest| rest.split_once('"'))
                .map(|(uuid, _)| uuid)
                .filter(|uuid| valid_uuid(uuid) && rule(uuid) == line)
                .ok_or(RulesError::Foreign(index + 1))?;
            if !uuids.iter().any(|known| known == uuid) {
                uuids.push(uuid.to_string());
            }
        }
        Ok(Self { uuids })
    }

    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, RulesError> {
        std::str::from_utf8(bytes)
            .map_err(|_| RulesError::NotText)
            .and_then(Self::parse)
    }

    pub fn is_off(&self, uuid: &str) -> bool {
        self.uuids
            .iter()
            .any(|known| known.eq_ignore_ascii_case(uuid))
    }

    /// Turns a drive off (`true`: a line is added) or on; whether anything
    /// changed. A UUID [`valid_uuid`] refuses changes nothing.
    pub fn set_off(&mut self, uuid: &str, off: bool) -> bool {
        if !valid_uuid(uuid) {
            return false;
        }
        let known = self.is_off(uuid);
        if off && !known {
            self.uuids.push(uuid.to_string());
        } else if !off && known {
            self.uuids.retain(|k| !k.eq_ignore_ascii_case(uuid));
        }
        known != off
    }

    pub fn is_empty(&self) -> bool {
        self.uuids.is_empty()
    }

    /// The file, or `None` when it should not exist.
    pub fn render(&self) -> Option<String> {
        if self.uuids.is_empty() {
            return None;
        }
        let mut text = HEADER.to_string();
        for uuid in &self.uuids {
            text.push_str(&rule(uuid));
            text.push('\n');
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_adds_a_line_and_on_takes_it_and_the_file_away() {
        let mut rules = Rules::default();
        assert!(rules.set_off("ABCD-1234", true));
        assert!(!rules.set_off("abcd-1234", true));
        let text = rules.render().unwrap();
        assert!(text.ends_with("ENV{ID_FS_UUID}==\"ABCD-1234\", ENV{UDISKS_AUTO}=\"0\"\n"));
        let mut back = Rules::parse(&text).unwrap();
        assert!(back.is_off("ABCD-1234"));
        assert!(back.set_off("ABCD-1234", false));
        assert_eq!(back.render(), None);
        assert!(!back.set_off("x\"; RUN+=\"/bin/sh", true));
    }

    #[test]
    fn a_file_with_anything_else_is_refused() {
        for (text, error) in [
            (
                "ENV{ID_FS_UUID}==\"AB\", RUN+=\"/bin/x\"\n",
                RulesError::Foreign(1),
            ),
            (
                "# c\nKERNEL==\"sd*\", ENV{UDISKS_AUTO}=\"0\"\n",
                RulesError::Foreign(2),
            ),
            ("\0", RulesError::NotText),
        ] {
            assert_eq!(Rules::parse(text), Err(error));
        }
    }
}

//! The GLib key file format Flatpak's metadata and override files use, read
//! and edited line by line.
//!
//! Editing keeps every line it does not touch exactly as it was: comments,
//! blank lines, groups and keys in their order, values it does not
//! understand. That is what lets an undo take out the keys a fix wrote and
//! leave anything a person added to the same file by hand.

/// One line of a key file.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Line {
    /// `[Group]`.
    Group(String),
    /// `key=value`, with the spacing around `=` dropped as GLib drops it.
    Entry { key: String, value: String },
    /// A comment, a blank line, or anything else, kept verbatim.
    Other(String),
}

/// A key file, as its lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyFile {
    lines: Vec<Line>,
}

impl KeyFile {
    pub fn parse(text: &str) -> Self {
        let lines = text
            .lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
                    Line::Group(trimmed[1..trimmed.len() - 1].to_string())
                } else if trimmed.starts_with('#') || trimmed.is_empty() {
                    Line::Other(line.to_string())
                } else if let Some((key, value)) = line.split_once('=') {
                    Line::Entry {
                        key: key.trim().to_string(),
                        value: value.trim_start().to_string(),
                    }
                } else {
                    Line::Other(line.to_string())
                }
            })
            .collect();
        Self { lines }
    }

    /// The text, one line each, ending with a newline when there is any.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            match line {
                Line::Group(name) => {
                    text.push('[');
                    text.push_str(name);
                    text.push(']');
                }
                Line::Entry { key, value } => {
                    text.push_str(key);
                    text.push('=');
                    text.push_str(value);
                }
                Line::Other(other) => text.push_str(other),
            }
            text.push('\n');
        }
        text
    }

    /// The index range of a group's lines after its header: from the line
    /// after `[group]` to the next header. The first such group when a file
    /// repeats one, as GLib merges repeats into the first.
    fn group_range(&self, group: &str) -> Option<(usize, usize)> {
        let header = self
            .lines
            .iter()
            .position(|line| matches!(line, Line::Group(name) if name == group))?;
        let end = self.lines[header + 1..]
            .iter()
            .position(|line| matches!(line, Line::Group(_)))
            .map_or(self.lines.len(), |offset| header + 1 + offset);
        Some((header + 1, end))
    }

    /// Whether the file has the group.
    pub fn has_group(&self, group: &str) -> bool {
        self.group_range(group).is_some()
    }

    /// A key's value; the last one when a key is repeated, as GLib reads it.
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        let (start, end) = self.group_range(group)?;
        self.lines[start..end]
            .iter()
            .rev()
            .find_map(|line| match line {
                Line::Entry { key: k, value } if k == key => Some(value.as_str()),
                _ => None,
            })
    }

    /// Every key of a group, in order.
    pub fn keys(&self, group: &str) -> Vec<&str> {
        let Some((start, end)) = self.group_range(group) else {
            return Vec::new();
        };
        self.lines[start..end]
            .iter()
            .filter_map(|line| match line {
                Line::Entry { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Sets a key: in place when it is there (dropping repeats of it), at
    /// the end of its group otherwise, and in a new group at the end of the
    /// file when the group is missing.
    pub fn set(&mut self, group: &str, key: &str, value: &str) {
        let Some((start, end)) = self.group_range(group) else {
            if self
                .lines
                .last()
                .is_some_and(|line| !matches!(line, Line::Other(text) if text.trim().is_empty()))
            {
                self.lines.push(Line::Other(String::new()));
            }
            self.lines.push(Line::Group(group.to_string()));
            self.lines.push(Line::Entry {
                key: key.to_string(),
                value: value.to_string(),
            });
            return;
        };
        let positions: Vec<usize> = (start..end)
            .filter(|&index| matches!(&self.lines[index], Line::Entry { key: k, .. } if k == key))
            .collect();
        match positions.split_last() {
            Some((&last, earlier)) => {
                self.lines[last] = Line::Entry {
                    key: key.to_string(),
                    value: value.to_string(),
                };
                for &index in earlier.iter().rev() {
                    self.lines.remove(index);
                }
            }
            None => {
                // After the group's last entry, so blank lines that separate
                // it from the next group stay where they were.
                let insert_at = (start..end)
                    .rev()
                    .find(|&index| matches!(self.lines[index], Line::Entry { .. }))
                    .map_or(start, |index| index + 1);
                self.lines.insert(
                    insert_at,
                    Line::Entry {
                        key: key.to_string(),
                        value: value.to_string(),
                    },
                );
            }
        }
    }

    /// Removes a key and any repeats of it.
    pub fn remove(&mut self, group: &str, key: &str) {
        let Some((start, end)) = self.group_range(group) else {
            return;
        };
        for index in (start..end).rev() {
            if matches!(&self.lines[index], Line::Entry { key: k, .. } if k == key) {
                self.lines.remove(index);
            }
        }
    }

    /// Removes a group's header when nothing but blank lines follow it, and
    /// the blank line before it that `set` added. A group holding a comment
    /// or a key someone wrote stays.
    pub fn remove_group_if_empty(&mut self, group: &str) {
        let Some((start, end)) = self.group_range(group) else {
            return;
        };
        let empty = self.lines[start..end]
            .iter()
            .all(|line| matches!(line, Line::Other(text) if text.trim().is_empty()));
        if !empty {
            return;
        }
        let header = start - 1;
        self.lines.drain(header..end);
        if header > 0
            && matches!(&self.lines[header - 1], Line::Other(text) if text.trim().is_empty())
            && (header == self.lines.len()
                || matches!(self.lines.get(header), Some(Line::Group(_))))
        {
            self.lines.remove(header - 1);
        }
    }

    /// Whether the file says nothing: no key and no comment.
    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| match line {
            Line::Group(_) => true,
            Line::Entry { .. } => false,
            Line::Other(text) => text.trim().is_empty(),
        })
    }
}

/// A list value's items: split on `;` not escaped as `\;`, without the
/// empty item after the trailing separator.
pub fn list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' => items.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        items.push(current);
    }
    items.retain(|item| !item.is_empty());
    items
}

/// A list value written as GLib writes one: every item followed by `;`.
pub fn join_list(items: &[String]) -> String {
    let mut value = String::new();
    for item in items {
        value.push_str(item);
        value.push(';');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERRIDE: &str = "\
# my own note
[Context]
filesystems=~/Projects;
sockets=wayland;

[Environment]
MY_VAR=1
";

    #[test]
    fn an_untouched_file_reads_back_byte_for_byte() {
        assert_eq!(KeyFile::parse(OVERRIDE).to_text(), OVERRIDE);
        assert_eq!(KeyFile::parse("").to_text(), "");
    }

    #[test]
    fn get_reads_the_last_repeat_in_its_group_only() {
        let file = KeyFile::parse("[A]\nk=1\nk=2\n[B]\nk=3\n");
        assert_eq!(file.get("A", "k"), Some("2"));
        assert_eq!(file.get("B", "k"), Some("3"));
        assert_eq!(file.get("C", "k"), None);
        assert_eq!(file.keys("A"), ["k", "k"]);
    }

    #[test]
    fn set_replaces_in_place_appends_to_the_group_or_adds_a_group() {
        let mut file = KeyFile::parse(OVERRIDE);
        file.set("Context", "sockets", "wayland;x11;");
        file.set("Environment", "NEW", "x");
        file.set("Session Bus Policy", "org.example", "talk");
        assert_eq!(
            file.to_text(),
            "\
# my own note
[Context]
filesystems=~/Projects;
sockets=wayland;x11;

[Environment]
MY_VAR=1
NEW=x

[Session Bus Policy]
org.example=talk
"
        );
    }

    #[test]
    fn a_new_group_in_an_empty_file_has_no_leading_blank_line() {
        let mut file = KeyFile::parse("");
        file.set("Context", "sockets", "x11;");
        assert_eq!(file.to_text(), "[Context]\nsockets=x11;\n");
    }

    #[test]
    fn set_drops_repeats_of_the_key() {
        let mut file = KeyFile::parse("[A]\nk=1\nother=2\nk=3\n");
        file.set("A", "k", "9");
        assert_eq!(file.to_text(), "[A]\nother=2\nk=9\n");
    }

    #[test]
    fn removing_the_last_key_and_then_the_empty_group_restores_the_file() {
        let mut file = KeyFile::parse(OVERRIDE);
        file.set("Extra", "k", "v");
        file.remove("Extra", "k");
        file.remove_group_if_empty("Extra");
        assert_eq!(file.to_text(), OVERRIDE);
    }

    #[test]
    fn a_group_with_a_comment_or_a_key_is_not_empty() {
        let mut file = KeyFile::parse("[A]\n# keep\n[B]\nk=1\n");
        file.remove_group_if_empty("A");
        file.remove_group_if_empty("B");
        assert_eq!(file.to_text(), "[A]\n# keep\n[B]\nk=1\n");
    }

    #[test]
    fn blank_means_no_key_and_no_comment() {
        assert!(KeyFile::parse("").is_blank());
        assert!(KeyFile::parse("[Context]\n\n").is_blank());
        assert!(!KeyFile::parse("# note\n").is_blank());
        assert!(!KeyFile::parse("[Context]\nk=v\n").is_blank());
    }

    #[test]
    fn lists_split_on_unescaped_separators() {
        assert_eq!(list("x11;!fallback-x11;"), ["x11", "!fallback-x11"]);
        assert_eq!(list("a;b"), ["a", "b"]);
        assert_eq!(list("a\\;b;c;"), ["a\\;b", "c"]);
        assert!(list("").is_empty());
        assert!(list(";;").is_empty());
        assert_eq!(
            join_list(&["x11".to_string(), "!fallback-x11".to_string()]),
            "x11;!fallback-x11;"
        );
    }
}

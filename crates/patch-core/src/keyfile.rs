//! The GLib key file format Flatpak's metadata and override files use, read
//! as GLib reads it and edited in place.
//!
//! Editing touches only the lines of the keys it is asked to change, and
//! keeps every other byte as it was: comments, blank lines, spacing around
//! `=`, repeated keys, repeated groups, line endings (`\r\n` included), and a
//! last line with no newline. That is what lets an undo take out the keys a
//! fix wrote and leave anything a person wrote in the same file alone.
//!
//! Reading follows GLib: a group that appears twice is one group, and when a
//! key appears more than once (in one group or across its repeats) the last
//! one wins. Writing a key therefore changes that last occurrence, which is
//! the one that takes effect.

/// What a line is.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Kind {
    /// `[Group]`.
    Group(String),
    /// `key=value`; the value starts at this byte of the line.
    Entry { key: String, value_at: usize },
    /// A comment, a blank line, or anything else.
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Line {
    /// The line without its ending.
    text: String,
    /// `"\n"`, `"\r\n"`, or `""` for a last line with none.
    ending: &'static str,
    kind: Kind,
}

impl Line {
    fn classify(text: &str) -> Kind {
        let trimmed = text.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
            return Kind::Group(trimmed[1..trimmed.len() - 1].to_string());
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Kind::Other;
        }
        match text.find('=') {
            Some(at) => {
                let rest = &text[at + 1..];
                let skipped = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                Kind::Entry {
                    key: text[..at].trim().to_string(),
                    value_at: at + 1 + skipped,
                }
            }
            None => Kind::Other,
        }
    }

    fn new(text: String, ending: &'static str) -> Self {
        let kind = Self::classify(&text);
        Self { text, ending, kind }
    }

    fn value(&self) -> Option<&str> {
        match &self.kind {
            Kind::Entry { value_at, .. } => Some(&self.text[*value_at..]),
            _ => None,
        }
    }

    fn is_entry(&self, wanted: &str) -> bool {
        matches!(&self.kind, Kind::Entry { key, .. } if key == wanted)
    }

    fn is_blank(&self) -> bool {
        matches!(self.kind, Kind::Other) && self.text.trim().is_empty()
    }
}

/// A key file, as its lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyFile {
    lines: Vec<Line>,
}

impl KeyFile {
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let (line, ending, next) = match rest.find('\n') {
                Some(at) if at > 0 && rest.as_bytes()[at - 1] == b'\r' => {
                    (&rest[..at - 1], "\r\n", &rest[at + 1..])
                }
                Some(at) => (&rest[..at], "\n", &rest[at + 1..]),
                None => (rest, "", ""),
            };
            lines.push(Line::new(line.to_string(), ending));
            rest = next;
        }
        Self { lines }
    }

    /// The text, every untouched line exactly as it was read.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.text);
            text.push_str(line.ending);
        }
        text
    }

    /// The line ending new lines get: the file's own, `\r\n` when it uses
    /// that, `\n` otherwise.
    fn ending(&self) -> &'static str {
        if self.lines.iter().any(|line| line.ending == "\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// The group each line is in, `None` before the first header.
    fn groups_of_lines(&self) -> Vec<Option<&str>> {
        let mut current = None;
        self.lines
            .iter()
            .map(|line| {
                if let Kind::Group(name) = &line.kind {
                    current = Some(name.as_str());
                    // The header itself belongs to no group's body.
                    return None;
                }
                current
            })
            .collect()
    }

    /// The index ranges of each repeat of a group's body, in order.
    fn sections(&self, group: &str) -> Vec<(usize, usize)> {
        let mut sections = Vec::new();
        let mut index = 0;
        while index < self.lines.len() {
            if matches!(&self.lines[index].kind, Kind::Group(name) if name == group) {
                let start = index + 1;
                let end = self.lines[start..]
                    .iter()
                    .position(|line| matches!(line.kind, Kind::Group(_)))
                    .map_or(self.lines.len(), |offset| start + offset);
                sections.push((start, end));
                index = end;
            } else {
                index += 1;
            }
        }
        sections
    }

    /// The indices of every line of a key in a group, in file order.
    fn entries(&self, group: &str, key: &str) -> Vec<usize> {
        let groups = self.groups_of_lines();
        (0..self.lines.len())
            .filter(|&index| groups[index] == Some(group) && self.lines[index].is_entry(key))
            .collect()
    }

    /// Whether the file has the group.
    pub fn has_group(&self, group: &str) -> bool {
        !self.sections(group).is_empty()
    }

    /// A key's value as GLib reads it: the last occurrence.
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.entries(group, key)
            .last()
            .and_then(|&index| self.lines[index].value())
    }

    /// Every key of a group, once each, in the order they first appear.
    pub fn keys(&self, group: &str) -> Vec<&str> {
        let groups = self.groups_of_lines();
        let mut keys: Vec<&str> = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            if groups[index] != Some(group) {
                continue;
            }
            if let Kind::Entry { key, .. } = &line.kind
                && !keys.contains(&key.as_str())
            {
                keys.push(key);
            }
        }
        keys
    }

    /// Makes the end of the file a complete line, before something is added
    /// after it.
    fn end_last_line(&mut self) {
        let ending = self.ending();
        if let Some(last) = self.lines.last_mut()
            && last.ending.is_empty()
        {
            last.ending = ending;
        }
    }

    /// Sets a key. The occurrence that takes effect (the last) gets the new
    /// value, keeping the key and the spacing before the value as written;
    /// earlier repeats are left. A key that is not there is added after the
    /// last entry of the group's last repeat, and a missing group is added at
    /// the end of the file.
    pub fn set(&mut self, group: &str, key: &str, value: &str) {
        if let Some(&index) = self.entries(group, key).last() {
            let line = &mut self.lines[index];
            if let Kind::Entry { value_at, .. } = line.kind {
                line.text.truncate(value_at);
                line.text.push_str(value);
            }
            return;
        }
        let ending = self.ending();
        let entry = Line::new(format!("{key}={value}"), ending);
        match self.sections(group).last() {
            Some(&(start, end)) => {
                let insert_at = (start..end)
                    .rev()
                    .find(|&index| matches!(self.lines[index].kind, Kind::Entry { .. }))
                    .map_or(start, |index| index + 1);
                if insert_at == self.lines.len() {
                    self.end_last_line();
                }
                self.lines.insert(insert_at, entry);
            }
            None => {
                self.end_last_line();
                if self.lines.last().is_some_and(|line| !line.is_blank()) {
                    self.lines.push(Line::new(String::new(), ending));
                }
                self.lines.push(Line::new(format!("[{group}]"), ending));
                self.lines.push(entry);
            }
        }
    }

    /// Removes every line of a key in a group.
    pub fn remove(&mut self, group: &str, key: &str) {
        for index in self.entries(group, key).into_iter().rev() {
            self.lines.remove(index);
        }
    }

    /// Removes each repeat of a group that has nothing but blank lines under
    /// its header, and the blank line before it when that one only separated
    /// it from what came before. A repeat holding a comment or a key stays.
    pub fn remove_group_if_empty(&mut self, group: &str) {
        for (start, end) in self.sections(group).into_iter().rev() {
            if !self.lines[start..end].iter().all(Line::is_blank) {
                continue;
            }
            let header = start - 1;
            self.lines.drain(header..end);
            let at_end_or_group = header == self.lines.len()
                || matches!(
                    self.lines.get(header).map(|line| &line.kind),
                    Some(Kind::Group(_))
                );
            if header > 0 && self.lines[header - 1].is_blank() && at_end_or_group {
                self.lines.remove(header - 1);
            }
        }
    }

    /// Whether the file says nothing: no key and no comment.
    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| match &line.kind {
            Kind::Group(_) => true,
            Kind::Entry { .. } => false,
            Kind::Other => line.text.trim().is_empty(),
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
    items.retain(|item| !item.trim().is_empty());
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
        for text in [
            OVERRIDE,
            "",
            "[A]\r\nk = v\r\n",
            "[A]\nk=v",
            "[A]\nk=1\nk=2\n[B]\n[A]\nk=3\n",
            "  [A]  \n\tk\t=\tv \n",
        ] {
            assert_eq!(KeyFile::parse(text).to_text(), text);
        }
    }

    #[test]
    fn get_reads_as_glib_does_the_last_occurrence_across_repeated_groups() {
        let file = KeyFile::parse("[A]\nk=1\nk=2\n[B]\nk=3\n[A]\nk=4\nj=5\n");
        assert_eq!(file.get("A", "k"), Some("4"));
        assert_eq!(file.get("A", "j"), Some("5"));
        assert_eq!(file.get("B", "k"), Some("3"));
        assert_eq!(file.get("C", "k"), None);
        assert_eq!(file.keys("A"), ["k", "j"]);
        assert_eq!(
            KeyFile::parse("[A]\nk = v x \n").get("A", "k"),
            Some("v x ")
        );
    }

    #[test]
    fn set_changes_only_the_line_that_takes_effect() {
        let mut file = KeyFile::parse("[A]\nk=1\nother=2\nk = 3\n[A]\nk=4\n");
        file.set("A", "k", "9");
        assert_eq!(file.to_text(), "[A]\nk=1\nother=2\nk = 3\n[A]\nk=9\n");
        let mut spaced = KeyFile::parse("[A]\r\nk = old\r\n");
        spaced.set("A", "k", "new");
        assert_eq!(spaced.to_text(), "[A]\r\nk = new\r\n");
    }

    #[test]
    fn set_appends_to_the_last_repeat_or_adds_a_group() {
        let mut file = KeyFile::parse(OVERRIDE);
        file.set("Context", "devices", "dri;");
        file.set("Environment", "NEW", "x");
        file.set("Session Bus Policy", "org.example", "talk");
        assert_eq!(
            file.to_text(),
            "\
# my own note
[Context]
filesystems=~/Projects;
sockets=wayland;
devices=dri;

[Environment]
MY_VAR=1
NEW=x

[Session Bus Policy]
org.example=talk
"
        );
        let mut repeated = KeyFile::parse("[A]\nx=1\n[B]\ny=2\n[A]\nz=3\n");
        repeated.set("A", "w", "4");
        assert_eq!(repeated.to_text(), "[A]\nx=1\n[B]\ny=2\n[A]\nz=3\nw=4\n");
    }

    #[test]
    fn new_lines_use_the_file_s_line_ending_and_end_a_last_line_first() {
        let mut crlf = KeyFile::parse("[A]\r\nk=v\r\n");
        crlf.set("B", "j", "w");
        assert_eq!(crlf.to_text(), "[A]\r\nk=v\r\n\r\n[B]\r\nj=w\r\n");
        let mut open = KeyFile::parse("[A]\nk=v");
        open.set("A", "j", "w");
        assert_eq!(open.to_text(), "[A]\nk=v\nj=w\n");
        let mut empty = KeyFile::parse("");
        empty.set("Context", "sockets", "x11;");
        assert_eq!(empty.to_text(), "[Context]\nsockets=x11;\n");
    }

    #[test]
    fn removing_what_was_added_restores_the_file_byte_for_byte() {
        for original in [OVERRIDE, "[A]\r\nk = v\r\n# note\r\n", "[A]\nk=v"] {
            let mut file = KeyFile::parse(original);
            file.set("Extra", "k", "v");
            file.remove("Extra", "k");
            file.remove_group_if_empty("Extra");
            let restored = file.to_text();
            // A file with no final newline gets one when a group was added
            // after it; everything else is byte for byte.
            assert_eq!(
                restored.trim_end_matches('\n'),
                original.trim_end_matches('\n')
            );
            if original.ends_with('\n') {
                assert_eq!(restored, original);
            }
        }
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

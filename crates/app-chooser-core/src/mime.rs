//! Type relationships read from the installed `shared-mime-info` data.
//!
//! Better OS does not own a MIME database. It reads the two plain-text tables
//! that `shared-mime-info` already installs — `aliases` and `subclasses` — and
//! uses them only to answer "is this type another name for that one" and "what
//! more general types does this one inherit from". When the files are missing
//! the answer is "nothing is known", which makes an application's own declared
//! types the only evidence, rather than inventing a hierarchy.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use app_catalog_core::MimeType;

/// The type of the selected file, together with the more general types it
/// inherits from. Ranking consumes this instead of reaching for a database of
/// its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MimeResolution {
    /// The canonical form of the selected type, after alias resolution.
    pub primary: MimeType,
    /// The type as the caller named it, when an alias renamed it.
    pub requested: MimeType,
    /// More general types, nearest first. Never contains `primary`.
    pub ancestors: Vec<MimeType>,
}

impl MimeResolution {
    /// A resolution with no known relationships, which is what an absent
    /// `shared-mime-info` installation honestly produces.
    pub fn standalone(mime: MimeType) -> Self {
        Self {
            requested: mime.clone(),
            primary: mime,
            ancestors: Vec::new(),
        }
    }

    /// Whether `candidate` names the selected type itself, under either its
    /// canonical name or the name the caller used.
    pub fn is_primary(&self, candidate: &MimeType) -> bool {
        candidate == &self.primary || candidate == &self.requested
    }

    /// The position of `candidate` among the ancestors, nearest first.
    pub fn ancestor_distance(&self, candidate: &MimeType) -> Option<usize> {
        self.ancestors.iter().position(|mime| mime == candidate)
    }
}

/// One filename rule from `shared-mime-info`'s `globs2` table.
#[derive(Clone, Debug, Eq, PartialEq)]
struct GlobRule {
    weight: u32,
    mime: MimeType,
    pattern: GlobPattern,
    case_sensitive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GlobPattern {
    /// `*.rs`
    Suffix(String),
    /// `Makefile`
    Literal(String),
}

/// Alias and subclass relationships, loaded read-only.
#[derive(Clone, Debug, Default)]
pub struct MimeGraph {
    aliases: BTreeMap<MimeType, MimeType>,
    parents: BTreeMap<MimeType, Vec<MimeType>>,
    globs: Vec<GlobRule>,
}

impl MimeGraph {
    /// A graph that knows no relationships at all.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Reads `aliases` and `subclasses` from each `<dir>/mime` directory, in
    /// the order given. Missing or unreadable files are skipped: a chooser that
    /// refused to open because a data file was absent would be worse than one
    /// that only knows about declared types.
    pub fn from_data_dirs<I, P>(dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut graph = Self::default();
        for dir in dirs {
            let mime_dir = dir.as_ref().join("mime");
            if let Ok(text) = std::fs::read_to_string(mime_dir.join("aliases")) {
                graph.load_aliases(&text);
            }
            if let Ok(text) = std::fs::read_to_string(mime_dir.join("subclasses")) {
                graph.load_subclasses(&text);
            }
            if let Ok(text) = std::fs::read_to_string(mime_dir.join("globs2")) {
                graph.load_globs(&text);
            }
        }
        graph.globs.sort_by(|left, right| {
            right
                .weight
                .cmp(&left.weight)
                .then_with(|| right.specificity().cmp(&left.specificity()))
        });
        graph
    }

    /// The XDG data directories, user first, exactly as the shared catalog
    /// resolves them.
    pub fn from_env() -> Self {
        Self::from_data_dirs(data_dirs())
    }

    fn load_aliases(&mut self, text: &str) {
        for (from, to) in pairs(text) {
            // A later data directory must not override an earlier,
            // higher-priority one.
            self.aliases.entry(from).or_insert(to);
        }
    }

    fn load_subclasses(&mut self, text: &str) {
        for (child, parent) in pairs(text) {
            let parents = self.parents.entry(child).or_default();
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        }
    }

    /// Parses `globs2`, whose lines read `weight:mimetype:pattern[:flags]`.
    /// Only the two pattern shapes that carry real traffic are understood — a
    /// `*.extension` suffix and a literal filename. Anything else is skipped
    /// rather than half-matched.
    fn load_globs(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split(':');
            let Some(weight) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
                continue;
            };
            let Some(mime) = fields.next().and_then(MimeType::parse) else {
                continue;
            };
            let Some(pattern) = fields.next() else {
                continue;
            };
            let case_sensitive = fields.any(|flag| flag.trim() == "cs");
            let pattern = match pattern.strip_prefix("*.") {
                Some(suffix) if !suffix.contains(['*', '?', '[']) => {
                    GlobPattern::Suffix(format!(".{suffix}"))
                }
                Some(_) => continue,
                None if !pattern.contains(['*', '?', '[']) => {
                    GlobPattern::Literal(pattern.to_string())
                }
                None => continue,
            };
            self.globs.push(GlobRule {
                weight,
                mime,
                pattern,
                case_sensitive,
            });
        }
    }

    /// The type of a file, judged by its name alone. Content sniffing is not
    /// done here: the chooser's caller already knows what it selected, and this
    /// is the fallback for a caller that only has a name.
    ///
    /// Returns `None` when nothing matches, which the caller must handle rather
    /// than fill in with a guess.
    pub fn guess_from_file_name(&self, name: &str) -> Option<MimeType> {
        let lowered = name.to_lowercase();
        self.globs
            .iter()
            .find(|rule| {
                let candidate = if rule.case_sensitive {
                    name
                } else {
                    lowered.as_str()
                };
                let pattern_text = match &rule.pattern {
                    GlobPattern::Suffix(suffix) => suffix,
                    GlobPattern::Literal(literal) => literal,
                };
                let pattern_text = if rule.case_sensitive {
                    pattern_text.clone()
                } else {
                    pattern_text.to_lowercase()
                };
                match &rule.pattern {
                    GlobPattern::Suffix(_) => candidate.ends_with(&pattern_text),
                    GlobPattern::Literal(_) => candidate == pattern_text,
                }
            })
            .map(|rule| self.canonical(&rule.mime))
    }

    /// The canonical name for a type, following alias chains. A cycle in the
    /// data stops rather than hangs.
    pub fn canonical(&self, mime: &MimeType) -> MimeType {
        let mut seen = BTreeSet::new();
        let mut current = mime.clone();
        while let Some(next) = self.aliases.get(&current) {
            if !seen.insert(current.clone()) {
                break;
            }
            current = next.clone();
        }
        current
    }

    /// The more general types of `mime`, nearest first, breadth first, with
    /// each type reported once.
    pub fn ancestors(&self, mime: &MimeType) -> Vec<MimeType> {
        let start = self.canonical(mime);
        let mut seen = BTreeSet::from([start.clone()]);
        let mut queue = VecDeque::from([start]);
        let mut ancestors = Vec::new();
        while let Some(current) = queue.pop_front() {
            let Some(parents) = self.parents.get(&current) else {
                continue;
            };
            for parent in parents {
                let parent = self.canonical(parent);
                if seen.insert(parent.clone()) {
                    ancestors.push(parent.clone());
                    queue.push_back(parent);
                }
            }
        }
        ancestors
    }

    /// The full resolution ranking consumes.
    pub fn resolve(&self, mime: &MimeType) -> MimeResolution {
        let primary = self.canonical(mime);
        MimeResolution {
            ancestors: self.ancestors(&primary),
            requested: mime.clone(),
            primary,
        }
    }
}

impl GlobRule {
    /// A longer pattern is a more specific match, so `.tar.gz` beats `.gz`.
    fn specificity(&self) -> usize {
        match &self.pattern {
            GlobPattern::Suffix(suffix) => suffix.len(),
            GlobPattern::Literal(literal) => literal.len() + 1,
        }
    }
}

/// Parses the two-column, whitespace-separated form both data files use.
/// Comments, blank lines, and anything that is not a pair of valid MIME types
/// are skipped rather than guessed at.
fn pairs(text: &str) -> Vec<(MimeType, MimeType)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut parts = line.split_whitespace();
            let left = MimeType::parse(parts.next()?)?;
            let right = MimeType::parse(parts.next()?)?;
            Some((left, right))
        })
        .collect()
}

/// `XDG_DATA_HOME` then `XDG_DATA_DIRS`, with the specification's defaults.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var("XDG_DATA_HOME") {
        Ok(value) if !value.trim().is_empty() => dirs.push(PathBuf::from(value)),
        _ => {
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(home).join(".local/share"));
            }
        }
    }
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let system = if system.trim().is_empty() {
        "/usr/local/share:/usr/share".to_string()
    } else {
        system
    };
    dirs.extend(
        system
            .split(':')
            .filter(|entry| !entry.trim().is_empty())
            .map(PathBuf::from),
    );
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mime(value: &str) -> MimeType {
        MimeType::parse(value).expect("valid mime type")
    }

    fn graph() -> MimeGraph {
        let mut graph = MimeGraph::empty();
        graph.load_aliases("application/x-shellscript text/x-shellscript\n# comment\n\n");
        graph.load_subclasses(
            "text/x-rust text/plain\n\
             text/x-shellscript text/plain\n\
             text/plain text/*\n\
             application/x-desktop text/plain\n",
        );
        graph
    }

    #[test]
    fn an_alias_resolves_to_its_canonical_type() {
        assert_eq!(
            graph().canonical(&mime("application/x-shellscript")),
            mime("text/x-shellscript")
        );
    }

    #[test]
    fn an_unknown_type_is_its_own_canonical_name() {
        assert_eq!(graph().canonical(&mime("text/x-rust")), mime("text/x-rust"));
    }

    #[test]
    fn ancestors_are_nearest_first_and_deduplicated() {
        assert_eq!(
            graph().ancestors(&mime("text/x-rust")),
            vec![mime("text/plain"), mime("text/*")]
        );
    }

    #[test]
    fn resolution_keeps_the_requested_name_alongside_the_canonical_one() {
        let resolution = graph().resolve(&mime("application/x-shellscript"));
        assert_eq!(resolution.primary, mime("text/x-shellscript"));
        assert_eq!(resolution.requested, mime("application/x-shellscript"));
        assert!(resolution.is_primary(&mime("application/x-shellscript")));
        assert!(resolution.is_primary(&mime("text/x-shellscript")));
        assert_eq!(
            resolution.ancestor_distance(&mime("text/plain")),
            Some(0usize)
        );
    }

    #[test]
    fn an_alias_cycle_terminates() {
        let mut graph = MimeGraph::empty();
        graph.load_aliases("a/one a/two\na/two a/one\n");
        assert!(matches!(
            graph.canonical(&mime("a/one")).as_str(),
            "a/one" | "a/two"
        ));
    }

    #[test]
    fn a_subclass_cycle_terminates() {
        let mut graph = MimeGraph::empty();
        graph.load_subclasses("a/one a/two\na/two a/one\n");
        assert_eq!(graph.ancestors(&mime("a/one")), vec![mime("a/two")]);
    }

    #[test]
    fn an_absent_database_knows_nothing_rather_than_guessing() {
        let graph = MimeGraph::from_data_dirs(["/nonexistent/better-os/data"]);
        let resolution = graph.resolve(&mime("text/x-rust"));
        assert_eq!(resolution.primary, mime("text/x-rust"));
        assert!(resolution.ancestors.is_empty());
    }

    #[test]
    fn a_malformed_line_is_skipped_not_guessed() {
        let mut graph = MimeGraph::empty();
        graph.load_subclasses("garbage\nnot-a-mime text/plain\ntext/x-rust text/plain\n");
        assert_eq!(
            graph.ancestors(&mime("text/x-rust")),
            vec![mime("text/plain")]
        );
    }

    fn glob_graph() -> MimeGraph {
        let mut graph = MimeGraph::empty();
        graph.load_globs(
            "# comment\n\
             50:text/x-rust:*.rs\n\
             50:application/gzip:*.gz\n\
             50:application/x-compressed-tar:*.tar.gz\n\
             50:text/x-makefile:Makefile\n\
             50:text/x-c:*.C:cs\n\
             50:image/jpeg:*.[jJ][pP]g\n",
        );
        graph.globs.sort_by(|left, right| {
            right
                .weight
                .cmp(&left.weight)
                .then_with(|| right.specificity().cmp(&left.specificity()))
        });
        graph
    }

    #[test]
    fn a_file_name_is_matched_by_its_most_specific_glob() {
        let graph = glob_graph();
        assert_eq!(
            graph.guess_from_file_name("main.rs"),
            Some(mime("text/x-rust"))
        );
        assert_eq!(
            graph.guess_from_file_name("archive.tar.gz"),
            Some(mime("application/x-compressed-tar"))
        );
        assert_eq!(
            graph.guess_from_file_name("Makefile"),
            Some(mime("text/x-makefile"))
        );
    }

    #[test]
    fn an_unmatched_file_name_reports_nothing_rather_than_a_guess() {
        assert_eq!(glob_graph().guess_from_file_name("mystery"), None);
    }

    #[test]
    fn a_case_sensitive_rule_is_honored_and_an_unsupported_pattern_is_skipped() {
        let graph = glob_graph();
        assert_eq!(graph.guess_from_file_name("Main.C"), Some(mime("text/x-c")));
        assert_eq!(graph.guess_from_file_name("main.c"), None);
        // The bracket pattern is not understood, so it matches nothing rather
        // than matching approximately.
        assert_eq!(graph.guess_from_file_name("photo.jpg"), None);
    }

    #[test]
    fn the_first_data_directory_wins_an_alias_conflict() {
        let mut graph = MimeGraph::empty();
        graph.load_aliases("a/one a/first\n");
        graph.load_aliases("a/one a/second\n");
        assert_eq!(graph.canonical(&mime("a/one")), mime("a/first"));
    }
}

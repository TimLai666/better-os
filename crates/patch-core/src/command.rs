//! Every program Better Patch may run, with its arguments.
//!
//! The list is closed: a [`Command`] is one of these variants, each names a
//! fixed absolute path, and its arguments are fixed strings plus, for one
//! variant, a validated Flatpak application ID. No variant takes a program,
//! an argument list, or text from a file, so nothing read from the machine
//! can become a command. `patch-platform`'s `command` module is the only
//! place that starts a process, and it starts only what [`Command::program`]
//! and [`Command::args`] say.

use std::time::Duration;

use crate::flatpak::AppId;

/// fontconfig's query tool.
pub const FC_MATCH: &str = "/usr/bin/fc-match";
/// fontconfig's cache builder.
pub const FC_CACHE: &str = "/usr/bin/fc-cache";
/// Flatpak's command line.
pub const FLATPAK: &str = "/usr/bin/flatpak";

/// Every program a [`Command`] can name.
pub const PROGRAMS: [&str; 3] = [FC_MATCH, FC_CACHE, FLATPAK];

/// What `fc-match` prints for one answer: the file, a tab, and the first
/// style name. `\t` and `\n` are fontconfig's own format escapes.
pub const FONT_ANSWER_FORMAT: &str = "%{file}\\t%{style[0]}\\n";

/// One font question the fontconfig entry asks. Different families and
/// styles, so a healthy cache answers them with different files.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum FontQuery {
    SansSerif,
    Serif,
    Monospace,
    SansSerifBold,
    SerifItalic,
    TraditionalChinese,
}

impl FontQuery {
    pub const ALL: [FontQuery; 6] = [
        FontQuery::SansSerif,
        FontQuery::Serif,
        FontQuery::Monospace,
        FontQuery::SansSerifBold,
        FontQuery::SerifItalic,
        FontQuery::TraditionalChinese,
    ];

    /// The fontconfig pattern.
    pub fn pattern(self) -> &'static str {
        match self {
            FontQuery::SansSerif => "sans-serif",
            FontQuery::Serif => "serif",
            FontQuery::Monospace => "monospace",
            FontQuery::SansSerifBold => "sans-serif:bold",
            FontQuery::SerifItalic => "serif:italic",
            FontQuery::TraditionalChinese => "sans-serif:lang=zh-tw",
        }
    }
}

/// A program run with fixed arguments.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Command {
    /// `fc-match -f FORMAT PATTERN`: reads, changes nothing of its own.
    MatchFont(FontQuery),
    /// `fc-cache -f`: rebuilds the person's own font cache. Run without
    /// root it can write only that cache.
    RebuildFontCache,
    /// `flatpak override --user --show -- APP`: the person's override for
    /// one application as Flatpak itself reads it.
    ShowFlatpakOverride(AppId),
}

impl Command {
    /// The absolute path of the program; one of [`PROGRAMS`].
    pub fn program(&self) -> &'static str {
        match self {
            Command::MatchFont(_) => FC_MATCH,
            Command::RebuildFontCache => FC_CACHE,
            Command::ShowFlatpakOverride(_) => FLATPAK,
        }
    }

    /// The arguments, in order.
    pub fn args(&self) -> Vec<String> {
        match self {
            Command::MatchFont(query) => vec![
                "-f".to_string(),
                FONT_ANSWER_FORMAT.to_string(),
                query.pattern().to_string(),
            ],
            Command::RebuildFontCache => vec!["-f".to_string()],
            Command::ShowFlatpakOverride(app) => vec![
                "override".to_string(),
                "--user".to_string(),
                "--show".to_string(),
                "--".to_string(),
                app.as_str().to_string(),
            ],
        }
    }

    /// Whether running it changes the machine. Only a change may run one
    /// that does; a read may not.
    pub fn changes_the_machine(&self) -> bool {
        matches!(self, Command::RebuildFontCache)
    }

    /// How long it may run before it is stopped and counted as failed.
    pub fn timeout(&self) -> Duration {
        match self {
            Command::MatchFont(_) | Command::ShowFlatpakOverride(_) => Duration::from_secs(15),
            // A first build over many font directories takes a while.
            Command::RebuildFontCache => Duration::from_secs(300),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_program_is_an_absolute_path_on_the_closed_list() {
        let app = AppId::parse("com.example.App").unwrap();
        let commands = [
            Command::MatchFont(FontQuery::Serif),
            Command::RebuildFontCache,
            Command::ShowFlatpakOverride(app),
        ];
        for command in &commands {
            assert!(command.program().starts_with('/'));
            assert!(PROGRAMS.contains(&command.program()));
        }
    }

    #[test]
    fn the_arguments_are_fixed_and_the_app_id_comes_after_the_end_of_options() {
        assert_eq!(Command::RebuildFontCache.args(), ["-f"]);
        assert_eq!(
            Command::MatchFont(FontQuery::SansSerifBold).args(),
            ["-f", "%{file}\\t%{style[0]}\\n", "sans-serif:bold"]
        );
        let app = AppId::parse("io.example.Editor").unwrap();
        assert_eq!(
            Command::ShowFlatpakOverride(app).args(),
            ["override", "--user", "--show", "--", "io.example.Editor"]
        );
    }

    #[test]
    fn only_the_cache_rebuild_changes_the_machine() {
        assert!(Command::RebuildFontCache.changes_the_machine());
        assert!(!Command::MatchFont(FontQuery::Serif).changes_the_machine());
        let app = AppId::parse("io.example.Editor").unwrap();
        assert!(!Command::ShowFlatpakOverride(app).changes_the_machine());
    }

    #[test]
    fn the_queries_are_distinct_patterns() {
        let mut patterns: Vec<&str> = FontQuery::ALL.iter().map(|q| q.pattern()).collect();
        patterns.sort();
        patterns.dedup();
        assert_eq!(patterns.len(), FontQuery::ALL.len());
    }
}

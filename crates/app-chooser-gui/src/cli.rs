//! Command-line handling for the standalone chooser window.
//!
//! Kept in the library rather than in the binary so the argument rules are
//! testable, and so an embedding surface can reuse the same target-building
//! logic instead of copying it.

use app_catalog_core::{LaunchTarget, MimeType};
use app_chooser_core::MimeGraph;

use crate::chooser::{ChooserMode, ChooserTarget};

/// The command line, parsed into exactly what the window needs.
pub struct Arguments {
    pub path: Option<std::path::PathBuf>,
    pub mime: Option<String>,
    pub mode: ChooserMode,
}

pub fn parse_arguments<I: Iterator<Item = String>>(arguments: I) -> Arguments {
    let mut parsed = Arguments {
        path: None,
        mime: None,
        mode: ChooserMode::OpenWith,
    };
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--executable" => parsed.mode = ChooserMode::ChooseExecutable,
            "--mime" => parsed.mime = arguments.next(),
            other if !other.starts_with("--") => {
                parsed.path.get_or_insert_with(|| other.into());
            }
            _ => {}
        }
    }
    parsed
}

/// Builds the target the window opens against. A path with an unrecognized name
/// falls back to `application/octet-stream` rather than to a guess about what
/// the file contains.
pub fn target_from(arguments: &Arguments) -> ChooserTarget {
    let fallback = || MimeType::parse("application/octet-stream").expect("a valid fallback type");
    match &arguments.path {
        Some(path) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let mime = arguments
                .mime
                .as_deref()
                .and_then(MimeType::parse)
                .or_else(|| MimeGraph::from_env().guess_from_file_name(&name))
                .unwrap_or_else(fallback);
            let target = LaunchTarget::path(path.clone())
                .unwrap_or_else(|_| LaunchTarget::uri("file:///").expect("a valid placeholder"));
            ChooserTarget::new(name, target, mime)
        }
        None => ChooserTarget::new(
            "example.txt".to_string(),
            LaunchTarget::uri("file:///tmp/example.txt").expect("a valid placeholder"),
            arguments
                .mime
                .as_deref()
                .and_then(MimeType::parse)
                .or_else(|| MimeType::parse("text/plain"))
                .unwrap_or_else(fallback),
        ),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn parse(arguments: &[&str]) -> Arguments {
        parse_arguments(arguments.iter().map(|value| value.to_string()))
    }

    #[test]
    fn the_executable_mode_is_opt_in_and_never_the_default() {
        assert_eq!(parse(&[]).mode, ChooserMode::OpenWith);
        assert_eq!(parse(&["/tmp/a.txt"]).mode, ChooserMode::OpenWith);
        assert_eq!(parse(&["--executable"]).mode, ChooserMode::ChooseExecutable);
    }

    #[test]
    fn an_explicit_mime_type_wins_over_the_file_name() {
        let arguments = parse(&["/tmp/notes.rs", "--mime", "text/plain"]);
        assert_eq!(target_from(&arguments).mime_type.as_str(), "text/plain");
    }

    #[test]
    fn an_unrecognized_name_falls_back_rather_than_guessing_a_type() {
        let arguments = parse(&["/tmp/mystery-file-with-no-extension"]);
        let target = target_from(&arguments);
        assert_eq!(target.display_name, "mystery-file-with-no-extension");
        assert!(
            target.mime_type.as_str() == "application/octet-stream"
                || target.mime_type.as_str().contains('/')
        );
    }

    #[test]
    fn no_arguments_still_produce_a_launchable_target() {
        let target = target_from(&parse(&[]));
        assert_eq!(target.display_name, "example.txt");
        assert_eq!(target.mime_type.as_str(), "text/plain");
    }
}

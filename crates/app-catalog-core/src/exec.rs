//! `Exec` parsing, field codes, and argument-vector construction.
//!
//! The whole point of this module is that a launch never becomes a string a
//! shell would look at. An `Exec` value is tokenized once, at parse time, into
//! argument pieces. Selected files and URIs are substituted into those pieces
//! afterwards, so a file named `; rm -rf ~` is one argument and stays one
//! argument no matter what it contains.

use std::path::{Path, PathBuf};

use crate::error::{EntryError, LaunchError};

/// The longest launch target accepted. Paths and URIs beyond this are refused
/// rather than handed to `execve`.
pub const MAX_TARGET_CHARS: usize = 4096;

/// A field code the specification defines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldCode {
    /// `%f` — a single local file path.
    SingleFile,
    /// `%F` — a list of local file paths.
    FileList,
    /// `%u` — a single URI.
    SingleUri,
    /// `%U` — a list of URIs.
    UriList,
    /// `%i` — expands to `--icon <icon>`, or to nothing when the entry has no
    /// icon. Two arguments, not one, which is why it cannot be embedded.
    Icon,
    /// `%c` — the entry's localized name.
    DisplayName,
    /// `%k` — the source path of the desktop entry itself.
    SourcePath,
    /// A code the specification deprecated. Carried so the argument it sits in
    /// can be dropped exactly the way the specification says, instead of being
    /// passed through as a literal `%d`.
    Deprecated(char),
}

impl FieldCode {
    fn from_char(character: char) -> Option<Self> {
        match character {
            'f' => Some(Self::SingleFile),
            'F' => Some(Self::FileList),
            'u' => Some(Self::SingleUri),
            'U' => Some(Self::UriList),
            'i' => Some(Self::Icon),
            'c' => Some(Self::DisplayName),
            'k' => Some(Self::SourcePath),
            'd' | 'D' | 'n' | 'N' | 'v' | 'm' => Some(Self::Deprecated(character)),
            _ => None,
        }
    }

    /// Whether the code expands to a variable number of arguments and so must
    /// stand alone as a whole argument.
    fn expands_to_multiple(self) -> bool {
        matches!(self, Self::FileList | Self::UriList | Self::Icon)
    }
}

/// One piece of one argument: either literal text or a field code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgumentPiece {
    Literal(String),
    Field(FieldCode),
}

/// What kind of launch target an entry accepts, derived from its field codes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TargetAcceptance {
    /// The entry declares no file or URI field code and must be launched with
    /// no targets at all.
    #[default]
    None,
    /// `%f` — one local file per process.
    SingleFile,
    /// `%F` — any number of local files in one process.
    MultipleFiles,
    /// `%u` — one URI per process.
    SingleUri,
    /// `%U` — any number of URIs in one process.
    MultipleUris,
}

impl TargetAcceptance {
    pub fn accepts_targets(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn accepts_multiple(self) -> bool {
        matches!(self, Self::MultipleFiles | Self::MultipleUris)
    }

    pub fn accepts_uris(self) -> bool {
        matches!(self, Self::SingleUri | Self::MultipleUris)
    }

    pub fn accepts_files(self) -> bool {
        matches!(self, Self::SingleFile | Self::MultipleFiles)
    }
}

/// A tokenized `Exec` value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecLine {
    arguments: Vec<Vec<ArgumentPiece>>,
    acceptance: TargetAcceptance,
    program: String,
}

impl ExecLine {
    /// Tokenizes an already-unescaped `Exec` value.
    pub fn parse(value: &str) -> Result<Self, EntryError> {
        let arguments = tokenize(value)?;
        if arguments.is_empty() {
            return Err(EntryError::ExecEmpty);
        }
        let mut acceptance = TargetAcceptance::None;
        for argument in &arguments {
            for piece in argument {
                let ArgumentPiece::Field(code) = piece else {
                    continue;
                };
                if code.expands_to_multiple() && argument.len() != 1 {
                    return Err(EntryError::ExecFieldCodePlacement(match code {
                        FieldCode::FileList => 'F',
                        FieldCode::UriList => 'U',
                        _ => 'i',
                    }));
                }
                let candidate = match code {
                    FieldCode::SingleFile => TargetAcceptance::SingleFile,
                    FieldCode::FileList => TargetAcceptance::MultipleFiles,
                    FieldCode::SingleUri => TargetAcceptance::SingleUri,
                    FieldCode::UriList => TargetAcceptance::MultipleUris,
                    _ => continue,
                };
                if acceptance.accepts_targets() {
                    // The specification allows at most one of %f %F %u %U.
                    return Err(EntryError::ExecMultipleTargetFieldCodes);
                }
                acceptance = candidate;
            }
        }
        let program = match arguments[0].as_slice() {
            [ArgumentPiece::Literal(program)] => program.clone(),
            _ => return Err(EntryError::ExecEmpty),
        };
        Ok(Self {
            arguments,
            acceptance,
            program,
        })
    }

    /// The program name as written in the entry. It is not a resolved path and
    /// must not be treated as one.
    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn acceptance(&self) -> TargetAcceptance {
        self.acceptance
    }

    pub fn arguments(&self) -> &[Vec<ArgumentPiece>] {
        &self.arguments
    }

    /// Builds the argument vectors for a launch. One invocation is returned
    /// per process that must be started: an entry declaring `%f` and handed
    /// three files runs three times, which is what the specification requires
    /// and what a naive string join gets wrong.
    pub fn build(
        &self,
        targets: &[LaunchTarget],
        context: &ExpansionContext<'_>,
    ) -> Result<Vec<Invocation>, LaunchError> {
        if targets.is_empty() {
            return Ok(vec![self.expand_once(&[], context)]);
        }
        if !self.acceptance.accepts_targets() {
            return Err(LaunchError::TargetsNotSupported(targets.len()));
        }
        if self.acceptance.accepts_multiple() {
            return Ok(vec![self.expand_once(targets, context)]);
        }
        targets
            .iter()
            .map(|target| Ok(self.expand_once(std::slice::from_ref(target), context)))
            .collect()
    }

    fn expand_once(&self, targets: &[LaunchTarget], context: &ExpansionContext<'_>) -> Invocation {
        let mut output: Vec<String> = Vec::with_capacity(self.arguments.len() + targets.len());
        for argument in &self.arguments {
            if let [ArgumentPiece::Field(code)] = argument.as_slice() {
                match code {
                    FieldCode::FileList => {
                        output.extend(targets.iter().map(|target| target.as_file_argument()));
                        continue;
                    }
                    FieldCode::UriList => {
                        output.extend(targets.iter().map(|target| target.as_uri_argument()));
                        continue;
                    }
                    FieldCode::Icon => {
                        if let Some(icon) = context.icon {
                            output.push("--icon".to_string());
                            output.push(icon.to_string());
                        }
                        continue;
                    }
                    FieldCode::Deprecated(_) => continue,
                    FieldCode::SingleFile | FieldCode::SingleUri => {
                        // A standalone single-target code with no target left
                        // is dropped rather than passed as an empty argument.
                        if targets.is_empty() {
                            continue;
                        }
                    }
                    FieldCode::DisplayName | FieldCode::SourcePath => {}
                }
            }
            let mut rendered = String::new();
            for piece in argument {
                match piece {
                    ArgumentPiece::Literal(text) => rendered.push_str(text),
                    ArgumentPiece::Field(FieldCode::SingleFile) => {
                        if let Some(target) = targets.first() {
                            rendered.push_str(&target.as_file_argument());
                        }
                    }
                    ArgumentPiece::Field(FieldCode::SingleUri) => {
                        if let Some(target) = targets.first() {
                            rendered.push_str(&target.as_uri_argument());
                        }
                    }
                    ArgumentPiece::Field(FieldCode::DisplayName) => {
                        rendered.push_str(context.display_name)
                    }
                    ArgumentPiece::Field(FieldCode::SourcePath) => {
                        rendered.push_str(&context.source_path.to_string_lossy())
                    }
                    ArgumentPiece::Field(FieldCode::Deprecated(_)) => {}
                    ArgumentPiece::Field(FieldCode::FileList)
                    | ArgumentPiece::Field(FieldCode::UriList)
                    | ArgumentPiece::Field(FieldCode::Icon) => {
                        // Rejected at parse time; unreachable for a parsed line.
                    }
                }
            }
            output.push(rendered);
        }
        let program = output.remove(0);
        Invocation {
            program,
            arguments: output,
        }
    }
}

/// What a field code needs from the record to expand.
#[derive(Clone, Copy, Debug)]
pub struct ExpansionContext<'a> {
    pub icon: Option<&'a str>,
    pub display_name: &'a str,
    pub source_path: &'a Path,
}

/// One process to start: a program and its argument vector, never a string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub program: String,
    pub arguments: Vec<String>,
}

/// A validated thing to open. Construction is the validation boundary: an
/// invalid path or URI never becomes a `LaunchTarget`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchTarget {
    Path(PathBuf),
    Uri(String),
}

impl LaunchTarget {
    /// Accepts an absolute local path. A relative path is refused because the
    /// launched process does not share this process's working directory.
    pub fn path(path: impl Into<PathBuf>) -> Result<Self, LaunchError> {
        let path = path.into();
        let text = path.to_str().ok_or(LaunchError::NonUtf8Path)?;
        if text.contains('\0') {
            return Err(LaunchError::EmbeddedNul);
        }
        if text.chars().count() > MAX_TARGET_CHARS {
            return Err(LaunchError::TargetTooLong(text.chars().count()));
        }
        if !path.is_absolute() {
            return Err(LaunchError::RelativePath);
        }
        Ok(Self::Path(path))
    }

    /// Accepts a URI with a syntactically valid scheme.
    pub fn uri(uri: impl Into<String>) -> Result<Self, LaunchError> {
        let uri = uri.into();
        if uri.contains('\0') {
            return Err(LaunchError::EmbeddedNul);
        }
        if uri.chars().count() > MAX_TARGET_CHARS {
            return Err(LaunchError::TargetTooLong(uri.chars().count()));
        }
        if uri.chars().any(|character| character.is_control()) {
            return Err(LaunchError::InvalidUri);
        }
        let Some((scheme, rest)) = uri.split_once(':') else {
            return Err(LaunchError::InvalidUri);
        };
        let mut scheme_characters = scheme.chars();
        let valid = scheme_characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && scheme_characters.all(|character| {
                character.is_ascii_alphanumeric()
                    || character == '+'
                    || character == '-'
                    || character == '.'
            });
        if !valid || rest.is_empty() {
            return Err(LaunchError::InvalidUri);
        }
        Ok(Self::Uri(uri))
    }

    /// The local path this target names, if it names one. A `file://` URI is
    /// decoded; any other scheme has no local path.
    pub fn local_path(&self) -> Option<PathBuf> {
        match self {
            Self::Path(path) => Some(path.clone()),
            Self::Uri(uri) => {
                let rest = uri.strip_prefix("file://")?;
                // Skip an empty or `localhost` authority.
                let path = match rest.find('/') {
                    Some(0) => rest,
                    Some(index) if &rest[..index] == "localhost" => &rest[index..],
                    _ => return None,
                };
                Some(PathBuf::from(percent_decode(path)))
            }
        }
    }

    fn as_file_argument(&self) -> String {
        match self.local_path() {
            Some(path) => path.to_string_lossy().into_owned(),
            None => match self {
                Self::Uri(uri) => uri.clone(),
                Self::Path(path) => path.to_string_lossy().into_owned(),
            },
        }
    }

    /// This target as a URI. A local path becomes a percent-encoded `file://`
    /// URI; a URI is already one.
    pub fn to_uri(&self) -> String {
        match self {
            Self::Uri(uri) => uri.clone(),
            Self::Path(path) => format!("file://{}", percent_encode(&path.to_string_lossy())),
        }
    }

    fn as_uri_argument(&self) -> String {
        self.to_uri()
    }

    /// Checks this target against what an entry says it can open. An entry
    /// declaring `%f` cannot be handed an `smb://` URI just because the caller
    /// would like it to work.
    pub fn check(&self, acceptance: TargetAcceptance) -> Result<(), LaunchError> {
        if acceptance.accepts_files() && self.local_path().is_none() {
            return Err(LaunchError::UriNotALocalFile);
        }
        Ok(())
    }
}

/// Percent-decodes the path component of a `file://` URI.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                output.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Percent-encodes everything outside the unreserved set, keeping `/` so the
/// result still reads as a path.
fn percent_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/' | b':' | b'@');
        if keep {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

/// Splits an `Exec` value into arguments according to the specification's
/// quoting rules, recording field codes as pieces rather than text.
fn tokenize(value: &str) -> Result<Vec<Vec<ArgumentPiece>>, EntryError> {
    let mut arguments: Vec<Vec<ArgumentPiece>> = Vec::new();
    let mut current: Vec<ArgumentPiece> = Vec::new();
    let mut literal = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut characters = value.chars().peekable();

    fn flush_literal(literal: &mut String, current: &mut Vec<ArgumentPiece>) {
        if !literal.is_empty() {
            current.push(ArgumentPiece::Literal(std::mem::take(literal)));
        }
    }

    while let Some(character) = characters.next() {
        if quoted {
            match character {
                '"' => quoted = false,
                '\\' => {
                    // Inside a quoted argument only these four characters may
                    // be escaped; anything else keeps the backslash.
                    match characters.next() {
                        Some(next @ ('"' | '`' | '$' | '\\')) => literal.push(next),
                        Some(next) => {
                            literal.push('\\');
                            literal.push(next);
                        }
                        None => return Err(EntryError::ExecTrailingEscape),
                    }
                }
                '%' => match characters.next() {
                    Some('%') => literal.push('%'),
                    Some(code) => {
                        let field = FieldCode::from_char(code)
                            .ok_or(EntryError::ExecUnknownFieldCode(code))?;
                        flush_literal(&mut literal, &mut current);
                        current.push(ArgumentPiece::Field(field));
                    }
                    None => literal.push('%'),
                },
                other => literal.push(other),
            }
            continue;
        }
        match character {
            ' ' | '\t' => {
                flush_literal(&mut literal, &mut current);
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '"' => {
                quoted = true;
                started = true;
            }
            '\\' => match characters.next() {
                Some(next) => {
                    started = true;
                    literal.push(next);
                }
                None => return Err(EntryError::ExecTrailingEscape),
            },
            '%' => {
                started = true;
                match characters.next() {
                    Some('%') => literal.push('%'),
                    Some(code) => {
                        let field = FieldCode::from_char(code)
                            .ok_or(EntryError::ExecUnknownFieldCode(code))?;
                        flush_literal(&mut literal, &mut current);
                        current.push(ArgumentPiece::Field(field));
                    }
                    None => literal.push('%'),
                }
            }
            other => {
                started = true;
                literal.push(other);
            }
        }
    }
    if quoted {
        return Err(EntryError::ExecUnterminatedQuote);
    }
    flush_literal(&mut literal, &mut current);
    if started {
        arguments.push(current);
    }
    // A deprecated code standing alone as its whole argument disappears with
    // the argument, per the specification's removal rule.
    arguments.retain(|argument| {
        !matches!(
            argument.as_slice(),
            [ArgumentPiece::Field(FieldCode::Deprecated(_))]
        )
    });
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context<'a>(name: &'a str, source: &'a Path) -> ExpansionContext<'a> {
        ExpansionContext {
            icon: Some("text-editor"),
            display_name: name,
            source_path: source,
        }
    }

    fn build(exec: &str, targets: &[LaunchTarget]) -> Vec<Invocation> {
        let source = Path::new("/usr/share/applications/editor.desktop");
        ExecLine::parse(exec)
            .unwrap()
            .build(targets, &context("Editor", source))
            .unwrap()
    }

    #[test]
    fn splits_plain_arguments() {
        let line = ExecLine::parse("editor --new-window").unwrap();
        assert_eq!(line.program(), "editor");
        assert_eq!(line.acceptance(), TargetAcceptance::None);
        let invocations = build("editor --new-window", &[]);
        assert_eq!(invocations[0].arguments, vec!["--new-window"]);
    }

    #[test]
    fn honors_quoting_so_a_hostile_file_name_stays_one_argument() {
        let invocations = build("\"/opt/my apps/editor\" --flag", &[]);
        assert_eq!(invocations[0].program, "/opt/my apps/editor");
        assert_eq!(invocations[0].arguments, vec!["--flag"]);

        let target = LaunchTarget::path("/home/user/; rm -rf ~/Documents.txt").unwrap();
        let invocations = build("editor %f", std::slice::from_ref(&target));
        assert_eq!(
            invocations[0].arguments,
            vec!["/home/user/; rm -rf ~/Documents.txt"]
        );
    }

    #[test]
    fn honors_escapes_inside_quotes() {
        let invocations = build("editor \"a\\\"b\" \"c\\\\d\" \"e\\$f\"", &[]);
        assert_eq!(invocations[0].arguments, vec!["a\"b", "c\\d", "e$f"]);
    }

    #[test]
    fn rejects_an_unterminated_quote() {
        assert_eq!(
            ExecLine::parse("editor \"unterminated").unwrap_err(),
            EntryError::ExecUnterminatedQuote
        );
    }

    #[test]
    fn rejects_an_empty_exec() {
        assert_eq!(ExecLine::parse("   ").unwrap_err(), EntryError::ExecEmpty);
    }

    #[test]
    fn single_file_code_runs_one_process_per_file() {
        let targets = vec![
            LaunchTarget::path("/tmp/one.txt").unwrap(),
            LaunchTarget::path("/tmp/two.txt").unwrap(),
        ];
        let invocations = build("editor %f", &targets);
        assert_eq!(invocations.len(), 2);
        assert_eq!(invocations[0].arguments, vec!["/tmp/one.txt"]);
        assert_eq!(invocations[1].arguments, vec!["/tmp/two.txt"]);
    }

    #[test]
    fn file_list_code_runs_one_process_with_every_file() {
        let targets = vec![
            LaunchTarget::path("/tmp/one.txt").unwrap(),
            LaunchTarget::path("/tmp/two.txt").unwrap(),
        ];
        let invocations = build("editor %F", &targets);
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].arguments,
            vec!["/tmp/one.txt", "/tmp/two.txt"]
        );
    }

    #[test]
    fn uri_codes_convert_paths_and_file_codes_convert_uris() {
        let path = LaunchTarget::path("/tmp/a b.txt").unwrap();
        let invocations = build("browser %U", std::slice::from_ref(&path));
        assert_eq!(invocations[0].arguments, vec!["file:///tmp/a%20b.txt"]);

        let uri = LaunchTarget::uri("file:///tmp/a%20b.txt").unwrap();
        let invocations = build("editor %F", std::slice::from_ref(&uri));
        assert_eq!(invocations[0].arguments, vec!["/tmp/a b.txt"]);
    }

    #[test]
    fn a_non_file_uri_is_refused_for_a_file_only_entry() {
        let uri = LaunchTarget::uri("smb://server/share/file.txt").unwrap();
        assert_eq!(
            uri.check(TargetAcceptance::SingleFile).unwrap_err(),
            LaunchError::UriNotALocalFile
        );
        assert!(uri.check(TargetAcceptance::MultipleUris).is_ok());
    }

    #[test]
    fn an_entry_without_a_target_code_refuses_targets() {
        let line = ExecLine::parse("calculator").unwrap();
        let target = LaunchTarget::path("/tmp/one.txt").unwrap();
        let source = Path::new("/usr/share/applications/calc.desktop");
        assert_eq!(
            line.build(std::slice::from_ref(&target), &context("Calc", source))
                .unwrap_err(),
            LaunchError::TargetsNotSupported(1)
        );
    }

    #[test]
    fn icon_name_and_source_codes_expand() {
        let source = Path::new("/usr/share/applications/editor.desktop");
        let invocations = ExecLine::parse("editor %i %c %k")
            .unwrap()
            .build(&[], &context("Editor", source))
            .unwrap();
        assert_eq!(
            invocations[0].arguments,
            vec![
                "--icon",
                "text-editor",
                "Editor",
                "/usr/share/applications/editor.desktop"
            ]
        );
    }

    #[test]
    fn icon_code_expands_to_nothing_without_an_icon() {
        let source = Path::new("/usr/share/applications/editor.desktop");
        let invocations = ExecLine::parse("editor %i --flag")
            .unwrap()
            .build(
                &[],
                &ExpansionContext {
                    icon: None,
                    display_name: "Editor",
                    source_path: source,
                },
            )
            .unwrap();
        assert_eq!(invocations[0].arguments, vec!["--flag"]);
    }

    #[test]
    fn deprecated_codes_are_dropped() {
        let invocations = build("editor %d %D %n %N %v %m --flag", &[]);
        assert_eq!(invocations[0].arguments, vec!["--flag"]);
    }

    #[test]
    fn a_doubled_percent_is_a_literal_percent() {
        let invocations = build("editor 100%%", &[]);
        assert_eq!(invocations[0].arguments, vec!["100%"]);
    }

    #[test]
    fn rejects_an_unknown_field_code() {
        assert_eq!(
            ExecLine::parse("editor %z").unwrap_err(),
            EntryError::ExecUnknownFieldCode('z')
        );
    }

    #[test]
    fn rejects_more_than_one_target_field_code() {
        assert_eq!(
            ExecLine::parse("editor %f %U").unwrap_err(),
            EntryError::ExecMultipleTargetFieldCodes
        );
    }

    #[test]
    fn rejects_a_list_field_code_embedded_in_an_argument() {
        assert_eq!(
            ExecLine::parse("editor --files=%F").unwrap_err(),
            EntryError::ExecFieldCodePlacement('F')
        );
        assert_eq!(
            ExecLine::parse("editor --icon%i").unwrap_err(),
            EntryError::ExecFieldCodePlacement('i')
        );
    }

    #[test]
    fn an_embedded_single_target_code_is_substituted_in_place() {
        let target = LaunchTarget::path("/tmp/one.txt").unwrap();
        let invocations = build("editor --file=%f", std::slice::from_ref(&target));
        assert_eq!(invocations[0].arguments, vec!["--file=/tmp/one.txt"]);
    }

    #[test]
    fn launch_targets_are_validated_on_construction() {
        assert_eq!(
            LaunchTarget::path("relative/path.txt").unwrap_err(),
            LaunchError::RelativePath
        );
        assert_eq!(
            LaunchTarget::uri("not-a-uri").unwrap_err(),
            LaunchError::InvalidUri
        );
        assert_eq!(
            LaunchTarget::uri("1http://example.com").unwrap_err(),
            LaunchError::InvalidUri
        );
        assert_eq!(
            LaunchTarget::path("/tmp/a\0b").unwrap_err(),
            LaunchError::EmbeddedNul
        );
        let long = format!("/tmp/{}", "a".repeat(MAX_TARGET_CHARS));
        assert!(matches!(
            LaunchTarget::path(long).unwrap_err(),
            LaunchError::TargetTooLong(_)
        ));
    }
}

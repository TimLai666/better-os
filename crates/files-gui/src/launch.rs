//! What `better-files` does with the arguments it was started with.
//!
//! The desktop entry declares `Exec=better-files %U` and `inode/directory`, so
//! the desktop hands this binary a `file://` URI whenever a folder is opened
//! from a browser's download bar, a terminal, or another file manager. A person
//! at a terminal hands it a plain path instead. Both land here, and none of it
//! needs a window: the parsing is pure, and the one question that needs the
//! filesystem — is this a folder, a file, missing, or unreadable — goes through
//! [`TargetProbe`] so the tests can answer it.
//!
//! The option rule is `manager-gui`'s: `--help` and `--version` (and their
//! short forms) are answered, and any other argument that starts with `-` is
//! refused with exit 2 rather than opening a window a person waiting at a
//! terminal did not ask for.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use files_core::{LocalPath, Location};

/// What the process should do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    /// Open the window, at this request.
    Open(StartRequest),
    /// Print this to standard output and exit 0.
    Print(String),
    /// Print this to standard error and exit 2.
    Refuse(String),
}

/// The location arguments, before anything has been checked.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartRequest {
    /// The first location argument, exactly as it arrived.
    pub target: Option<OsString>,
    /// How many further location arguments there were. They are not opened.
    pub ignored: usize,
}

/// Why the location that was given could not be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartProblem {
    /// A URI with a scheme other than `file`, or a `file` URI naming another
    /// host. Better Files opens folders on this computer only.
    NotLocal,
    /// A URI that does not decode: a broken percent escape, an escaped `/` or
    /// NUL, or a fragment.
    Malformed,
    NotFound,
    /// It exists and this user may not list it.
    Unreadable,
}

/// What a given location resolved to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartTarget {
    Folder(LocalPath),
    /// A file: its folder is opened and the file is selected.
    File {
        folder: LocalPath,
        name: String,
    },
}

/// What the filesystem says about one path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    /// A directory this user can list.
    Folder,
    /// Anything else that exists: a regular file, a device node, a socket.
    File,
    Missing,
    /// A directory this user may not list, or a path the kernel refused to
    /// look at.
    Unreadable,
}

/// The one filesystem question the start logic asks.
pub trait TargetProbe {
    fn kind(&self, path: &Path) -> TargetKind;
}

/// The real answer.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostProbe;

impl TargetProbe for HostProbe {
    fn kind(&self, path: &Path) -> TargetKind {
        // `metadata` follows a symlink, so a link to a folder is a folder and
        // opens under the name it was given.
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => match std::fs::read_dir(path) {
                Ok(_) => TargetKind::Folder,
                Err(_) => TargetKind::Unreadable,
            },
            Ok(_) => TargetKind::File,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                TargetKind::Unreadable
            }
            // Not there, a dangling link, or a path through something that is
            // not a directory: in every case there is nothing to open.
            Err(_) => TargetKind::Missing,
        }
    }
}

/// Where the window starts, and what it says about how it got there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartPlan {
    pub target: Option<StartTarget>,
    pub problem: Option<StartProblem>,
    pub ignored: usize,
}

impl StartPlan {
    /// The location the first tab opens.
    pub fn location(&self, default: Location) -> Location {
        match &self.target {
            Some(StartTarget::Folder(path)) | Some(StartTarget::File { folder: path, .. }) => {
                Location::Local(path.clone())
            }
            None => default,
        }
    }

    /// The entry to select once the folder is listed, by its display name.
    pub fn selection(&self) -> Option<&str> {
        match &self.target {
            Some(StartTarget::File { name, .. }) => Some(name),
            _ => None,
        }
    }

    /// Whether the window has anything to say about how it started.
    pub fn needs_notice(&self) -> bool {
        self.problem.is_some() || self.ignored > 0
    }
}

const HELP: &str = "\
better-files — Better OS 檔案管理員 / the Better OS file manager

用法 / Usage:
  better-files                 開啟家目錄 / open your home folder
  better-files <資料夾或檔案>  開啟資料夾；檔案則開啟所在資料夾並選取它
  better-files <folder-or-file>
                               open a folder, or a file's folder with the file selected
                               (a path or a file:// URI; only the first one is opened)
  better-files --version       顯示版本 / print the version
  better-files --help          顯示這段說明 / print this help";

/// Reads the argument list, without the program name.
pub fn invocation(arguments: &[OsString]) -> Invocation {
    // The first option decides, wherever it is: `better-files /tmp --help`
    // prints help, and `better-files /tmp -x` opens nothing.
    if let Some(option) = arguments
        .iter()
        .find(|argument| argument.as_bytes().first() == Some(&b'-'))
    {
        return match option.as_bytes() {
            b"--help" | b"-h" => Invocation::Print(HELP.to_string()),
            b"--version" | b"-V" => {
                Invocation::Print(format!("better-files {}", env!("CARGO_PKG_VERSION")))
            }
            _ => {
                let shown = option.to_string_lossy();
                Invocation::Refuse(format!(
                    "better-files：不認得「{shown}」這個選項。請執行 better-files --help 查看用法。\n\
                     better-files: unknown option \"{shown}\". Run better-files --help for usage."
                ))
            }
        };
    }
    Invocation::Open(StartRequest {
        target: arguments.first().cloned(),
        ignored: arguments.len().saturating_sub(1),
    })
}

/// Turns a request into a plan. `cwd` resolves a relative path typed at a
/// terminal; the desktop never sends one.
pub fn plan_start(
    request: &StartRequest,
    cwd: Option<&Path>,
    probe: &dyn TargetProbe,
) -> StartPlan {
    let (target, problem) = match &request.target {
        None => (None, None),
        Some(raw) => match resolve_target(raw, cwd, probe) {
            Ok(target) => (Some(target), None),
            Err(problem) => (None, Some(problem)),
        },
    };
    StartPlan {
        target,
        problem,
        ignored: request.ignored,
    }
}

/// Resolves one argument to a folder or a file.
pub fn resolve_target(
    raw: &OsStr,
    cwd: Option<&Path>,
    probe: &dyn TargetProbe,
) -> Result<StartTarget, StartProblem> {
    let path = argument_path(raw, cwd)?;
    let local = LocalPath::new(path).map_err(|_| StartProblem::Malformed)?;
    match probe.kind(local.as_path()) {
        TargetKind::Folder => Ok(StartTarget::Folder(local)),
        TargetKind::Missing => Err(StartProblem::NotFound),
        TargetKind::Unreadable => Err(StartProblem::Unreadable),
        TargetKind::File => {
            // A file is shown in its folder, so the folder has to be one this
            // user can list; otherwise there is nothing to select it in.
            let folder = local.parent().ok_or(StartProblem::NotFound)?;
            match probe.kind(folder.as_path()) {
                TargetKind::Folder => Ok(StartTarget::File {
                    name: local.file_name(),
                    folder,
                }),
                TargetKind::Missing | TargetKind::File => Err(StartProblem::NotFound),
                TargetKind::Unreadable => Err(StartProblem::Unreadable),
            }
        }
    }
}

/// Turns one argument into an absolute path, without touching the filesystem.
///
/// The order is GIO's `g_file_new_for_commandline_arg`: an absolute path is a
/// path, something that starts with a URI scheme is a URI, and everything else
/// is relative to the working directory.
pub fn argument_path(raw: &OsStr, cwd: Option<&Path>) -> Result<PathBuf, StartProblem> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return Err(StartProblem::Malformed);
    }
    if bytes[0] == b'/' {
        return Ok(PathBuf::from(raw));
    }
    if let Some(scheme) = uri_scheme(bytes) {
        if scheme.eq_ignore_ascii_case(b"file") {
            return decode_file_uri(bytes);
        }
        return Err(StartProblem::NotLocal);
    }
    match cwd {
        Some(cwd) => Ok(cwd.join(raw)),
        None => Err(StartProblem::NotFound),
    }
}

/// The scheme, when the argument starts with one: a letter, then letters,
/// digits, `+`, `-`, or `.`, then a colon (RFC 3986).
fn uri_scheme(bytes: &[u8]) -> Option<&[u8]> {
    let colon = bytes.iter().position(|byte| *byte == b':')?;
    let scheme = &bytes[..colon];
    let (first, rest) = scheme.split_first()?;
    (first.is_ascii_alphabetic()
        && rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')))
    .then_some(scheme)
}

/// Decodes a `file:` URI into a path, the way GLib's `g_filename_from_uri`
/// does.
///
/// The host must be empty or `localhost`; any other host is a folder on
/// another computer. A fragment is refused, an escaped `/` or NUL is refused
/// because it would change which path is meant, and the decoded bytes become
/// the path as they are, so a name that is not UTF-8 is not mangled on the
/// way in.
pub fn decode_file_uri(uri: &[u8]) -> Result<PathBuf, StartProblem> {
    let colon = uri
        .iter()
        .position(|byte| *byte == b':')
        .ok_or(StartProblem::Malformed)?;
    if !uri[..colon].eq_ignore_ascii_case(b"file") {
        return Err(StartProblem::NotLocal);
    }
    let rest = &uri[colon + 1..];
    if rest.contains(&b'#') {
        return Err(StartProblem::Malformed);
    }
    let path = if let Some(after) = rest.strip_prefix(b"//") {
        let slash = after
            .iter()
            .position(|byte| *byte == b'/')
            .ok_or(StartProblem::Malformed)?;
        let host = &after[..slash];
        if !host.is_empty() && !host.eq_ignore_ascii_case(b"localhost") {
            return Err(StartProblem::NotLocal);
        }
        &after[slash..]
    } else if rest.first() == Some(&b'/') {
        rest
    } else {
        return Err(StartProblem::Malformed);
    };
    Ok(PathBuf::from(OsString::from_vec(percent_decode(path)?)))
}

fn percent_decode(input: &[u8]) -> Result<Vec<u8>, StartProblem> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            out.push(input[index]);
            index += 1;
            continue;
        }
        let high = input.get(index + 1).and_then(|byte| hex_value(*byte));
        let low = input.get(index + 2).and_then(|byte| hex_value(*byte));
        let (Some(high), Some(low)) = (high, low) else {
            return Err(StartProblem::Malformed);
        };
        let byte = high * 16 + low;
        if byte == b'/' || byte == 0 {
            return Err(StartProblem::Malformed);
        }
        out.push(byte);
        index += 3;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn open(target: Option<&str>, ignored: usize) -> Invocation {
        Invocation::Open(StartRequest {
            target: target.map(OsString::from),
            ignored,
        })
    }

    struct FakeProbe(HashMap<PathBuf, TargetKind>);

    impl FakeProbe {
        fn new(entries: &[(&str, TargetKind)]) -> Self {
            Self(
                entries
                    .iter()
                    .map(|(path, kind)| (PathBuf::from(path), *kind))
                    .collect(),
            )
        }
    }

    impl TargetProbe for FakeProbe {
        fn kind(&self, path: &Path) -> TargetKind {
            self.0.get(path).copied().unwrap_or(TargetKind::Missing)
        }
    }

    fn local(path: &str) -> LocalPath {
        LocalPath::new(path).unwrap()
    }

    // --- Options ----------------------------------------------------------

    #[test]
    fn no_argument_opens_the_default_location() {
        assert_eq!(invocation(&[]), open(None, 0));
    }

    #[test]
    fn help_and_version_are_answered_rather_than_opening_a_window() {
        for flag in ["--help", "-h"] {
            match invocation(&args(&[flag])) {
                Invocation::Print(text) => assert!(text.contains("better-files"), "{text}"),
                other => panic!("{flag}: {other:?}"),
            }
        }
        for flag in ["--version", "-V"] {
            match invocation(&args(&[flag])) {
                Invocation::Print(text) => {
                    assert_eq!(text, format!("better-files {}", env!("CARGO_PKG_VERSION")))
                }
                other => panic!("{flag}: {other:?}"),
            }
        }
    }

    #[test]
    fn any_other_option_is_refused_wherever_it_appears() {
        for list in [
            vec!["--new-window"],
            vec!["-x"],
            vec!["-"],
            vec!["/tmp", "--select"],
        ] {
            match invocation(&args(&list)) {
                Invocation::Refuse(text) => {
                    assert!(text.contains("better-files"), "{text}");
                    assert!(text.contains("--help"), "{text}");
                }
                other => panic!("{list:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn the_first_location_is_taken_and_the_rest_are_counted() {
        assert_eq!(invocation(&args(&["/a"])), open(Some("/a"), 0));
        assert_eq!(invocation(&args(&["/a", "/b"])), open(Some("/a"), 1));
        assert_eq!(
            invocation(&args(&["file:///a", "/b", "/c"])),
            open(Some("file:///a"), 2)
        );
    }

    #[test]
    fn a_path_that_is_not_utf8_is_still_a_location() {
        let raw = OsString::from_vec(b"/home/tim/caf\xe9".to_vec());
        assert_eq!(
            invocation(std::slice::from_ref(&raw)),
            Invocation::Open(StartRequest {
                target: Some(raw.clone()),
                ignored: 0,
            })
        );
    }

    // --- URIs -------------------------------------------------------------

    #[test]
    fn percent_encoded_uris_decode_spaces_and_non_ascii_names() {
        assert_eq!(
            decode_file_uri(b"file:///home/tim/My%20Files").unwrap(),
            PathBuf::from("/home/tim/My Files")
        );
        assert_eq!(
            decode_file_uri(b"file:///home/tim/%E6%96%87%E4%BB%B6").unwrap(),
            PathBuf::from("/home/tim/文件")
        );
        // An unescaped non-ASCII name is taken as it is.
        assert_eq!(
            decode_file_uri("file:///home/tim/相片".as_bytes()).unwrap(),
            PathBuf::from("/home/tim/相片")
        );
        // A name that is not UTF-8 decodes to its bytes, not to U+FFFD.
        assert_eq!(
            decode_file_uri(b"file:///home/tim/caf%E9").unwrap(),
            PathBuf::from(OsString::from_vec(b"/home/tim/caf\xe9".to_vec()))
        );
        assert_eq!(
            decode_file_uri(b"file:///a/100%25").unwrap(),
            PathBuf::from("/a/100%")
        );
    }

    #[test]
    fn the_local_host_forms_are_accepted() {
        for uri in [
            "file:///tmp/x",
            "file://localhost/tmp/x",
            "FILE:///tmp/x",
            "file:/tmp/x",
        ] {
            assert_eq!(
                decode_file_uri(uri.as_bytes()).unwrap(),
                PathBuf::from("/tmp/x"),
                "{uri}"
            );
        }
    }

    #[test]
    fn a_uri_this_computer_cannot_open_is_not_local() {
        for uri in [
            "file://server/share",
            "https://example.com/a",
            "smb://server/share",
            "trash:///",
        ] {
            assert_eq!(
                argument_path(OsStr::new(uri), None),
                Err(StartProblem::NotLocal),
                "{uri}"
            );
        }
    }

    #[test]
    fn a_uri_that_does_not_decode_is_malformed() {
        for uri in [
            "file:///a%2Fb",
            "file:///a%00b",
            "file:///a%zz",
            "file:///a%4",
            "file:///a#top",
            "file://",
            "file:relative",
        ] {
            assert_eq!(
                decode_file_uri(uri.as_bytes()),
                Err(StartProblem::Malformed),
                "{uri}"
            );
        }
    }

    // --- Plain paths ------------------------------------------------------

    #[test]
    fn a_plain_path_is_absolute_or_resolved_against_the_working_directory() {
        assert_eq!(
            argument_path(OsStr::new("/srv/data"), None).unwrap(),
            PathBuf::from("/srv/data")
        );
        assert_eq!(
            argument_path(OsStr::new("Downloads"), Some(Path::new("/home/tim"))).unwrap(),
            PathBuf::from("/home/tim/Downloads")
        );
        assert_eq!(
            argument_path(OsStr::new("."), Some(Path::new("/home/tim"))).unwrap(),
            PathBuf::from("/home/tim/.")
        );
        // A plain path is not decoded: `%20` in a real name stays.
        assert_eq!(
            argument_path(OsStr::new("/a%20b"), None).unwrap(),
            PathBuf::from("/a%20b")
        );
        assert_eq!(
            argument_path(OsStr::new("Downloads"), None),
            Err(StartProblem::NotFound)
        );
        assert_eq!(
            argument_path(OsStr::new(""), Some(Path::new("/home/tim"))),
            Err(StartProblem::Malformed)
        );
    }

    // --- Resolution -------------------------------------------------------

    #[test]
    fn a_folder_opens_itself_and_a_file_opens_its_folder_with_it_selected() {
        let probe = FakeProbe::new(&[
            ("/home/tim/My Files", TargetKind::Folder),
            ("/home/tim", TargetKind::Folder),
            ("/home/tim/report.pdf", TargetKind::File),
        ]);
        assert_eq!(
            resolve_target(OsStr::new("file:///home/tim/My%20Files"), None, &probe),
            Ok(StartTarget::Folder(local("/home/tim/My Files")))
        );
        assert_eq!(
            resolve_target(OsStr::new("/home/tim/report.pdf"), None, &probe),
            Ok(StartTarget::File {
                folder: local("/home/tim"),
                name: "report.pdf".to_string(),
            })
        );
    }

    #[test]
    fn missing_and_unreadable_are_told_apart() {
        let probe = FakeProbe::new(&[
            ("/root", TargetKind::Unreadable),
            ("/root/secret.txt", TargetKind::File),
        ]);
        assert_eq!(
            resolve_target(OsStr::new("/nowhere"), None, &probe),
            Err(StartProblem::NotFound)
        );
        assert_eq!(
            resolve_target(OsStr::new("/root"), None, &probe),
            Err(StartProblem::Unreadable)
        );
        // A file whose folder cannot be listed cannot be shown selected.
        assert_eq!(
            resolve_target(OsStr::new("/root/secret.txt"), None, &probe),
            Err(StartProblem::Unreadable)
        );
    }

    // --- The plan ---------------------------------------------------------

    fn home() -> Location {
        Location::local("/home/tim").unwrap()
    }

    #[test]
    fn a_problem_opens_the_default_location_and_says_why() {
        let probe = FakeProbe::new(&[]);
        let plan = plan_start(
            &StartRequest {
                target: Some(OsString::from("https://example.com")),
                ignored: 0,
            },
            None,
            &probe,
        );
        assert_eq!(plan.problem, Some(StartProblem::NotLocal));
        assert_eq!(plan.location(home()), home());
        assert_eq!(plan.selection(), None);
        assert!(plan.needs_notice());
    }

    #[test]
    fn a_file_plan_opens_the_folder_and_selects_the_file() {
        let probe = FakeProbe::new(&[
            ("/srv", TargetKind::Folder),
            ("/srv/notes.txt", TargetKind::File),
        ]);
        let plan = plan_start(
            &StartRequest {
                target: Some(OsString::from("file:///srv/notes.txt")),
                ignored: 0,
            },
            None,
            &probe,
        );
        assert_eq!(plan.problem, None);
        assert_eq!(plan.location(home()), Location::local("/srv").unwrap());
        assert_eq!(plan.selection(), Some("notes.txt"));
        assert!(!plan.needs_notice());
    }

    #[test]
    fn ignored_arguments_are_reported_even_when_the_first_one_opened() {
        let probe = FakeProbe::new(&[("/srv", TargetKind::Folder)]);
        let plan = plan_start(
            &StartRequest {
                target: Some(OsString::from("/srv")),
                ignored: 2,
            },
            None,
            &probe,
        );
        assert_eq!(plan.problem, None);
        assert_eq!(plan.ignored, 2);
        assert_eq!(plan.location(home()), Location::local("/srv").unwrap());
        assert!(plan.needs_notice());
    }

    #[test]
    fn no_argument_plans_the_default_quietly() {
        let plan = plan_start(&StartRequest::default(), None, &FakeProbe::new(&[]));
        assert_eq!(plan.location(home()), home());
        assert!(!plan.needs_notice());
    }

    // --- The real probe ---------------------------------------------------

    #[test]
    fn the_host_probe_tells_folders_files_missing_and_unreadable_apart() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("folder");
        let file = root.path().join("file.txt");
        let locked = root.path().join("locked");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(&file, b"x").unwrap();
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        assert_eq!(HostProbe.kind(&folder), TargetKind::Folder);
        assert_eq!(HostProbe.kind(&file), TargetKind::File);
        assert_eq!(
            HostProbe.kind(&root.path().join("gone")),
            TargetKind::Missing
        );
        assert_eq!(HostProbe.kind(&locked), TargetKind::Unreadable);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

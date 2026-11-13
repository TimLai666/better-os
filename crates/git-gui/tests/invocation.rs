//! What `better-git` does with the arguments it was started with.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use git_gui::i18n::Locale;
use git_gui::{Appearance, Start, StartOptions, invocation};

fn run(values: &[&str]) -> Start {
    let arguments: Vec<OsString> = values.iter().map(OsString::from).collect();
    invocation(&arguments)
}

fn opened(start: Start) -> StartOptions {
    match start {
        Start::Open(options) => options,
        other => panic!("expected the window to open, got {other:?}"),
    }
}

fn refused(start: Start) -> String {
    match start {
        Start::Refuse(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn no_argument_opens_an_empty_dark_window() {
    let options = opened(run(&[]));
    assert_eq!(options.path, None);
    assert_eq!(options.locale, Locale::System);
    assert_eq!(options.appearance, None, "no flag keeps the stored choice");
}

#[test]
fn the_first_plain_argument_is_the_repository() {
    let options = opened(run(&["/work/repo"]));
    assert_eq!(options.path, Some(PathBuf::from("/work/repo")));
    let options = opened(run(&["relative/repo", "--lang", "en-US"]));
    assert_eq!(options.path, Some(PathBuf::from("relative/repo")));
    assert_eq!(options.locale, Locale::EnUs);
}

#[test]
fn flags_and_their_values_are_not_taken_for_a_path() {
    let options = opened(run(&["--lang", "zh-TW", "--appearance", "light", "/r"]));
    assert_eq!(options.locale, Locale::ZhTw);
    assert_eq!(options.appearance, Some(Appearance::Light));
    assert_eq!(options.path, Some(PathBuf::from("/r")));
    let options = opened(run(&["--appearance=system", "--lang=en-US"]));
    assert_eq!(options.appearance, Some(Appearance::System));
    assert_eq!(options.path, None);
}

#[test]
fn a_path_starting_with_a_dash_follows_the_separator() {
    let options = opened(run(&["--", "-odd-name"]));
    assert_eq!(options.path, Some(PathBuf::from("-odd-name")));
    assert!(refused(run(&["-odd-name"])).contains("-odd-name"));
}

#[test]
fn a_path_that_is_not_utf8_is_kept_as_bytes() {
    let raw = OsString::from_vec(b"/work/caf\xe9".to_vec());
    let options = opened(invocation(std::slice::from_ref(&raw)));
    assert_eq!(options.path, Some(PathBuf::from(raw)));
}

#[test]
fn help_and_version_are_answered_without_a_window() {
    match run(&["--version"]) {
        Start::Print(text) => assert_eq!(text, format!("better-git {}", env!("CARGO_PKG_VERSION"))),
        other => panic!("{other:?}"),
    }
    match run(&["/work/repo", "--help"]) {
        Start::Print(text) => assert!(text.contains("--appearance"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unknown_flags_second_paths_and_unknown_values_are_refused() {
    assert!(refused(run(&["--bogus"])).contains("「--bogus」"));
    assert!(refused(run(&["/a", "/b"])).contains("「/b」"));
    assert!(refused(run(&["--lang", "fr"])).contains("--lang fr"));
    assert!(refused(run(&["--appearance=blue"])).contains("--appearance blue"));
}

#[test]
fn a_flag_with_no_value_keeps_its_default() {
    let options = opened(run(&["--lang"]));
    assert_eq!(options.locale, Locale::System);
}

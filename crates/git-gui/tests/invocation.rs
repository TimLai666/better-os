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
fn no_argument_opens_an_empty_window_in_the_stored_appearance() {
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

/// Ticket 110: the appearance the window opens in when no flag names one.
fn stored(file: Option<&str>) -> Appearance {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("git.json");
    if let Some(text) = file {
        std::fs::write(&path, text).expect("write the settings file");
    }
    let settings = git_gui::settings::Settings::load(Some(&path));
    Appearance::stored(&settings.appearance)
}

#[test]
fn the_stored_appearance_defaults_to_the_system_and_keeps_a_choice() {
    assert_eq!(Appearance::default(), Appearance::System);
    // No settings file.
    assert_eq!(stored(None), Appearance::System);
    // A file from before the setting existed.
    assert_eq!(stored(Some(r#"{"language":"zh-TW"}"#)), Appearance::System);
    // A word this version does not know.
    assert_eq!(
        stored(Some(r#"{"appearance":"sepia"}"#)),
        Appearance::System
    );
    assert_eq!(stored(Some(r#"{"appearance":""}"#)), Appearance::System);
    // A stored choice stays; a stored dark cannot be told from one written
    // as the old default, so it stays dark too.
    assert_eq!(stored(Some(r#"{"appearance":"dark"}"#)), Appearance::Dark);
    assert_eq!(stored(Some(r#"{"appearance":"light"}"#)), Appearance::Light);
    assert_eq!(
        stored(Some(r#"{"appearance":"system"}"#)),
        Appearance::System
    );
}

#[test]
fn the_settings_page_lists_the_default_first_and_only_it_follows_the_desktop() {
    assert_eq!(Appearance::ALL[0], Appearance::default());
    for appearance in Appearance::ALL {
        assert_eq!(
            appearance.fluent().follows_desktop(),
            appearance == Appearance::System,
            "{appearance:?}"
        );
        assert_eq!(appearance.fluent().as_str(), appearance.as_str());
    }
}

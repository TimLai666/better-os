//! The `?` sheet lists keys the key table really binds.

use files_gui::keys::{Command, Focus, Modifiers, command_for};
use files_gui::shortcuts::SHORTCUTS;

fn bound(key: &str, modifiers: Modifiers, key_char: Option<&str>) -> Option<Command> {
    command_for(key, modifiers, key_char, Focus::Content)
}

#[test]
fn the_keys_on_the_sheet_do_what_it_says() {
    let alt = Modifiers::alt();
    let shift = Modifiers::shift();
    let control = Modifiers::control();
    let both = Modifiers::control_shift();
    let cases = [
        (bound("home", alt, None), Command::GoHome),
        (bound("d", alt, None), Command::FocusPathField),
        (bound("l", control, None), Command::FocusPathField),
        (bound("/", Modifiers::NONE, Some("/")), Command::FocusSearch),
        (bound("1", control, None), Command::ShowGrid),
        (bound("2", control, None), Command::ShowList),
        (bound("3", control, None), Command::ShowColumns),
        (bound("0", control, None), Command::NormalSize),
        (bound("p", control, None), Command::TogglePin),
        (bound("c", both, None), Command::CopyPath),
        (bound("d", both, None), Command::Duplicate),
        (bound("f10", shift, None), Command::ContextMenu),
        (bound("menu", Modifiers::NONE, None), Command::ContextMenu),
        (bound("enter", alt, None), Command::ContextMenu),
        (bound("/", shift, Some("?")), Command::ShowShortcuts),
        (bound("f1", Modifiers::NONE, None), Command::ShowShortcuts),
        (bound("h", control, None), Command::ToggleHidden),
        (bound("f2", Modifiers::NONE, None), Command::Rename),
        (bound("delete", shift, None), Command::DeletePermanently),
        (bound("z", control, None), Command::Undo),
        (bound("n", control, None), Command::NewWindow),
        (
            bound(
                "n",
                Modifiers {
                    control: true,
                    shift: false,
                    alt: true,
                },
                None,
            ),
            Command::SplitView,
        ),
        (bound("f6", control, None), Command::CyclePane),
        (
            bound("space", Modifiers::NONE, None),
            Command::PreviewWindow,
        ),
        (bound("f", both, None), Command::FilterFolder),
        (bound("z", both, None), Command::Redo),
        (bound("y", control, None), Command::Redo),
    ];
    for (got, wanted) in cases {
        assert_eq!(got, Some(wanted));
    }
}

#[test]
fn every_line_reads_in_both_languages() {
    for line in SHORTCUTS {
        assert!(!line.keys.is_empty());
        assert!(!line.english.is_empty() && !line.chinese.is_empty());
    }
    assert!(SHORTCUTS.len() >= 35);
}

#[test]
fn a_plain_letter_still_types_ahead() {
    assert_eq!(
        bound("s", Modifiers::NONE, Some("s")),
        Some(Command::TypeAhead('s'))
    );
}

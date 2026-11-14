//! Presentation-policy tests that need no display backend.
//!
//! What can be asserted without a window: that both locales answer every
//! question the launcher asks them and say different things where the
//! situations differ, that every command has words, that times read the way
//! Windows' Recommended reads them, and that the window is the Start menu's
//! size and still holds the six-column pinned grid. The pages, the keyboard,
//! the menus, and the launch outcomes are covered in `model.rs`; the key
//! path through GPUI in `overlay.rs`.

use crate::i18n::{Locale, command_label, copy, group_label, no_matches, uninstall_detail, when};
use crate::model::{Command, GroupKey, PINNED_COLUMNS};
use crate::{
    DISPLAY_MARGIN, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, START_HEIGHT, START_WIDTH, overlay_size,
};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];

fn every_command() -> Vec<Command> {
    vec![
        Command::Open,
        Command::RunInTerminal,
        Command::PinToStart,
        Command::UnpinFromStart,
        Command::MoveToFront,
        Command::RemoveFromList,
        Command::OpenFileLocation,
        Command::Uninstall(launcher_platform::ManagedComponent {
            id: "better-files".into(),
            display_name: "Better Files".into(),
        }),
        Command::CopyResult,
        Command::OpenPath,
        Command::OpenContainingFolder,
    ]
}

#[test]
fn both_locales_fill_in_every_string_the_launcher_reads() {
    for locale in LOCALES {
        let c = copy(locale);
        for value in [
            c.search_placeholder,
            c.pinned,
            c.all_apps,
            c.back,
            c.recommended,
            c.pinned_empty,
            c.recommended_empty,
            c.best_match,
            c.apps,
            c.files_and_folders,
            c.kind_app,
            c.kind_terminal_app,
            c.kind_folder,
            c.kind_file,
            c.kind_calculation,
            c.loading_title,
            c.refreshing,
            c.empty_library_title,
            c.empty_library_detail,
            c.no_matches_detail,
            c.launch_failed,
            c.launch_failed_advice,
            c.not_saved,
            c.letters_other,
            c.letters_hint,
            c.appearance,
            c.appearance_dark,
            c.appearance_light,
            c.appearance_system,
        ] {
            assert!(!value.trim().is_empty(), "{locale:?} has an empty string");
        }
        for command in every_command() {
            assert!(!command_label(locale, &command).trim().is_empty());
        }
    }
}

#[test]
fn no_string_shows_a_desktop_file_name_or_engineering_words() {
    // The audit found `.desktop` file names under every app and wording
    // about render threads; neither belongs in front of a person.
    for locale in LOCALES {
        let c = copy(locale);
        for value in [
            c.search_placeholder,
            c.loading_title,
            c.empty_library_detail,
            c.launch_failed,
            c.launch_failed_advice,
            c.not_saved,
        ] {
            for word in [".desktop", "render", "thread", "index", "catalog", "MIME"] {
                assert!(!value.contains(word), "{locale:?}: {value}");
            }
        }
    }
}

#[test]
fn the_empty_states_and_the_menu_words_differ_where_the_situations_differ() {
    for locale in LOCALES {
        let c = copy(locale);
        assert_ne!(c.empty_library_detail, c.no_matches_detail);
        assert_ne!(c.pinned_empty, c.recommended_empty);
        assert_ne!(
            command_label(locale, &Command::Open),
            command_label(locale, &Command::RunInTerminal)
        );
        assert_ne!(
            command_label(locale, &Command::PinToStart),
            command_label(locale, &Command::UnpinFromStart)
        );
        assert!(no_matches(locale, " gimpp ").contains("gimpp"));
        assert!(uninstall_detail(locale, "Better Files").contains("Better Manager"));
        assert!(uninstall_detail(locale, "Better Files").contains("Better Files"));
    }
    let english = copy(Locale::EnUs);
    let chinese = copy(Locale::ZhTw);
    assert_ne!(english.search_placeholder, chinese.search_placeholder);
    assert_ne!(english.pinned, chinese.pinned);
}

#[test]
fn a_launch_time_reads_like_windows_recommended() {
    let now = 1_000_000;
    let cases = [
        (now, "Just now", "剛剛"),
        (now - 60, "1 minute ago", "1 分鐘前"),
        (now - 5 * 60, "5 minutes ago", "5 分鐘前"),
        (now - 3600, "1 hour ago", "1 小時前"),
        (now - 5 * 3600, "5 hours ago", "5 小時前"),
        (now - 30 * 3600, "Yesterday", "昨天"),
        (now - 4 * 86_400, "4 days ago", "4 天前"),
    ];
    for (then, english, chinese) in cases {
        assert_eq!(when(Locale::EnUs, now, then), english);
        assert_eq!(when(Locale::ZhTw, now, then), chinese);
    }
    // A clock that went backwards reads as just now, not as a panic.
    assert_eq!(when(Locale::EnUs, 10, 20), "Just now");
}

#[test]
fn letter_headers_are_the_letter_or_hash_or_other() {
    assert_eq!(group_label(Locale::EnUs, GroupKey::Letter('A')), "A");
    assert_eq!(group_label(Locale::ZhTw, GroupKey::Symbol), "#");
    assert_eq!(group_label(Locale::EnUs, GroupKey::Other), "Other");
    assert_eq!(group_label(Locale::ZhTw, GroupKey::Other), "其他");
}

#[test]
fn the_window_is_the_start_menus_size_and_holds_the_pinned_grid() {
    assert_eq!(overlay_size(1920.0, 1080.0), (START_WIDTH, START_HEIGHT));
    // A small display gets what fits, never below the minimum.
    assert_eq!(
        overlay_size(1280.0, 700.0),
        (START_WIDTH, 700.0 - DISPLAY_MARGIN)
    );
    assert_eq!(
        overlay_size(320.0, 240.0),
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
    );
    // Six 96px tiles and the two 32px margins.
    let grid = f32::from(better_ui::fluent::list::TILE_WIDTH) * PINNED_COLUMNS as f32;
    assert!(MIN_WINDOW_WIDTH >= grid + 64.0);
}

#[test]
fn the_system_locale_resolves_to_one_of_the_two_shipped_locales() {
    assert!(matches!(
        Locale::System.resolved(),
        Locale::EnUs | Locale::ZhTw
    ));
    assert_eq!(Locale::ZhTw.resolved(), Locale::ZhTw);
    assert!(Locale::ZhTw.entry_locale().is_some());
    assert!(Locale::EnUs.entry_locale().is_some());
}

/// Ticket 110: the appearance the overlay opens in, read from its word file.
#[test]
fn the_appearance_follows_the_desktop_unless_one_was_picked() {
    use better_ui::fluent::theme::Appearance;
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory
        .path()
        .join("better-os")
        .join("launcher")
        .join("appearance");
    // Nothing picked yet.
    assert_eq!(Appearance::load(&path), Appearance::System);
    // A word this version does not know.
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "sepia\n").unwrap();
    assert_eq!(Appearance::load(&path), Appearance::System);
    // The file is written only when a person picks, so what is in it stays.
    for picked in [Appearance::Dark, Appearance::Light, Appearance::System] {
        picked.save(&path).unwrap();
        assert_eq!(Appearance::load(&path), picked);
    }
    assert_eq!(copy(Locale::EnUs).appearance_system, "Use system setting");
    assert_eq!(copy(Locale::ZhTw).appearance_system, "跟隨系統");
}

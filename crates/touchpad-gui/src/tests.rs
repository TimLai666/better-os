//! What this window adds over `touchpad-views`, asserted without a display:
//! its action row fits, its tab stops are in order, its appearance file, and
//! that its own source builds no backend command.

use crate::i18n::{Locale, copy};
use crate::{ActionLayout, action_layout};

#[test]
fn the_action_row_fits_the_default_window_at_every_scale_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [c.switch_language, c.refresh, c.discard, c.apply];
        for scale in [1.0, 1.25, 1.5] {
            assert_eq!(
                action_layout(DEFAULT_WINDOW_WIDTH, scale, &labels),
                ActionLayout::Inline,
                "{} does not fit at {scale}x",
                locale.tag()
            );
        }
        // At the minimum window width it still fits unscaled.
        assert_eq!(
            action_layout(crate::MIN_WINDOW_WIDTH, 1.0, &labels),
            ActionLayout::Inline,
            "{} does not fit the minimum window",
            locale.tag()
        );
    }
}

#[test]
fn the_action_row_wraps_rather_than_pushing_a_button_out_of_view() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let labels = [c.switch_language, c.refresh, c.discard, c.apply];
        // Narrow enough that the four buttons genuinely cannot share a line.
        assert_eq!(
            action_layout(640.0, 1.5, &labels),
            ActionLayout::Wrapped,
            "{} claimed four buttons fit in 427 logical pixels",
            locale.tag()
        );
    }
}

/// The size the window opens at.
const DEFAULT_WINDOW_WIDTH: f32 = 1040.0;

/// Ticket 110: the window follows the desktop until the person picks.
#[test]
fn the_appearance_follows_the_desktop_unless_one_was_picked() {
    use crate::app::stored_appearance;
    use better_ui::fluent::theme::Appearance;
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory
        .path()
        .join("better-os")
        .join("touchpad")
        .join("appearance");
    // No config directory at all, and no file in it.
    assert_eq!(stored_appearance(None), Appearance::System);
    assert_eq!(stored_appearance(Some(&path)), Appearance::System);
    // A word this version does not know.
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "sepia\n").unwrap();
    assert_eq!(stored_appearance(Some(&path)), Appearance::System);
    // A picked appearance stays.
    for picked in [Appearance::Dark, Appearance::Light, Appearance::System] {
        picked.save(&path).unwrap();
        assert_eq!(stored_appearance(Some(&path)), picked);
    }
    // The choice is listed with the default first, named as the other
    // windows name it.
    assert_eq!(Appearance::ALL[0], Appearance::default());
    let en = copy(Locale::EnUs);
    let zh = copy(Locale::ZhTw);
    assert_eq!(
        [
            en.appearance_system,
            en.appearance_light,
            en.appearance_dark
        ],
        ["Use system setting", "Light", "Dark"]
    );
    assert_eq!(
        [
            zh.appearance_system,
            zh.appearance_light,
            zh.appearance_dark
        ],
        ["跟隨系統", "淺色", "深色"]
    );
    assert_eq!(zh.appearance, "外觀");
}

#[test]
fn the_screens_stated_tab_stops_are_distinct_and_in_the_order_they_are_drawn() {
    let order = crate::pages_gestures::GESTURE_TAB_ORDER;
    for pair in order.windows(2) {
        assert!(
            pair[0] < pair[1],
            "tab stop {} is not before {}",
            pair[0],
            pair[1]
        );
    }
    let mut sorted = order.to_vec();
    sorted.dedup();
    assert_eq!(sorted.len(), order.len(), "two controls share a tab stop");
}

#[test]
fn the_shipped_source_builds_no_backend_command() {
    // ADR 0005 and Issue #3 both require that the GUI never runs a
    // backend-specific command. Asserting it over this crate's own source is
    // cheaper than reviewing every future change to it.
    let source_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // `std::process::exit` is fine and `--safe-mode` uses it; running a
    // command is what must never happen.
    let forbidden = [
        "gsettings",
        "xinput",
        "dconf ",
        "Command::new",
        "process::Command",
    ];
    for entry in std::fs::read_dir(&source_directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        // This file is `#[cfg(test)]` and names the forbidden tokens on
        // purpose; it is not shipped source.
        if path.file_name().and_then(|name| name.to_str()) == Some("tests.rs") {
            continue;
        }
        // Comments are allowed to name what the code must not do; that is how
        // the rule is explained.
        let text: String = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} names {needle}, which the GUI must never do",
                path.display()
            );
        }
    }
}

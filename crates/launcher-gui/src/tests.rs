//! Presentation-policy tests that need no display backend.
//!
//! What can be asserted without a window: that both locales answer every
//! question the overlay asks them, that the keyboard hints still fit at the
//! minimum size in both languages at every supported interface scale, and that
//! the grid the keyboard walks is the same grid the layout draws. The rows,
//! the selection, and the launch outcomes are covered in `model.rs`, and the
//! window opening at all is covered by the headless smoke run.

use crate::i18n::{Locale, copy};
use crate::{
    HintLayout, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, OVERLAY_COVERAGE, TILE_WIDTH, grid_columns,
    hint_layout, overlay_size,
};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];
const SCALES: [f32; 3] = [1.0, 1.25, 1.5];

fn hints(locale: Locale) -> [&'static str; 3] {
    let c = copy(locale);
    [c.hint_navigate, c.hint_launch, c.hint_close]
}

#[test]
fn both_locales_fill_in_every_string_the_overlay_reads() {
    for locale in LOCALES {
        let c = copy(locale);
        for value in [
            c.search_placeholder,
            c.loading_title,
            c.loading_detail,
            c.refreshing,
            c.empty_library_title,
            c.empty_library_detail,
            c.no_matches_title,
            c.no_matches_detail,
            c.launch_failed,
            c.hint_navigate,
            c.hint_launch,
            c.hint_close,
            c.library_count,
            c.result_count,
        ] {
            assert!(!value.trim().is_empty(), "{locale:?} has an empty string");
        }
    }
}

#[test]
fn the_two_empty_states_are_worded_differently_in_both_locales() {
    // "nothing matched what you typed" and "this machine has no applications"
    // are different situations, and a user who cannot tell them apart cannot
    // tell a broken install from a bad search.
    for locale in LOCALES {
        let c = copy(locale);
        assert_ne!(c.no_matches_title, c.empty_library_title, "{locale:?}");
        assert_ne!(c.no_matches_detail, c.empty_library_detail, "{locale:?}");
    }
}

#[test]
fn the_two_locales_are_actually_different_translations() {
    let english = copy(Locale::EnUs);
    let chinese = copy(Locale::ZhTw);
    assert_ne!(english.search_placeholder, chinese.search_placeholder);
    assert_ne!(english.no_matches_title, chinese.no_matches_title);
    assert_ne!(english.hint_close, chinese.hint_close);
}

#[test]
fn the_hint_row_fits_at_the_minimum_size_in_both_locales_at_every_scale() {
    for locale in LOCALES {
        for scale in SCALES {
            assert_eq!(
                hint_layout(MIN_WINDOW_WIDTH, scale, &hints(locale)),
                HintLayout::Inline,
                "{locale:?} at {scale}x does not fit the hint row at the minimum width"
            );
        }
    }
}

#[test]
fn a_hint_row_that_no_longer_fits_reports_wrapped_rather_than_overflowing() {
    for locale in LOCALES {
        for scale in SCALES {
            let logical = MIN_WINDOW_WIDTH / scale;
            assert_eq!(
                hint_layout(MIN_WINDOW_WIDTH, scale, &hints(locale)),
                hint_layout(logical, 1.0, &hints(locale)),
                "{locale:?} at {scale}x disagrees with its own logical width"
            );
        }
        assert_eq!(
            hint_layout(160.0, 1.0, &hints(locale)),
            HintLayout::Wrapped,
            "{locale:?} claims a row fits when the arithmetic says otherwise"
        );
    }
}

#[test]
fn the_minimum_window_holds_a_search_row_and_a_row_of_tiles() {
    const { assert!(MIN_WINDOW_WIDTH >= TILE_WIDTH * 2.0) };
    const { assert!(MIN_WINDOW_HEIGHT >= 480.0) };
}

#[test]
fn the_grid_widens_with_the_window_and_never_reaches_zero_columns() {
    assert_eq!(grid_columns(0.0, 1.0), 1);
    assert_eq!(grid_columns(-100.0, 1.0), 1);
    assert!(grid_columns(MIN_WINDOW_WIDTH, 1.0) >= 3);

    let mut previous = 0;
    for width in [640.0, 900.0, 1280.0, 1920.0, 2560.0, 3840.0] {
        let columns = grid_columns(width, 1.0);
        assert!(
            columns >= previous,
            "a wider window must not fit fewer tiles"
        );
        assert!(columns <= 12, "the grid is capped so a row stays scannable");
        previous = columns;
    }
}

#[test]
fn a_scaled_up_interface_fits_fewer_tiles_across() {
    let hundred = grid_columns(1920.0, 1.0);
    let one_fifty = grid_columns(1920.0, 1.5);
    assert!(
        one_fifty < hundred,
        "at 150% the same window holds fewer tiles, not the same number"
    );
    assert!(one_fifty >= 1);
}

#[test]
fn the_overlay_covers_most_of_the_display_but_never_less_than_the_minimum() {
    let (width, height) = overlay_size(1920.0, 1080.0);
    assert_eq!(width, 1920.0 * OVERLAY_COVERAGE);
    assert_eq!(height, 1080.0 * OVERLAY_COVERAGE);
    assert!(
        width < 1920.0 && height < 1080.0,
        "near-full-screen leaves the desktop visible at the edges"
    );

    // A display smaller than the minimum still gets a usable overlay.
    let (width, height) = overlay_size(320.0, 240.0);
    assert_eq!(width, MIN_WINDOW_WIDTH);
    assert_eq!(height, MIN_WINDOW_HEIGHT);
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
